use crate::gateway::protocol::{GatewayMessage, MessageEnvelope, WelcomePayload};
use crate::gateway::router::{handle_message, AppState};
use futures_util::{SinkExt, StreamExt};
use log::{info, trace};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;

pub async fn run_server<C: crate::provider::LlmClient + 'static>(
    addr: SocketAddr,
    state: Arc<AppState<C>>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    log::info!("WebSocket Gateway listening on: {}", addr);

    while let Ok((stream, peer)) = listener.accept().await {
        let state_clone = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, peer, state_clone).await {
                log::error!("Error handling connection from {}: {}", peer, e);
            }
        });
    }
    Ok(())
}

async fn handle_connection<C: crate::provider::LlmClient + 'static>(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    state: Arc<AppState<C>>,
) -> anyhow::Result<()> {
    let ws_stream = tokio_tungstenite::accept_async(stream).await?;
    info!("New WebSocket connection: {}", peer);

    let (mut ws_sink, mut ws_source) = ws_stream.split();
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<GatewayMessage>();

    // 发送 welcome 消息（OpenFlux 前端握手协议要求）
    let _ = outbound_tx.send(GatewayMessage::new_event(MessageEnvelope::Welcome(WelcomePayload {
        require_auth: false,
        setup_required: false,
    })));

    // Write Task: 发送消息到客户端
    let write_task = tokio::spawn(async move {
        while let Some(msg) = outbound_rx.recv().await {
            if let Ok(json_str) = serde_json::to_string(&msg) {
                // info!("send: {}", json_str);
                if ws_sink.send(WsMessage::Text(json_str)).await.is_err() {
                    break;
                }
            }
        }
    });

    // Read Loop: 接收客户端消息
    while let Some(msg_result) = ws_source.next().await {
        trace!("recv: {:?}", msg_result);
        match msg_result {
            Ok(WsMessage::Text(text)) => {
                match serde_json::from_str::<GatewayMessage>(&text) {
                    Ok(gateway_msg) => {
                        let state_clone = state.clone();
                        let tx_clone = outbound_tx.clone();
                        // 每个 inbound message 的 handler 都 spawn 独立 task，不阻塞读循环
                        tokio::spawn(async move {
                            handle_message(gateway_msg, state_clone, tx_clone).await;
                        });
                    }
                    Err(e) => {
                        log::warn!("Failed to parse GatewayMessage from {}: {}. Text: {}", peer, e, text);
                    }
                }
            }
            Ok(WsMessage::Close(_)) => break,
            Err(e) => {
                log::error!("WS read error from {}: {}", peer, e);
                break;
            }
            _ => {} // 忽略 binary 等其他类型
        }
    }

    // 清理
    drop(outbound_tx); // 关闭通道，触发 write_task 结束
    let _ = write_task.await;
    log::info!("Connection closed: {}", peer);
    Ok(())
}
