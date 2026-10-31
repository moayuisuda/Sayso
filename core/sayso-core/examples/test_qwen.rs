//! 真实服务回放检查：参数为 16 kHz 单声道 Int16 PCM 文件和目标/本地语言代码，API Key 从标准输入读取。
use sayso_core::asr::{AsrConfig, Recognition};
use sayso_core::protocol::{Language, LanguagePair};
use std::io::BufRead;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("Usage: test_qwen audio.pcm target-language native-language; API key on stdin".into());
    }
    let mut api_key = String::new();
    std::io::stdin().lock().read_line(&mut api_key)?;
    let config = AsrConfig::Qwen {
        api_key: api_key.trim().to_string(),
        context: String::new(),
        languages: LanguagePair {
            target: Language::from_code(&args[2]).ok_or("Invalid target language")?,
            native: Language::from_code(&args[3]).ok_or("Invalid native language")?,
        },
    };
    let audio = std::fs::read(&args[1])?;
    if audio.is_empty() || audio.len() % 2 != 0 {
        return Err("Expected nonempty 16-bit PCM audio".into());
    }
    let updates = Arc::new(AtomicUsize::new(0));
    let count = updates.clone();
    let started = std::time::Instant::now();
    let recognition = Recognition::start(
        &tokio::runtime::Handle::current(),
        config,
        reqwest::Client::new(),
        Arc::new(move |_| {
            count.fetch_add(1, Ordering::Relaxed);
        }),
    );
    let feed = recognition.feed();
    // 以实际播放速率送入同一条生产音频管线，覆盖任务确认、增量更新和结束收尾。
    for chunk in audio.chunks(3200) {
        feed.send(chunk.to_vec());
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    drop(feed);
    let transcript = recognition.finish().await?;
    if transcript.is_empty() {
        return Err("Expected a nonempty final transcript".into());
    }
    println!(
        "{}",
        serde_json::json!({"transcript":transcript,"partialUpdates":updates.load(Ordering::Relaxed),"elapsedMs":started.elapsed().as_millis()})
    );
    Ok(())
}
