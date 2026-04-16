use crate::agent::AgentRuntime;
use crate::gateway::protocol::{ChatPayload, GatewayMessage, SessionCreatePayload, SessionGetPayload};
use crate::gateway::session::SessionStore;
use crate::provider::LlmClient;
use log::{error, info, warn};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;

/// 共享应用状态
pub struct AppState<C: LlmClient> {
    pub agent: AgentRuntime<C>,
    pub sessions: SessionStore,
}

/// 消息路由入口
pub async fn handle_message<C: LlmClient>(
    msg: GatewayMessage,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
) {
    let msg_type = msg.msg_type.clone();
    let msg_id = msg.id.clone();

    info!(
        "Received message: type={}, id={}, payload={}",
        msg_type, msg_id, msg.payload
    );

    match msg_type.as_str() {
        "auth" => {
            handle_auth(&msg, &outbound_tx).await;
        }
        "chat" => match serde_json::from_value::<ChatPayload>(msg.payload) {
            Ok(payload) => handle_chat(payload, state, outbound_tx, msg_id).await,
            Err(e) => {
                warn!("Invalid chat payload for message {}: {}", msg_id, e);
                send_error(&outbound_tx, &msg_id, format!("Invalid chat payload: {}", e));
            }
        },
        "sessions.list" => {
            handle_sessions_list(state, outbound_tx, msg_id).await;
        }
        "sessions.get" => match serde_json::from_value::<SessionGetPayload>(msg.payload) {
            Ok(payload) => handle_session_get(payload, state, outbound_tx, msg_id).await,
            Err(e) => {
                warn!("Invalid sessions.get payload for message {}: {}", msg_id, e);
                send_error(&outbound_tx, &msg_id, format!("Invalid sessions.get payload: {}", e));
            }
        },
        "sessions.create" => match serde_json::from_value::<SessionCreatePayload>(msg.payload) {
            Ok(payload) => handle_session_create(payload, state, outbound_tx, msg_id).await,
            Err(e) => {
                warn!("Invalid sessions.create payload for message {}: {}", msg_id, e);
                send_error(&outbound_tx, &msg_id, format!("Invalid sessions.create payload: {}", e));
            }
        },
        "agents.list" => {
            handle_agents_list::<C>(state, outbound_tx, msg_id).await;
        }
        _ => {
            error!("Unknown message type: {}", msg_type);
            send_general_error(
                &outbound_tx,
                &msg_id,
                format!("Unknown message type: {}", msg_type),
                "UNKNOWN_MESSAGE_TYPE",
            );
        }
    }
}

async fn handle_auth(msg: &GatewayMessage, outbound_tx: &mpsc::UnboundedSender<GatewayMessage>) {
    // 简化认证：直接返回 success
    let _ = outbound_tx.send(GatewayMessage {
        msg_type: "auth.success".to_string(),
        id: msg.id.clone(),
        payload: json!({}),
    });
}

async fn handle_chat<C: LlmClient>(
    payload: ChatPayload,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let session_id = match payload.session_id {
        Some(id) => id,
        None => {
            let new_session = state.sessions.create(None).await;
            new_session.id.clone()
        }
    };

    let session = match state.sessions.get(&session_id).await {
        Some(s) => s,
        None => {
            send_general_error(
                &outbound_tx,
                &request_id,
                format!("Session {} not found", session_id),
                "SESSION_NOT_FOUND",
            );
            return;
        }
    };

    // 使用 session 的 chat_lock 确保同一会话内的聊天串行执行
    // 注意：这里使用的是 tokio::sync::Mutex，需要 .lock().await
    let _lock = session.chat_lock.lock().await;

    // 1. 发送 chat.start
    let _ = outbound_tx.send(GatewayMessage {
        msg_type: "chat.start".to_string(),
        id: request_id.clone(),
        payload: json!({ "session_id": session_id }),
    });

    // 2. 创建事件转发通道
    let (event_tx, mut event_rx) = mpsc::channel(100);
    let outbound_tx_clone = outbound_tx.clone();
    let request_id_clone = request_id.clone();
    let session_id_clone = session_id.clone();

    // 3. Spawn 转发任务: event_rx.recv() -> bridge 转换 -> outbound_tx.send()
    let bridge_handle = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let gateway_msg =
                crate::gateway::bridge::agent_event_to_gateway(event, &request_id_clone, &session_id_clone);
            if outbound_tx_clone.send(gateway_msg).is_err() {
                break;
            }
        }
    });

    // 4. 准备历史上下文 (从 session.history clone)
    let history = {
        let h = session.history.read().unwrap();
        h.clone()
    };

    // 5. 调用 agent.run_turn
    match state.agent.run_turn(&history, &payload.message, event_tx).await {
        Ok(new_messages) => {
            // 6. 成功: 追加 user message + new_messages 到 session.history
            let mut h = session.history.write().unwrap();
            let user_msg = crate::message::Message {
                role: crate::message::Role::User,
                content: vec![crate::message::ContentBlock::Text { text: payload.message }],
            };
            h.push(user_msg);
            h.extend(new_messages);
        }
        Err(e) => {
            send_chat_error(
                &outbound_tx,
                &request_id,
                &session_id,
                format!("Agent execution error: {}", e),
                "AGENT_EXECUTION_ERROR",
            );
        }
    }

    // 7. 等待转发任务结束
    let _ = bridge_handle.await;
}

async fn handle_sessions_list<C: LlmClient>(
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let sessions = state.sessions.list().await;
    let _ = outbound_tx.send(GatewayMessage {
        msg_type: "sessions.list".to_string(),
        id: request_id,
        payload: json!({ "sessions": sessions }),
    });
}

async fn handle_session_get<C: LlmClient>(
    payload: SessionGetPayload,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    if let Some(session) = state.sessions.get(&payload.session_id).await {
        let history = session.history.read().unwrap().clone();
        let _ = outbound_tx.send(GatewayMessage {
            msg_type: "sessions.get".to_string(),
            id: request_id,
            payload: json!({
                "id": session.id,
                "name": session.name,
                "messages": history,
                "created_at": session.created_at
            }),
        });
    } else {
        send_error(&outbound_tx, &request_id, "Session not found".to_string());
    }
}

async fn handle_session_create<C: LlmClient>(
    payload: SessionCreatePayload,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let session = state.sessions.create(payload.name).await;
    let _ = outbound_tx.send(GatewayMessage {
        msg_type: "sessions.create".to_string(),
        id: request_id,
        payload: json!({
            "id": session.id,
            "name": session.name,
            "message_count": 0,
            "created_at": session.created_at
        }),
    });
}

async fn handle_agents_list<C: LlmClient>(
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let tool_names: Vec<String> = state
        .agent
        .tools()
        .tool_definitions()
        .iter()
        .map(|d| d.name.clone())
        .collect();

    let _ = outbound_tx.send(GatewayMessage {
        msg_type: "agents.list".to_string(),
        id: request_id,
        payload: json!({
            "agents": [
                {
                    "id": "nova",
                    "name": "Zero-Nova",
                    "description": "A powerful AI agent built on zero-nova",
                    "tools": tool_names
                }
            ]
        }),
    });
}

fn send_chat_error(
    outbound_tx: &mpsc::UnboundedSender<GatewayMessage>,
    request_id: &str,
    session_id: &str,
    error_msg: String,
    code: &str,
) {
    let _ = outbound_tx.send(GatewayMessage {
        msg_type: "chat.error".to_string(),
        id: request_id.to_string(),
        payload: serde_json::to_value(crate::gateway::protocol::ChatErrorPayload {
            session_id: session_id.to_string(),
            error: error_msg,
            code: code.to_string(),
        })
        .unwrap(),
    });
}

fn send_general_error(
    outbound_tx: &mpsc::UnboundedSender<GatewayMessage>,
    request_id: &str,
    error_msg: String,
    code: &str,
) {
    let _ = outbound_tx.send(GatewayMessage {
        msg_type: "error".to_string(),
        id: request_id.to_string(),
        payload: serde_json::to_value(crate::gateway::protocol::ErrorPayload {
            message: error_msg,
            code: code.to_string(),
        })
        .unwrap(),
    });
}

fn send_error(outbound_tx: &mpsc::UnboundedSender<GatewayMessage>, request_id: &str, error_msg: String) {
    send_general_error(outbound_tx, request_id, error_msg, "GENERAL_ERROR");
}
