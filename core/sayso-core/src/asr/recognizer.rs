//! 扩展提供的流式语音识别：一次录音对应一个识别器进程。
//!
//! 进程约定（stdin / stdout 均为每行一条 JSON）：
//! - stdin：`start`（语言与音频格式）→ 录音期间的 `audio`（base64 编码的 PCM）→ `end`，随后 EOF。
//! - stdout：随时输出 `partial`（当前完整文本，候选框实时显示），最后输出 `final`；出错时输出 `error`。
//! - 收到 `final` 即结束；进程未给出 `final` 就退出视为失败。

use std::path::Path;
use std::process::Stdio;

use base64::Engine;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::mpsc;

use super::{PartialSink, SAMPLE_RATE};
use crate::error::CoreError;
use crate::protocol::{LanguagePair, RecognizerSpec, SCHEMA_VERSION};

/// stderr 只保留末尾部分，附在失败信息中。
const STDERR_LIMIT: usize = 4096;

#[derive(Deserialize)]
struct Reply {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
    message: Option<String>,
}

pub(super) async fn run(
    extension_id: &str,
    directory: &Path,
    spec: &RecognizerSpec,
    languages: LanguagePair,
    chunks: mpsc::UnboundedReceiver<Vec<u8>>,
    partial: PartialSink,
) -> Result<String, CoreError> {
    let failure = |message: String| CoreError::Recognition(message);
    let (program, arguments) = spec.command.split_first().ok_or_else(|| failure("empty recognizer command".into()))?;
    // 带路径的可执行文件相对扩展目录，其余按 PATH 查找。
    let program =
        if program.contains('/') || program.contains('\\') { directory.join(program) } else { program.into() };
    let mut child = Command::new(program)
        .args(arguments)
        .current_dir(directory)
        .env("SAYSO_SCHEMA_VERSION", SCHEMA_VERSION.to_string())
        .env("SAYSO_EXTENSION_ID", extension_id)
        .env("SAYSO_MODE", "recognizer")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // 取消识别时任务被中止，进程随之结束。
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| failure(format!("{}: {error}", spec.command[0])))?;

    // 写入与读取分开进行：识别器读写节奏不受彼此阻塞。
    let stdin = child.stdin.take().expect("stdin is piped");
    let writer = tokio::spawn(feed(stdin, languages, chunks));
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let stderr_tail = tokio::spawn(async move {
        let mut buffer = Vec::new();
        let _ = stderr.read_to_end(&mut buffer).await;
        let start = buffer.len().saturating_sub(STDERR_LIMIT);
        String::from_utf8_lossy(&buffer[start..]).trim().to_string()
    });

    let mut lines = BufReader::new(child.stdout.take().expect("stdout is piped")).lines();
    while let Some(line) = lines.next_line().await? {
        let Ok(reply) = serde_json::from_str::<Reply>(line.trim()) else { continue };
        match reply.kind.as_str() {
            "partial" => partial(reply.text.unwrap_or_default()),
            "final" => {
                writer.abort();
                return Ok(reply.text.unwrap_or_default());
            }
            "error" => return Err(failure(reply.message.unwrap_or_else(|| "recognizer error".into()))),
            _ => {}
        }
    }
    // 输出结束仍没有 `final`：识别器已退出，带上退出码与 stderr 报错。
    let status = child.wait().await?;
    let stderr = stderr_tail.await.unwrap_or_default();
    Err(failure(format!("Recognizer exited ({status}) without a final result {stderr}").trim().to_string()))
}

/// 发送 `start`、全部音频与 `end`，然后关闭 stdin。识别器提前退出时写入失败，直接停止。
async fn feed(mut stdin: ChildStdin, languages: LanguagePair, mut chunks: mpsc::UnboundedReceiver<Vec<u8>>) {
    let start = serde_json::json!({
        "type": "start",
        "schemaVersion": SCHEMA_VERSION,
        "languages": { "target": languages.target.code(), "native": languages.native.code() },
        "audio": { "encoding": "pcm_s16le", "sampleRate": SAMPLE_RATE, "channels": 1 },
    });
    if write_line(&mut stdin, &start).await.is_err() {
        return;
    }
    while let Some(pcm) = chunks.recv().await {
        let audio =
            serde_json::json!({ "type": "audio", "data": base64::engine::general_purpose::STANDARD.encode(pcm) });
        if write_line(&mut stdin, &audio).await.is_err() {
            return;
        }
    }
    let _ = write_line(&mut stdin, &serde_json::json!({ "type": "end" })).await;
}

async fn write_line(stdin: &mut ChildStdin, message: &serde_json::Value) -> std::io::Result<()> {
    let mut line = message.to_string();
    line.push('\n');
    stdin.write_all(line.as_bytes()).await?;
    stdin.flush().await
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Mutex};

    use super::*;

    fn spec(script: &str, directory: &Path) -> RecognizerSpec {
        let path = directory.join("run.sh");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        serde_json::from_value(serde_json::json!({ "title": "Test", "command": ["./run.sh"] })).unwrap()
    }

    /// 识别器读完全部音频后给出结果，期间的 partial 实时转发。
    #[tokio::test]
    async fn streams_partials_and_returns_final() {
        let directory = tempfile::tempdir().unwrap();
        let spec = spec(
            "#!/bin/sh\ncount=0\nwhile read -r line; do\n  case \"$line\" in\n    *'\"type\":\"audio\"'*) count=$((count+1)); echo \"{\\\"type\\\":\\\"partial\\\",\\\"text\\\":\\\"chunk $count\\\"}\" ;;\n    *'\"type\":\"end\"'*) echo \"{\\\"type\\\":\\\"final\\\",\\\"text\\\":\\\"done $count\\\"}\" ;;\n  esac\ndone\n",
            directory.path(),
        );
        let streamed = Arc::new(Mutex::new(Vec::new()));
        let sink = streamed.clone();
        let (audio, chunks) = mpsc::unbounded_channel();
        for _ in 0..3 {
            audio.send(vec![0u8; 320]).unwrap();
        }
        drop(audio);
        let partial: PartialSink = Arc::new(move |text| sink.lock().unwrap().push(text));

        let text = run("test.asr", directory.path(), &spec, LanguagePair::default(), chunks, partial).await.unwrap();
        assert_eq!(text, "done 3");
        assert_eq!(*streamed.lock().unwrap(), ["chunk 1", "chunk 2", "chunk 3"]);
    }

    #[tokio::test]
    async fn exit_without_final_reports_status_and_stderr() {
        let directory = tempfile::tempdir().unwrap();
        let spec = spec("#!/bin/sh\necho broken >&2\nexit 2\n", directory.path());
        let (_audio, chunks) = mpsc::unbounded_channel();

        let error = run("test.asr", directory.path(), &spec, LanguagePair::default(), chunks, Arc::new(|_| {}))
            .await
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("exit status: 2") && message.ends_with("broken"), "{message}");
    }
}
