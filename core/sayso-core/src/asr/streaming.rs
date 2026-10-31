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
    /// 某些协议须等任务确认后才能发送音频，其余协议配置后即可发送。
    fn is_ready(&self) -> bool {
        true
    }
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
    fn finish_on_close(&self) -> Result<String, CoreError> {
        Ok(self.current_text())
    }
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
    let ready_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);

    let mut audio_open = true;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(ready_deadline), if !wire.is_ready() => return Err(server_error("recognition task did not start within 10 seconds")),
            chunk = audio.recv(), if audio_open && wire.is_ready() => {
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
                Some(Ok(Message::Close(_))) if !audio_open => return wire.finish_on_close(),
                Some(Ok(message)) => match wire.receive(message)? {
                    Update::None => {}
                    Update::Partial(text) => partial(text),
                    Update::Finished(text) => return Ok(text),
                },
                // 已请求结束后服务端直接断开，视为正常结束。
                Some(Err(_)) | None if !audio_open => return wire.finish_on_close(),
                Some(Err(error)) => return Err(server_error(error)),
                None => return Err(server_error("connection closed")),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    struct GatedWire {
        url: String,
        ready: bool,
    }
    impl Wire for GatedWire {
        fn is_ready(&self) -> bool {
            self.ready
        }
        fn request(&self) -> Result<Request, CoreError> {
            request(&self.url)
        }
        fn opening_message(&self) -> Result<Message, CoreError> {
            Ok(Message::text("start"))
        }
        fn audio_message(&self, pcm: Vec<u8>) -> Message {
            Message::binary(pcm)
        }
        fn end_of_audio_messages(&self) -> Vec<Message> {
            vec![Message::text("finish")]
        }
        fn receive(&mut self, message: Message) -> Result<Update, CoreError> {
            if message == Message::text("ready") {
                self.ready = true;
                Ok(Update::None)
            } else {
                Ok(Update::Finished("final transcript".into()))
            }
        }
        fn current_text(&self) -> String {
            String::new()
        }
    }

    /// 即使用户在任务确认前已经结束录音，也不能提前送音频或丢掉队列里的尾部数据。
    #[tokio::test]
    async fn queues_audio_and_finish_until_the_server_confirms_readiness() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (connection, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(connection).await.unwrap();
            assert_eq!(socket.next().await.unwrap().unwrap(), Message::text("start"));
            assert!(tokio::time::timeout(Duration::from_millis(80), socket.next()).await.is_err());
            socket.send(Message::text("ready")).await.unwrap();
            assert_eq!(socket.next().await.unwrap().unwrap(), Message::binary(vec![1, 2, 3, 4]));
            assert_eq!(socket.next().await.unwrap().unwrap(), Message::text("finish"));
            socket.send(Message::text("done")).await.unwrap();
        });
        let (sender, receiver) = mpsc::unbounded_channel();
        sender.send(vec![1, 2, 3, 4]).unwrap();
        drop(sender);
        assert_eq!(
            run(GatedWire { url, ready: false }, receiver, std::sync::Arc::new(|_| {})).await.unwrap(),
            "final transcript"
        );
        server.await.unwrap();
    }
}
