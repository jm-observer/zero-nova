pub use channel_core::{ChannelHandler, PeerId, ResponseSink};
use futures_util::{SinkExt, StreamExt};
use log::{error, info, trace, warn};
use serde::{de::DeserializeOwned, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;

const DEFAULT_OUTBOUND_CAPACITY: usize = 128;

enum InternalMessage<R> {
    Protocol(R),
    Raw(WsMessage),
}

/// 启动 WebSocket 服务器，适配 channel-core 接口
pub async fn run_server<H, Req, Resp>(addr: &str, handler: Arc<H>) -> anyhow::Result<()>
where
    H: ChannelHandler<Req = Req, Resp = Resp>,
    Req: DeserializeOwned + Send + 'static,
    Resp: Serialize + Send + 'static,
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

async fn handle_connection<H, Req, Resp>(stream: TcpStream, peer: SocketAddr, handler: Arc<H>) -> anyhow::Result<()>
where
    H: ChannelHandler<Req = Req, Resp = Resp>,
    Req: DeserializeOwned + Send + 'static,
    Resp: Serialize + Send + 'static,
{
    let peer_id = peer.to_string();
    let ws_stream = tokio_tungstenite::accept_async(stream).await?;
    info!("New WebSocket connection: {}", peer_id);

    let (mut ws_sink, mut ws_source) = ws_stream.split();
    let (internal_tx, mut internal_rx) = mpsc::channel::<InternalMessage<Resp>>(DEFAULT_OUTBOUND_CAPACITY);

    // Create the channel-core ResponseSink
    let (core_tx, mut core_rx) = mpsc::channel::<Resp>(DEFAULT_OUTBOUND_CAPACITY);
    let response_sink = ResponseSink::new(core_tx);

    // Forwarding task: core_rx -> internal_tx
    let internal_tx_clone = internal_tx.clone();
    tokio::spawn(async move {
        while let Some(msg) = core_rx.recv().await {
            if internal_tx_clone.send(InternalMessage::Protocol(msg)).await.is_err() {
                break;
            }
        }
    });

    // 调用业务层的连接建立回调
    match handler.on_connect(peer_id.clone()).await {
        Ok(initial_msgs) => {
            for msg in initial_msgs {
                if internal_tx.send(InternalMessage::Protocol(msg)).await.is_err() {
                    warn!("Failed to queue initial message for {}: SendError", peer_id);
                    return Ok(());
                }
            }
        }
        Err(e) => {
            error!("on_connect failed for {}: {}", peer_id, e);
            return Err(e);
        }
    }

    // 发送循环任务
    let write_task = tokio::spawn(async move {
        while let Some(msg) = internal_rx.recv().await {
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
            Ok(WsMessage::Text(text)) => match serde_json::from_str::<Req>(&text) {
                Ok(req) => {
                    let handler_clone = handler.clone();
                    let peer_id_clone = peer_id.clone();
                    let sink_clone = response_sink.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handler_clone.on_message(peer_id_clone, req, sink_clone).await {
                            error!("Error in on_message: {}", e);
                        }
                    });
                }
                Err(e) => {
                    warn!("Failed to parse request from {}: {}. Text: {}", peer_id, e, text);
                }
            },
            Ok(WsMessage::Ping(data)) => {
                if internal_tx
                    .send(InternalMessage::Raw(WsMessage::Pong(data)))
                    .await
                    .is_err()
                {
                    trace!("Failed to queue pong for {}", peer_id);
                    break;
                }
            }
            Ok(WsMessage::Close(_)) => break,
            Err(e) => {
                error!("WS read error from {}: {}", peer_id, e);
                break;
            }
            _ => {}
        }
    }

    // 通知业务层连接断开
    handler.on_disconnect(peer_id.clone()).await;

    write_task.abort();
    let _ = write_task.await;
    info!("Connection closed for {}", peer_id);
    Ok(())
}
