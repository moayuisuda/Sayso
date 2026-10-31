//! 语音识别：云端流式识别、录完后批量识别，以及扩展提供的子进程识别器。可选的识别来源见 [`source`]。
//!
//! 一次录音对应一个 [`Recognition`]：录音期间 `append` 音频，结束时 `finish` 取得最终文本，或 `cancel` 放弃。
//! 各识别服务只实现「消费音频通道、产出文本」的异步函数，缓冲、结束等待与取消在这里统一处理。

mod openai_transcription;
mod qwen;
mod recognizer;
mod soniox;
pub mod source;
mod streaming;
mod volcengine;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::error::CoreError;
use crate::l10n::tr;
use crate::protocol::{LanguagePair, RecognizerSpec};

/// 识别音频格式：16kHz 单声道 16 位小端 PCM。
pub const SAMPLE_RATE: u32 = 16_000;

/// 云端流式识别在音频结束后等待最终结果的时长。
const STREAMING_FINISH_TIMEOUT: Duration = Duration::from_secs(10);
/// 批量识别要上传整段音频，等待更久。
const BATCH_FINISH_TIMEOUT: Duration = Duration::from_secs(60);

/// 创建识别会话所需的全部参数，由选用的识别来源（见 [`source`]）在每次录音开始时生成。
#[derive(Clone, Debug)]
pub enum AsrConfig {
    Volcengine { api_key: String, resource_id: String },
    Qwen { api_key: String, languages: LanguagePair },
    Soniox { api_key: String, languages: LanguagePair },
    OpenAiCompatible { api_key: String, base_url: String, model: String, languages: LanguagePair },
    Extension { extension_id: String, directory: PathBuf, spec: RecognizerSpec, languages: LanguagePair },
}

impl AsrConfig {
    fn finish_timeout(&self) -> Duration {
        match self {
            Self::Volcengine { .. } | Self::Qwen { .. } | Self::Soniox { .. } => STREAMING_FINISH_TIMEOUT,
            Self::OpenAiCompatible { .. } => BATCH_FINISH_TIMEOUT,
            Self::Extension { spec, .. } => spec.finish_timeout(),
        }
    }
}

/// 实时识别文本（每次为完整的当前文本）的接收方。批量识别不产生中间结果。
pub type PartialSink = Arc<dyn Fn(String) + Send + Sync>;

/// 录音线程向识别会话送音频的入口，可在任意线程使用。
#[derive(Clone)]
pub struct AudioFeed(mpsc::UnboundedSender<Vec<u8>>);

impl AudioFeed {
    pub fn send(&self, pcm: Vec<u8>) {
        // 识别已提前结束（如连接失败）时通道已关闭，结果由 finish 返回。
        let _ = self.0.send(pcm);
    }
}

/// 一次录音的识别会话。
pub struct Recognition {
    audio: mpsc::UnboundedSender<Vec<u8>>,
    result: oneshot::Receiver<Result<String, CoreError>>,
    task: JoinHandle<()>,
    finish_timeout: Duration,
}

impl Recognition {
    /// 立即开始连接识别服务；连接建立前到达的音频留在通道中，连接后按序发送，不会吞掉开头。
    pub fn start(runtime: &Handle, config: AsrConfig, http: reqwest::Client, partial: PartialSink) -> Self {
        let (audio, chunks) = mpsc::unbounded_channel();
        let (sender, result) = oneshot::channel();
        let finish_timeout = config.finish_timeout();
        let task = runtime.spawn(async move {
            let outcome = match config {
                AsrConfig::Volcengine { api_key, resource_id } => {
                    streaming::run(volcengine::Wire::new(api_key, resource_id), chunks, partial).await
                }
                AsrConfig::Qwen { api_key, languages } => {
                    streaming::run(qwen::Wire::new(api_key, languages), chunks, partial).await
                }
                AsrConfig::Soniox { api_key, languages } => {
                    streaming::run(soniox::Wire::new(api_key, languages), chunks, partial).await
                }
                AsrConfig::OpenAiCompatible { api_key, base_url, model, languages } => {
                    openai_transcription::run(&http, &api_key, &base_url, &model, languages, chunks).await
                }
                AsrConfig::Extension { extension_id, directory, spec, languages } => {
                    recognizer::run(&extension_id, &directory, &spec, languages, chunks, partial).await
                }
            };
            let _ = sender.send(outcome.map(|text| text.trim().to_string()));
        });
        Recognition { audio, result, task, finish_timeout }
    }

    /// 音频通道在本会话与全部 [`AudioFeed`] 都释放后关闭，识别服务随即收到音频结束。
    pub fn feed(&self) -> AudioFeed {
        AudioFeed(self.audio.clone())
    }

    /// 结束音频输入并等待最终结果。调用前须先释放全部 [`AudioFeed`]。
    pub async fn finish(self) -> Result<String, CoreError> {
        let Recognition { audio, result, task, finish_timeout } = self;
        drop(audio);
        match tokio::time::timeout(finish_timeout, result).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => Err(server_error("recognition task ended unexpectedly")),
            Err(_) => {
                task.abort();
                Err(CoreError::Recognition(tr("识别超时", "Recognition timed out")))
            }
        }
    }

    /// 放弃本次识别：结束连接或识别器进程。
    pub fn cancel(self) {
        self.task.abort();
    }
}

fn server_error(message: impl std::fmt::Display) -> CoreError {
    CoreError::Recognition(tr(&format!("识别服务出错：{message}"), &format!("Recognition failed: {message}")))
}
