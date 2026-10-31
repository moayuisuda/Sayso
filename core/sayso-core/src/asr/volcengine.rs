//! 火山引擎（豆包）大模型流式识别，使用新版控制台 API Key 鉴权。原生支持中英混说。
//!
//! 二进制帧结构：4 字节头 + [4 字节序号] + 4 字节负载长度（大端）+ 负载。
//! 头部依次为：版本/头长度、消息类型/标志位、序列化/压缩方式、保留字节。

use tokio_tungstenite::tungstenite::handshake::client::Request;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

use super::streaming::{self, Update};
use super::{server_error, SAMPLE_RATE};
use crate::error::CoreError;

const ENDPOINT: &str = "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async";
/// 请求中的用户标识，仅用于服务端统计。
const UID: &str = "sayso";

const FULL_CLIENT_REQUEST: u8 = 0b0001;
const AUDIO_ONLY_REQUEST: u8 = 0b0010;
const SERVER_RESPONSE: u8 = 0b1001;
const SERVER_ERROR: u8 = 0b1111;

const FLAG_NONE: u8 = 0b0000;
const FLAG_POSITIVE_SEQUENCE: u8 = 0b0001;
const FLAG_LAST_PACKET: u8 = 0b0010;
const FLAG_NEGATIVE_SEQUENCE: u8 = 0b0011;
const FLAG_ASYNC_FINAL: u8 = 0b0100;

const SERIALIZATION_JSON: u8 = 0b0001;
const COMPRESSION_NONE: u8 = 0b0000;

pub(super) struct Wire {
    api_key: String,
    resource_id: String,
    current_text: String,
}

impl Wire {
    pub(super) fn new(api_key: String, resource_id: String) -> Self {
        Wire { api_key, resource_id, current_text: String::new() }
    }
}

impl streaming::Wire for Wire {
    fn request(&self) -> Result<Request, CoreError> {
        let mut request = streaming::request(ENDPOINT)?;
        let headers = request.headers_mut();
        let value = |text: &str| HeaderValue::from_str(text).map_err(server_error);
        headers.insert("X-Api-Key", value(&self.api_key)?);
        headers.insert("X-Api-Resource-Id", value(&self.resource_id)?);
        headers.insert("X-Api-Connect-Id", value(&uuid::Uuid::new_v4().to_string())?);
        Ok(request)
    }

    /// 首帧：音频格式与识别参数。中英混说无需指定语言，模型原生支持。
    fn opening_message(&self) -> Result<Message, CoreError> {
        let payload = serde_json::json!({
            "user": { "uid": UID },
            "audio": { "format": "pcm", "codec": "raw", "rate": SAMPLE_RATE, "bits": 16, "channel": 1 },
            "request": {
                "model_name": "bigmodel",
                "result_type": "full",
                "show_utterances": true,
                "enable_punc": true,
                "enable_itn": true,
                // 语义顺滑：去掉「嗯」「那个」等口头语。
                "enable_ddc": true,
                // 二遍识别：分句结束后用整句重新识别，提升准确率。
                "enable_nonstream": true,
            },
        });
        Ok(Message::binary(frame(
            FULL_CLIENT_REQUEST,
            FLAG_NONE,
            SERIALIZATION_JSON,
            &payload.to_string().into_bytes(),
        )))
    }

    fn audio_message(&self, pcm: Vec<u8>) -> Message {
        Message::binary(frame(AUDIO_ONLY_REQUEST, FLAG_NONE, 0, &pcm))
    }

    fn end_of_audio_messages(&self) -> Vec<Message> {
        vec![Message::binary(frame(AUDIO_ONLY_REQUEST, FLAG_LAST_PACKET, 0, &[]))]
    }

    fn receive(&mut self, message: Message) -> Result<Update, CoreError> {
        let Message::Binary(data) = message else { return Ok(Update::None) };
        let response = decode(&data).map_err(server_error)?;
        // result_type=full：每帧都是完整文本；空文本帧不覆盖已有结果。
        if !response.text.is_empty() {
            self.current_text = response.text;
        }
        Ok(if response.is_final {
            Update::Finished(self.current_text.clone())
        } else {
            Update::Partial(self.current_text.clone())
        })
    }

    fn current_text(&self) -> String {
        self.current_text.clone()
    }
}

fn frame(message_type: u8, flags: u8, serialization: u8, payload: &[u8]) -> Vec<u8> {
    let mut data = vec![0b0001_0001, message_type << 4 | flags, serialization << 4 | COMPRESSION_NONE, 0];
    data.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    data.extend_from_slice(payload);
    data
}

/// 识别结果：完整文本（多句已拼接）与是否为最后一帧。
#[derive(Debug, PartialEq)]
struct Response {
    text: String,
    is_final: bool,
}

fn decode(bytes: &[u8]) -> Result<Response, String> {
    let malformed = || "Malformed Volcengine frame".to_string();
    if bytes.len() < 4 {
        return Err(malformed());
    }
    let header_length = usize::from(bytes[0] & 0x0F) * 4;
    let message_type = bytes[1] >> 4;
    let flags = bytes[1] & 0x0F;
    let compression = bytes[2] & 0x0F;

    if message_type == SERVER_ERROR {
        return Err(server_error_message(bytes, header_length));
    }
    if message_type != SERVER_RESPONSE {
        return Err(malformed());
    }
    // 请求未压缩时服务端也不压缩响应。
    if compression != COMPRESSION_NONE {
        return Err("Unexpected compressed Volcengine frame".into());
    }

    let mut offset = header_length;
    if flags == FLAG_POSITIVE_SEQUENCE || flags == FLAG_NEGATIVE_SEQUENCE {
        offset += 4;
    }
    let size = read_u32(bytes, offset).ok_or_else(malformed)?;
    offset += 4;
    let payload = bytes.get(offset..offset + size).ok_or_else(malformed)?;
    let json: serde_json::Value = serde_json::from_slice(payload).map_err(|error| error.to_string())?;
    let result = &json["result"];
    let utterances: Vec<&str> = result["utterances"]
        .as_array()
        .map(|items| items.iter().filter_map(|item| item["text"].as_str()).collect())
        .unwrap_or_default();
    // 服务端拼接多句时总用空格分隔，中文句间不应有空格，因此按句子重新拼接。
    let text = if utterances.is_empty() {
        result["text"].as_str().unwrap_or_default().to_string()
    } else {
        join_utterances(&utterances)
    };
    Ok(Response {
        text: text.trim().to_string(),
        is_final: flags == FLAG_NEGATIVE_SEQUENCE || flags == FLAG_ASYNC_FINAL,
    })
}

/// 错误帧：头 + 4 字节错误码 + 4 字节长度 + UTF-8 错误信息。
fn server_error_message(bytes: &[u8], header_length: usize) -> String {
    let fallback = || "Volcengine server error".to_string();
    let (Some(code), Some(size)) = (read_u32(bytes, header_length), read_u32(bytes, header_length + 4)) else {
        return fallback();
    };
    let start = header_length + 8;
    match bytes.get(start..start + size) {
        Some(message) => format!("{} ({code})", String::from_utf8_lossy(message)),
        None => fallback(),
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<usize> {
    let slice: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_be_bytes(slice) as usize)
}

/// 两侧任一为中日韩字符或后句以标点开头时直接相连，否则以空格分隔。
fn join_utterances(pieces: &[&str]) -> String {
    let mut result = String::new();
    for piece in pieces.iter().map(|piece| piece.trim()).filter(|piece| !piece.is_empty()) {
        if let (Some(last), Some(first)) = (result.chars().last(), piece.chars().next()) {
            let glued = is_cjk(last) || is_cjk(first) || first.is_ascii_punctuation() || is_cjk_punctuation(first);
            if !glued {
                result.push(' ');
            }
        }
        result.push_str(piece);
    }
    result
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x303F | 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF
        | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x2FA1F)
}

fn is_cjk_punctuation(c: char) -> bool {
    matches!(c, '，' | '。' | '、' | '；' | '：' | '？' | '！' | '…' | '”' | '’' | '）' | '》' | '」')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response_frame(flags: u8, payload: &str) -> Vec<u8> {
        let mut data = vec![0b0001_0001, SERVER_RESPONSE << 4 | flags, SERIALIZATION_JSON << 4, 0];
        if flags == FLAG_POSITIVE_SEQUENCE || flags == FLAG_NEGATIVE_SEQUENCE {
            data.extend_from_slice(&7u32.to_be_bytes());
        }
        data.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        data.extend_from_slice(payload.as_bytes());
        data
    }

    #[test]
    fn decodes_sequenced_final_frames_and_rejoins_utterances() {
        let payload = r#"{"result":{"text":"我想 吃 an apple .","utterances":[{"text":"我想吃"},{"text":"an apple"},{"text":"."},{"text":"Then"}]}}"#;
        let response = decode(&response_frame(FLAG_NEGATIVE_SEQUENCE, payload)).unwrap();
        assert_eq!(response, Response { text: "我想吃an apple. Then".into(), is_final: true });

        let partial = decode(&response_frame(FLAG_POSITIVE_SEQUENCE, r#"{"result":{"text":" hi "}}"#)).unwrap();
        assert_eq!(partial, Response { text: "hi".into(), is_final: false });
    }

    #[test]
    fn reports_server_errors() {
        let message = b"quota exceeded";
        let mut data = vec![0b0001_0001, SERVER_ERROR << 4, 0, 0];
        data.extend_from_slice(&45000001u32.to_be_bytes());
        data.extend_from_slice(&(message.len() as u32).to_be_bytes());
        data.extend_from_slice(message);
        assert_eq!(decode(&data), Err("quota exceeded (45000001)".into()));
    }
}
