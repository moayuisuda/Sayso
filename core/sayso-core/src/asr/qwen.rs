//! 阿里云百炼千问实时识别（qwen3-asr-flash-realtime），协议与 OpenAI Realtime 类似。
//!
//! - 不指定语言，由模型自动识别：一句话内中日、中英等混说都能识别；指定语言后其他语言会被译成该语言。
//! - 以上下文文本告知目标语言与本地语言：自动识别对短句没有把握，带口音的「こんにちは」会被当作中文音译成「扣你基哇」。
//! - 关闭服务端断句，整段录音作为一句提交：模型能看到完整上下文。按停顿切开后单独识别的短片段
//!   （如日语助词）容易被识别成另一种语言。

use base64::Engine;
use serde::Deserialize;
use tokio_tungstenite::tungstenite::handshake::client::Request;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

use super::streaming::{self, Update};
use super::{server_error, SAMPLE_RATE};
use crate::error::CoreError;
use crate::protocol::LanguagePair;

const ENDPOINT: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/realtime?model=qwen3-asr-flash-realtime";

pub(super) struct Wire {
    api_key: String,
    languages: LanguagePair,
    /// 已提交音频的最终文本。
    finalized_text: String,
}

impl Wire {
    pub(super) fn new(api_key: String, languages: LanguagePair) -> Self {
        Wire { api_key, languages, finalized_text: String::new() }
    }

    /// 识别上下文：说明两种语言可能在一句话内互相夹杂，各按原语言书写。
    fn context(&self) -> String {
        let (target, native) = (self.languages.target.english_name(), self.languages.native.english_name());
        format!(
            "The speaker speaks {target} and {native}. A {target} sentence may contain {native} words, \
             and a {native} sentence may contain {target} words; \
             transcribe every word in the language it was spoken, without translating."
        )
    }
}

#[derive(Deserialize)]
struct ServerEvent {
    #[serde(rename = "type")]
    kind: String,
    /// 中间结果：`text` 为已稳定部分，`stash` 为仍可能变化的尾部。
    #[serde(default)]
    text: String,
    #[serde(default)]
    stash: String,
    /// 最终结果。
    #[serde(default)]
    transcript: String,
    error: Option<ServerError>,
}

#[derive(Deserialize)]
struct ServerError {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
}

fn client_event(event: serde_json::Value) -> Message {
    Message::text(event.to_string())
}

impl streaming::Wire for Wire {
    fn request(&self) -> Result<Request, CoreError> {
        let mut request = streaming::request(ENDPOINT)?;
        let headers = request.headers_mut();
        let authorization = HeaderValue::from_str(&format!("Bearer {}", self.api_key)).map_err(server_error)?;
        headers.insert("Authorization", authorization);
        headers.insert("OpenAI-Beta", HeaderValue::from_static("realtime=v1"));
        Ok(request)
    }

    fn opening_message(&self) -> Result<Message, CoreError> {
        Ok(client_event(serde_json::json!({
            "type": "session.update",
            "session": {
                "input_audio_format": "pcm",
                "sample_rate": SAMPLE_RATE,
                "turn_detection": null,
                "input_audio_transcription": { "corpus": { "text": self.context() } },
            },
        })))
    }

    fn audio_message(&self, pcm: Vec<u8>) -> Message {
        let audio = base64::engine::general_purpose::STANDARD.encode(pcm);
        client_event(serde_json::json!({ "type": "input_audio_buffer.append", "audio": audio }))
    }

    /// 提交整段音频后结束会话：服务端先返回最终结果，再发送 `session.finished`。
    fn end_of_audio_messages(&self) -> Vec<Message> {
        vec![
            client_event(serde_json::json!({ "type": "input_audio_buffer.commit" })),
            client_event(serde_json::json!({ "type": "session.finish" })),
        ]
    }

    fn receive(&mut self, message: Message) -> Result<Update, CoreError> {
        let Message::Text(text) = message else { return Ok(Update::None) };
        let event: ServerEvent = serde_json::from_str(text.as_str()).map_err(server_error)?;
        if let Some(error) = event.error {
            return Err(server_error(format!("{} {}", error.code, error.message).trim()));
        }
        Ok(match event.kind.as_str() {
            "conversation.item.input_audio_transcription.text" => {
                Update::Partial(format!("{}{}{}", self.finalized_text, event.text, event.stash))
            }
            "conversation.item.input_audio_transcription.completed" => {
                self.finalized_text.push_str(&event.transcript);
                Update::Partial(self.finalized_text.clone())
            }
            "session.finished" => Update::Finished(self.current_text()),
            _ => Update::None,
        })
    }

    fn current_text(&self) -> String {
        self.finalized_text.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use streaming::Wire as _;

    fn receive(wire: &mut Wire, event: &str) -> Result<Update, CoreError> {
        wire.receive(Message::text(event))
    }

    #[test]
    fn partials_include_the_unstable_tail_and_finish_with_the_transcript() {
        let mut wire = Wire::new("sk".into(), LanguagePair::default());
        let partial = r#"{"type":"conversation.item.input_audio_transcription.text","text":"週末は","stash":"友達と火"}"#;
        assert!(matches!(receive(&mut wire, partial), Ok(Update::Partial(text)) if text == "週末は友達と火"));

        let completed = r#"{"type":"conversation.item.input_audio_transcription.completed","transcript":"週末は友達と火锅を食べたいです。"}"#;
        receive(&mut wire, completed).unwrap();
        let finished = receive(&mut wire, r#"{"type":"session.finished"}"#);
        assert!(matches!(finished, Ok(Update::Finished(text)) if text == "週末は友達と火锅を食べたいです。"));
    }

    #[test]
    fn reports_server_errors() {
        let mut wire = Wire::new("sk".into(), LanguagePair::default());
        let error = r#"{"type":"error","error":{"code":"InvalidApiKey","message":"Invalid API-key provided."}}"#;
        assert!(receive(&mut wire, error).is_err());
    }
}
