use crate::{ChannelHandler, ResponseSink};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, trace, warn};
use serde::{de::DeserializeOwned, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{Error as WsError, Message as WsMessage};

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

    // 分离协议响应通道，避免业务回调直接耦合底层 WebSocket sink。
    let (core_tx, mut core_rx) = mpsc::channel::<Resp>(DEFAULT_OUTBOUND_CAPACITY);
    let response_sink = ResponseSink::new(core_tx);

    // 单独转发任务可避免业务发送受写 socket 背压直接影响。
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
                        let max_chars = 500usize;
                        let preview = if json_str.chars().nth(max_chars).is_some() {
                            let truncated: String = json_str.chars().take(max_chars).collect();
                            format!("{}... ({} bytes)", truncated, json_str.len())
                        } else {
                            json_str.to_string()
                        };
                        debug!("[OUTBOUND] Sending response to peer: {}: {}", peer, preview);
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
    handle_receive_loop(&mut ws_source, &handler, &peer_id, &peer, &internal_tx, &response_sink).await;

    write_task.abort();
    let _ = write_task.await;
    info!("Connection closed for {}", peer_id);

    // 在写任务收尾后再通知业务层，避免断开事件先于连接关闭日志出现。
    handler.on_disconnect(peer_id).await;
    Ok(())
}

/// 处理来自 WebSocket 客户端的接收循环。
///
/// 将接收分支与连接收尾逻辑分离，便于在读循环异常退出时保持统一清理路径。
async fn handle_receive_loop<H, Req, Resp>(
    ws_source: &mut (impl StreamExt<Item = Result<WsMessage, WsError>> + Unpin),
    handler: &Arc<H>,
    peer_id: &str,
    peer: &SocketAddr,
    internal_tx: &mpsc::Sender<InternalMessage<Resp>>,
    response_sink: &ResponseSink<Resp>,
) where
    H: ChannelHandler<Req = Req, Resp = Resp>,
    Req: DeserializeOwned + Send + 'static,
    Resp: Serialize + Send + 'static,
{
    while let Some(msg_result) = ws_source.next().await {
        trace!("recv from {}: {:?}", peer, msg_result);
        match msg_result {
            Ok(WsMessage::Text(text)) => {
                handle_text_message(&text, handler, peer_id, response_sink).await;
            }
            Ok(WsMessage::Ping(data)) => match internal_tx.send(InternalMessage::Raw(WsMessage::Pong(data))).await {
                Ok(()) => {}
                Err(_) => {
                    trace!("Failed to queue pong for {}", peer_id);
                    break;
                }
            },
            Ok(WsMessage::Close(_)) => break,
            Err(e) => {
                error!("WS read error from {}: {}", peer_id, e);
                break;
            }
            _ => {}
        }
    }
}

/// 处理 WebSocket Text 消息并进行 JSON 反序列化。
///
/// 提取单独函数后，读循环可集中处理协议分支，文本请求解析失败也能局部化记录。
async fn handle_text_message<H, Req, Resp>(
    text: &str,
    handler: &Arc<H>,
    peer_id: &str,
    response_sink: &ResponseSink<Resp>,
) where
    H: ChannelHandler<Req = Req, Resp = Resp>,
    Req: DeserializeOwned + Send + 'static,
    Resp: Serialize + Send + 'static,
{
    match serde_json::from_str::<Req>(text) {
        Ok(req) => {
            debug!(
                "[INBOUND] Received request from peer: {} (type={})",
                peer_id,
                text.chars().take(200).collect::<String>()
            );
            let handler_clone = handler.clone();
            let peer_id_clone = peer_id.to_string();
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
    }
}
