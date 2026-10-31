//! Soniox 实时识别。`language_hints` 同时给出目标语言与本地语言，支持一句话内混说。

use serde::Deserialize;
use tokio_tungstenite::tungstenite::handshake::client::Request;
use tokio_tungstenite::tungstenite::Message;

use super::streaming::{self, Update};
use super::{server_error, SAMPLE_RATE};
use crate::error::CoreError;
use crate::protocol::LanguagePair;

const ENDPOINT: &str = "wss://stt-rt.soniox.com/transcribe-websocket";
const MODEL: &str = "stt-rt-v5";
/// 端点检测与手动结束的标记，不属于识别文本。
const MARKER_TOKENS: [&str; 2] = ["<end>", "<fin>"];

pub(super) struct Wire {
    api_key: String,
    languages: LanguagePair,
    finalized_text: String,
}

impl Wire {
    pub(super) fn new(api_key: String, languages: LanguagePair) -> Self {
        Wire { api_key, languages, finalized_text: String::new() }
    }
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    tokens: Vec<Token>,
    #[serde(default)]
    finished: bool,
    error_code: Option<i64>,
    error_message: Option<String>,
}

#[derive(Deserialize)]
struct Token {
    text: String,
    #[serde(default)]
    is_final: bool,
}

impl streaming::Wire for Wire {
    fn request(&self) -> Result<Request, CoreError> {
        streaming::request(ENDPOINT)
    }

    fn opening_message(&self) -> Result<Message, CoreError> {
        let payload = serde_json::json!({
            "api_key": self.api_key,
            "model": MODEL,
            "audio_format": "pcm_s16le",
            "sample_rate": SAMPLE_RATE,
            "num_channels": 1,
            "language_hints": [self.languages.target.code(), self.languages.native.code()],
            "language_hints_strict": true,
            "enable_endpoint_detection": true,
        });
        Ok(Message::text(payload.to_string()))
    }

    fn audio_message(&self, pcm: Vec<u8>) -> Message {
        Message::binary(pcm)
    }

    /// 空文本帧表示音频结束，服务端返回剩余结果后发送 finished。
    fn end_of_audio_messages(&self) -> Vec<Message> {
        vec![Message::text("")]
    }

    fn receive(&mut self, message: Message) -> Result<Update, CoreError> {
        let response: Response = match &message {
            Message::Text(text) => serde_json::from_str(text.as_str()),
            Message::Binary(data) => serde_json::from_slice(data),
            _ => return Ok(Update::None),
        }
        .map_err(server_error)?;

        if let Some(code) = response.error_code {
            return Err(server_error(format!("{code} {}", response.error_message.unwrap_or_default())));
        }
        let (finals, pending): (Vec<Token>, Vec<Token>) = response
            .tokens
            .into_iter()
            .filter(|token| !MARKER_TOKENS.contains(&token.text.as_str()))
            .partition(|token| token.is_final);
        self.finalized_text.extend(finals.iter().map(|token| token.text.as_str()));
        if response.finished {
            return Ok(Update::Finished(self.current_text()));
        }
        let mut text = self.finalized_text.clone();
        text.extend(pending.iter().map(|token| token.text.as_str()));
        Ok(Update::Partial(text))
    }

    fn current_text(&self) -> String {
        self.finalized_text.trim().to_string()
    }
}
