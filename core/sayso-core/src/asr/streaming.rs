//! 基于 WebSocket 的流式识别会话。各服务的线上协议由 [`Wire`] 适配。

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::client::Request;
use tokio_tungstenite::tungstenite::Message;

use super::{server_error, PartialSink};
use crate::error::CoreError;

/// 流式识别服务的线上协议：握手请求、帧编码与服务端消息解析。
pub(super) trait Wire: Send {
    fn request(&self) -> Result<Request, CoreError>;
    /// 建立连接后、发送音频前的消息（如参数配置帧）。
    fn opening_message(&self) -> Result<Message, CoreError>;
    fn audio_message(&self, pcm: Vec<u8>) -> Message;
    /// 音频结束时依次发送的消息。
    fn end_of_audio_messages(&self) -> Vec<Message>;
    /// 解析一条服务端消息，可在内部累积文本。
    fn receive(&mut self, message: Message) -> Result<Update, CoreError>;
    /// 到目前为止的识别文本。服务端在音频结束后直接断开时以此为最终结果。
    fn current_text(&self) -> String;
}

pub(super) enum Update {
    None,
    Partial(String),
    Finished(String),
}

pub(super) fn request(url: &str) -> Result<Request, CoreError> {
    url.into_client_request().map_err(server_error)
}

/// 连接服务，按序发送音频；音频通道关闭后发送结束帧，等待最终结果。
pub(super) async fn run(
    mut wire: impl Wire,
    mut audio: mpsc::UnboundedReceiver<Vec<u8>>,
    partial: PartialSink,
) -> Result<String, CoreError> {
    // 关闭 Nagle：每 100ms 一帧音频的尾段不能被攒包延迟，否则实时字幕明显滞后。
    let (socket, _) =
        tokio_tungstenite::connect_async_with_config(wire.request()?, None, true).await.map_err(server_error)?;
    let (mut sink, mut stream) = socket.split();
    sink.send(wire.opening_message()?).await.map_err(server_error)?;

    let mut audio_open = true;
    loop {
        tokio::select! {
            chunk = audio.recv(), if audio_open => {
                let messages = match chunk {
                    Some(pcm) => vec![wire.audio_message(pcm)],
                    None => {
                        audio_open = false;
                        wire.end_of_audio_messages()
                    }
                };
                for message in messages {
                    sink.send(message).await.map_err(server_error)?;
                }
            }
            message = stream.next() => match message {
                Some(Ok(message)) => match wire.receive(message)? {
                    Update::None => {}
                    Update::Partial(text) => partial(text),
                    Update::Finished(text) => return Ok(text),
                },
                // 已请求结束后服务端直接断开，视为正常结束。
                Some(Err(_)) | None if !audio_open => return Ok(wire.current_text()),
                Some(Err(error)) => return Err(server_error(error)),
                None => return Err(server_error("connection closed")),
            },
        }
    }
}
