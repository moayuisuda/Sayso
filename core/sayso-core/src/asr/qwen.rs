//! 千问 3.1 Message：任务确认后发送 PCM，按句子 ID 更新文字，最终结果才交给命令。
//! 协议：https://help.aliyun.com/zh/model-studio/qwen-asr-message-client-events

use super::streaming::{self, Update};
use super::{server_error, SAMPLE_RATE};
use crate::error::CoreError;
use crate::protocol::LanguagePair;
use serde::Deserialize;
use std::collections::BTreeMap;
use tokio_tungstenite::tungstenite::handshake::client::Request;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

const ENDPOINT: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/inference";
pub(super) const MODEL: &str = "qwen-audio-3.1-asr-flash-message";

pub(super) struct Wire {
    api_key: String,
    languages: LanguagePair,
    context: String,
    task_id: String,
    ready: bool,
    sentences: BTreeMap<u64, Sentence>,
}

impl Wire {
    pub(super) fn new(api_key: String, languages: LanguagePair, context: String) -> Self {
        Self {
            api_key,
            languages,
            context,
            task_id: uuid::Uuid::new_v4().to_string(),
            ready: false,
            sentences: BTreeMap::new(),
        }
    }
    fn event(&self, action: &str, payload: serde_json::Value) -> Message {
        Message::text(serde_json::json!({"header":{"action":action,"task_id":self.task_id,"streaming":"duplex"},"payload":payload}).to_string())
    }
    fn final_text(&self) -> Result<String, CoreError> {
        if self.sentences.values().any(|sentence| !sentence.sentence_end) {
            return Err(server_error("recognition task finished without finalizing every sentence"));
        }
        Ok(self.current_text())
    }
}

#[derive(Deserialize)]
struct ServerEvent {
    header: Header,
    #[serde(default)]
    payload: serde_json::Value,
}
#[derive(Deserialize)]
struct Header {
    event: String,
    task_id: String,
    error_code: Option<String>,
    error_message: Option<String>,
}
#[derive(Deserialize)]
struct Sentence {
    sentence_id: u64,
    text: String,
    #[serde(default)]
    heartbeat: bool,
    #[serde(default)]
    sentence_end: bool,
}

impl streaming::Wire for Wire {
    fn is_ready(&self) -> bool {
        self.ready
    }
    fn request(&self) -> Result<Request, CoreError> {
        let mut request = streaming::request(ENDPOINT)?;
        request
            .headers_mut()
            .insert("Authorization", HeaderValue::from_str(&format!("Bearer {}", self.api_key)).map_err(server_error)?);
        Ok(request)
    }
    fn opening_message(&self) -> Result<Message, CoreError> {
        if self.context.chars().count() > 400 {
            return Err(server_error("Recognition context must not exceed 400 characters"));
        }
        let input = if self.context.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::json!({"context":[{"role":"user","content":[{"type":"input_text","text":self.context}]}]})
        };
        Ok(self.event(
            "run-task",
            serde_json::json!({
                "task_group":"audio", "task":"asr", "function":"recognition", "model":MODEL,
                "parameters":{
                    "format":"pcm", "sample_rate":SAMPLE_RATE,
                    "intermediate_result_enabled":true, "disfluency_removal_enabled":false,
                    "vad_model":"near_meeting_16k",
                    "language_hints":[self.languages.target.code(),self.languages.native.code()]
                }, "input":input
            }),
        ))
    }
    fn audio_message(&self, pcm: Vec<u8>) -> Message {
        Message::binary(pcm)
    }
    fn end_of_audio_messages(&self) -> Vec<Message> {
        vec![self.event("finish-task", serde_json::json!({"input":{}}))]
    }
    fn receive(&mut self, message: Message) -> Result<Update, CoreError> {
        let Message::Text(text) = message else { return Ok(Update::None) };
        let event: ServerEvent = serde_json::from_str(text.as_str()).map_err(server_error)?;
        if event.header.task_id != self.task_id {
            return Err(server_error("recognition task ID mismatch"));
        }
        match event.header.event.as_str() {
            "task-started" => {
                self.ready = true;
                Ok(Update::None)
            }
            "task-failed" => Err(server_error(format!(
                "{}: {}",
                event.header.error_code.unwrap_or_default(),
                event.header.error_message.unwrap_or_default()
            ))),
            "result-generated" => {
                let sentence: Sentence =
                    serde_json::from_value(event.payload["output"]["sentence"].clone()).map_err(server_error)?;
                if sentence.heartbeat {
                    return Ok(Update::None);
                }
                // 最终确认的句子不能被迟到的中间帧覆盖；重复最终帧按相同 ID 替换。
                if self.sentences.get(&sentence.sentence_id).is_some_and(|old| old.sentence_end)
                    && !sentence.sentence_end
                {
                    return Ok(Update::None);
                }
                self.sentences.insert(sentence.sentence_id, sentence);
                Ok(Update::Partial(self.current_text()))
            }
            "task-finished" => Ok(Update::Finished(self.final_text()?)),
            _ => Ok(Update::None),
        }
    }
    fn current_text(&self) -> String {
        self.sentences.values().map(|sentence| sentence.text.as_str()).collect::<String>().trim().to_string()
    }
    fn finish_on_close(&self) -> Result<String, CoreError> {
        Err(server_error("recognition connection closed before task-finished"))
    }
}

use streaming::Wire as _;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Language;
    fn wire() -> Wire {
        Wire::new(
            "test".into(),
            LanguagePair { target: Language::Korean, native: Language::Chinese },
            "词汇：괜찮아".into(),
        )
    }
    fn receive(wire: &mut Wire, event: &str, payload: serde_json::Value) -> Result<Update, CoreError> {
        wire.receive(Message::text(
            serde_json::json!({"header":{"event":event,"task_id":wire.task_id},"payload":payload}).to_string(),
        ))
    }
    fn sentence(wire: &mut Wire, id: u64, text: &str, final_result: bool) {
        receive(
            wire,
            "result-generated",
            serde_json::json!({"output":{"sentence":{"sentence_id":id,"text":text,"sentence_end":final_result}}}),
        )
        .unwrap();
    }
    #[test]
    fn uses_supported_parameters_and_waits_for_task_confirmation() {
        let mut wire = wire();
        let Message::Text(message) = wire.opening_message().unwrap() else { panic!() };
        let request: serde_json::Value = serde_json::from_str(&message).unwrap();
        assert_eq!(request["payload"]["model"], MODEL);
        assert_eq!(request["payload"]["parameters"]["language_hints"], serde_json::json!(["ko", "zh"]));
        assert_eq!(request["payload"]["parameters"]["disfluency_removal_enabled"], false);
        assert_eq!(request["payload"]["input"]["context"][0]["content"][0]["text"], "词汇：괜찮아");
        assert!(!wire.is_ready());
        receive(&mut wire, "task-started", serde_json::json!({})).unwrap();
        assert!(wire.is_ready());
    }
    #[test]
    fn replaces_interim_sentences_with_korean_finals_without_duplicates() {
        let mut wire = wire();
        sentence(&mut wire, 1, "下次再试试吧。", true);
        sentence(&mut wire, 2, "啃枪男啊", false);
        assert!(receive(&mut wire, "task-finished", serde_json::json!({})).is_err());
        sentence(&mut wire, 2, "괜찮아.", true);
        sentence(&mut wire, 2, "괜찮아.", true);
        sentence(&mut wire, 2, "迟到的中间文字", false);
        assert!(
            matches!(receive(&mut wire,"task-finished",serde_json::json!({})),Ok(Update::Finished(text)) if text=="下次再试试吧。괜찮아.")
        );
        assert!(wire.finish_on_close().is_err());
    }
    #[test]
    fn reports_task_failures_and_rejects_foreign_task_events() {
        let mut wire = wire();
        let failure = serde_json::json!({"header":{"event":"task-failed","task_id":wire.task_id,"error_code":"Denied","error_message":"Model unavailable"},"payload":{}});
        assert!(wire
            .receive(Message::text(failure.to_string()))
            .err()
            .unwrap()
            .to_string()
            .contains("Model unavailable"));
        assert!(wire
            .receive(Message::text(r#"{"header":{"event":"task-started","task_id":"other"},"payload":{}}"#))
            .is_err());
    }
}
