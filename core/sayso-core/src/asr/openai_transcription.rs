//! OpenAI 兼容的批量识别：录音结束后上传整段 WAV 到 `/audio/transcriptions`。

use std::time::Duration;

use serde::Deserialize;
use tokio::sync::mpsc;

use super::{server_error, SAMPLE_RATE};
use crate::error::CoreError;
use crate::l10n::tr;
use crate::protocol::LanguagePair;

/// 少于 0.3 秒的录音不上传，避免 Whisper 类模型对静音产生幻觉文本。
const MINIMUM_BYTES: usize = SAMPLE_RATE as usize * 2 * 3 / 10;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) async fn run(
    http: &reqwest::Client,
    api_key: &str,
    base_url: &str,
    model: &str,
    languages: LanguagePair,
    context: &str,
    mut chunks: mpsc::UnboundedReceiver<Vec<u8>>,
) -> Result<String, CoreError> {
    let mut audio = Vec::new();
    while let Some(chunk) = chunks.recv().await {
        audio.extend_from_slice(&chunk);
    }
    if audio.len() < MINIMUM_BYTES {
        return Ok(String::new());
    }

    let url = format!("{}/audio/transcriptions", base_url.trim().trim_end_matches('/'));
    if !reqwest::Url::parse(&url).is_ok_and(|url| matches!(url.scheme(), "http" | "https")) {
        return Err(CoreError::Recognition(tr(
            &format!("识别服务地址无效：{base_url}"),
            &format!("Invalid recognition endpoint: {base_url}"),
        )));
    }
    let file = reqwest::multipart::Part::bytes(wav(&audio))
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(server_error)?;
    // 不指定 language，改用提示词告知会出现两种语言，让模型保留原语言转写而不是翻译；个人背景帮助识别专业词汇。
    let prompt = format!(
        "The speaker mixes {} and {}. Transcribe each word in the language it was spoken. {context}",
        languages.target.english_name(),
        languages.native.english_name()
    );
    let form = reqwest::multipart::Form::new()
        .part("file", file)
        .text("model", model.to_string())
        .text("response_format", "json")
        .text("prompt", prompt.trim_end().to_string());

    let mut request = http.post(url).multipart(form).timeout(REQUEST_TIMEOUT);
    // 本地服务可不启用认证；仅配置密钥时发送 Authorization。
    if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }
    let response = request.send().await.map_err(server_error)?;
    let status = response.status();
    let body = response.bytes().await.map_err(server_error)?;
    if !status.is_success() {
        let preview: String = String::from_utf8_lossy(&body).chars().take(300).collect();
        return Err(server_error(format!("HTTP {} {preview}", status.as_u16())));
    }
    #[derive(Deserialize)]
    struct Payload {
        text: String,
    }
    Ok(serde_json::from_slice::<Payload>(&body).map_err(server_error)?.text)
}

/// 为 16kHz 单声道 Int16 PCM 加上 WAV 头。
fn wav(pcm: &[u8]) -> Vec<u8> {
    let byte_rate = SAMPLE_RATE * 2;
    let mut data = Vec::with_capacity(44 + pcm.len());
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    data.extend_from_slice(b"WAVEfmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes()); // PCM
    data.extend_from_slice(&1u16.to_le_bytes()); // 单声道
    data.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    data.extend_from_slice(&byte_rate.to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes()); // 每帧字节数
    data.extend_from_slice(&16u16.to_le_bytes()); // 位深
    data.extend_from_slice(b"data");
    data.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    data.extend_from_slice(pcm);
    data
}
