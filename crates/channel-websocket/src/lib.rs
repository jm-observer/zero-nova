use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use log::{error, info, trace, warn};
use serde::{de::DeserializeOwned, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;

const DEFAULT_OUTBOUND_CAPACITY: usize = 128;

/// 业务处理接口，用于解耦底层 WebSocket 与上层业务逻辑
#[async_trait]
pub trait ChannelHandler: Send + Sync + 'static {
    type Req: DeserializeOwned + Send + 'static;
    type Resp: Serialize + Send + 'static;

    /// 连接建立时的回调，可返回初始推送的消息（如 Welcome 消息）
    async fn on_connect(&self, peer: SocketAddr) -> anyhow::Result<Vec<Self::Resp>>;

    /// 收到消息时的回调
    async fn on_message(
        &self,
        peer: SocketAddr,
        req: Self::Req,
        response_sink: ResponseSink<Self::Resp>,
    ) -> anyhow::Result<()>;

    /// 连接断开时的回调
    async fn on_disconnect(&self, peer: SocketAddr);
}

enum InternalMessage<R> {
    Protocol(R),
    Raw(WsMessage),
}

pub struct ResponseSink<R> {
    tx: mpsc::Sender<InternalMessage<R>>,
}

impl<R> ResponseSink<R> {
    fn new(tx: mpsc::Sender<InternalMessage<R>>) -> Self {
        Self { tx }
    }

    pub fn send(&self, response: R) -> Result<(), ResponseSinkError> {
        self.tx
            .try_send(InternalMessage::Protocol(response))
            .map_err(ResponseSinkError::from)
    }

    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }

    fn send_raw(&self, message: WsMessage) -> Result<(), ResponseSinkError> {
        self.tx
            .try_send(InternalMessage::Raw(message))
            .map_err(ResponseSinkError::from)
    }
}

impl<R> Clone for ResponseSink<R> {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseSinkError {
    Closed,
    Full,
}

impl std::fmt::Display for ResponseSinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "response sink is closed"),
            Self::Full => write!(f, "response sink is full"),
        }
    }
}

impl std::error::Error for ResponseSinkError {}

impl<R> From<mpsc::error::TrySendError<InternalMessage<R>>> for ResponseSinkError {
    fn from(error: mpsc::error::TrySendError<InternalMessage<R>>) -> Self {
        match error {
            mpsc::error::TrySendError::Full(_) => Self::Full,
            mpsc::error::TrySendError::Closed(_) => Self::Closed,
        }
    }
}

/// 启动 WebSocket 服务器
pub async fn run_server<H>(addr: SocketAddr, handler: Arc<H>) -> anyhow::Result<()>
where
    H: ChannelHandler,
{
    let listener = TcpListener::bind(addr).await?;
    info!("WebSocket server listening on: {}", addr);

    while let Ok((stream, peer)) = listener.accept().await {
        let handler_clone = handler.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, peer, handler_clone).await {
                error!("Error handling connection from {}: {}", peer, e);
            }
        });
    }
    Ok(())
}

async fn handle_connection<H>(stream: TcpStream, peer: SocketAddr, handler: Arc<H>) -> anyhow::Result<()>
where
    H: ChannelHandler,
{
    let ws_stream = tokio_tungstenite::accept_async(stream).await?;
    info!("New WebSocket connection: {}", peer);

    let (mut ws_sink, mut ws_source) = ws_stream.split();
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<InternalMessage<H::Resp>>(DEFAULT_OUTBOUND_CAPACITY);
    let response_sink = ResponseSink::new(outbound_tx.clone());

    // 调用业务层的连接建立回调
    match handler.on_connect(peer).await {
        Ok(initial_msgs) => {
            for msg in initial_msgs {
                if let Err(err) = response_sink.send(msg) {
                    warn!("Failed to queue initial message for {}: {}", peer, err);
                    return Ok(());
                }
            }
        }
        Err(e) => {
            error!("on_connect failed for {}: {}", peer, e);
            return Err(e);
        }
    }

    // 发送循环任务
    let write_task = tokio::spawn(async move {
        while let Some(msg) = outbound_rx.recv().await {
            match msg {
                InternalMessage::Protocol(p) => match serde_json::to_string(&p) {
                    Ok(json_str) => {
                        if ws_sink.send(WsMessage::Text(json_str)).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Failed to serialize response: {}", e);
                    }
                },
                InternalMessage::Raw(raw) => {
                    if ws_sink.send(raw).await.is_err() {
                        break;
                    }
                }
            }
        }
        info!("Write task for {} finished", peer);
    });

    // 接收消息循环
    while let Some(msg_result) = ws_source.next().await {
        trace!("recv from {}: {:?}", peer, msg_result);
        match msg_result {
            Ok(WsMessage::Text(text)) => match serde_json::from_str::<H::Req>(&text) {
                Ok(req) => {
                    let handler_clone = handler.clone();
                    let sink_clone = response_sink.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handler_clone.on_message(peer, req, sink_clone).await {
                            error!("Error in on_message for {}: {}", peer, e);
                        }
                    });
                }
                Err(e) => {
                    warn!("Failed to parse request from {}: {}. Text: {}", peer, e, text);
                }
            },
            Ok(WsMessage::Ping(data)) => {
                if let Err(err) = response_sink.send_raw(WsMessage::Pong(data)) {
                    trace!("Failed to queue pong for {}: {}", peer, err);
                    break;
                }
            }
            Ok(WsMessage::Close(_)) => break,
            Err(e) => {
                error!("WS read error from {}: {}", peer, e);
                break;
            }
            _ => {}
        }
    }

    // 通知业务层连接断开
    handler.on_disconnect(peer).await;

    drop(response_sink);
    drop(outbound_tx);
    write_task.abort();
    let _ = write_task.await;
    info!("Connection closed for {}", peer);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::oneshot;
    use tokio_tungstenite::{connect_async, tungstenite::Message};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct TestMessage {
        value: String,
    }

    struct TestHandler {
        disconnects: Arc<AtomicUsize>,
        disconnect_notifier: std::sync::Mutex<Option<oneshot::Sender<()>>>,
    }

    #[async_trait]
    impl ChannelHandler for TestHandler {
        type Req = TestMessage;
        type Resp = TestMessage;

        async fn on_connect(&self, _peer: SocketAddr) -> anyhow::Result<Vec<Self::Resp>> {
            Ok(vec![TestMessage {
                value: "welcome".to_string(),
            }])
        }

        async fn on_message(
            &self,
            _peer: SocketAddr,
            req: Self::Req,
            response_sink: ResponseSink<Self::Resp>,
        ) -> anyhow::Result<()> {
            let _ = response_sink.send(req);
            Ok(())
        }

        async fn on_disconnect(&self, _peer: SocketAddr) {
            self.disconnects.fetch_add(1, Ordering::SeqCst);
            if let Some(tx) = self.disconnect_notifier.lock().unwrap().take() {
                let _ = tx.send(());
            }
        }
    }

    async fn spawn_test_server() -> (SocketAddr, oneshot::Receiver<()>, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let disconnects = Arc::new(AtomicUsize::new(0));
        let (disconnect_tx, disconnect_rx) = oneshot::channel();
        let handler = Arc::new(TestHandler {
            disconnects: disconnects.clone(),
            disconnect_notifier: std::sync::Mutex::new(Some(disconnect_tx)),
        });

        tokio::spawn(async move {
            let _ = run_server(addr, handler).await;
        });

        (addr, disconnect_rx, disconnects)
    }

    #[tokio::test]
    async fn sends_welcome_and_echoes_messages() {
        let (addr, _, _) = spawn_test_server().await;
        let (mut socket, _) = connect_async(format!("ws://{}", addr)).await.unwrap();

        let welcome = socket.next().await.unwrap().unwrap();
        assert_eq!(
            welcome.into_text().unwrap(),
            serde_json::to_string(&TestMessage {
                value: "welcome".to_string()
            })
            .unwrap()
        );

        socket
            .send(Message::Text(
                serde_json::to_string(&TestMessage {
                    value: "echo".to_string(),
                })
                .unwrap(),
            ))
            .await
            .unwrap();

        let echoed = socket.next().await.unwrap().unwrap();
        assert_eq!(
            echoed.into_text().unwrap(),
            serde_json::to_string(&TestMessage {
                value: "echo".to_string()
            })
            .unwrap()
        );
    }

    #[tokio::test]
    async fn invalid_json_does_not_break_connection() {
        let (addr, _, _) = spawn_test_server().await;
        let (mut socket, _) = connect_async(format!("ws://{}", addr)).await.unwrap();

        let _ = socket.next().await.unwrap().unwrap();

        socket.send(Message::Text("{invalid".to_string())).await.unwrap();
        socket
            .send(Message::Text(
                serde_json::to_string(&TestMessage {
                    value: "after-invalid".to_string(),
                })
                .unwrap(),
            ))
            .await
            .unwrap();

        let echoed = socket.next().await.unwrap().unwrap();
        assert_eq!(
            echoed.into_text().unwrap(),
            serde_json::to_string(&TestMessage {
                value: "after-invalid".to_string()
            })
            .unwrap()
        );
    }

    #[tokio::test]
    async fn response_sink_is_bounded_and_disconnects_are_reported() {
        let (tx, mut rx) = mpsc::channel::<InternalMessage<TestMessage>>(1);
        let sink = ResponseSink::new(tx);

        sink.send(TestMessage {
            value: "first".to_string(),
        })
        .unwrap();

        let second = sink.send(TestMessage {
            value: "second".to_string(),
        });
        assert_eq!(second.unwrap_err(), ResponseSinkError::Full);

        let _ = rx.recv().await;
        drop(rx);
        assert_eq!(
            sink.send(TestMessage {
                value: "closed".to_string(),
            })
            .unwrap_err(),
            ResponseSinkError::Closed
        );

        let (addr, disconnect_rx, disconnects) = spawn_test_server().await;
        let (socket, _) = connect_async(format!("ws://{}", addr)).await.unwrap();
        drop(socket);

        disconnect_rx.await.unwrap();
        assert_eq!(disconnects.load(Ordering::SeqCst), 1);
    }
}
