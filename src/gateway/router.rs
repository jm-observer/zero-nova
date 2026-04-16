use crate::agent::{AgentConfig, AgentRuntime};
use crate::anthropic::AnthropicClient;
use crate::event::AgentEvent;
use crate::gateway::bridge::agent_event_to_gateway;
use crate::gateway::protocol::{
    AgentInfo, ChatCompletePayload, ChatErrorPayload, ChatPayload, GatewayMessage, SessionCreatePayload,
    SessionGetPayload, SessionInfo,
};
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

    info!("Received message: type={}, id={}", msg_type, msg_id);

    match msg_type.as_str() {
        "auth" => {
            handle_auth(&msg, &outbound_tx).await;
        }
        "chat" => {
            if let Ok(payload) = serde_json::from_value::<ChatPayload>(msg.payload) {
                handle_chat(payload, state, outbound_tx, msg_id).await;
            } else {
                warn!("Invalid chat payload for message {}", msg_id);
                send_error(&outbound_tx, &msg_id, "Invalid chat payload".to_string());
            }
        }
        "sessions.list" => {
            handle_sessions_list(state, outbound_tx, msg_id).await;
        }
        "sessions.get" => {
            if let Ok(payload) = serde_json::from_value::<SessionGetPayload>(msg.payload) {
                handle_session_get(payload, state, outbound_tx, msg_id).await;
            } else {
                warn!("Invalid sessions.get payload for message {}", msg_id);
                send_error(&outbound_tx, &msg_id, "Invalid sessions.get payload".to_string());
            }
        }
        "sessions.create" => {
            if let Ok(payload) = serde_json::from_value::<SessionCreatePayload>(msg.payload) {
                handle_session_create(payload, state, outbound_tx, msg_id).await;
            } else {
                warn!("Invalid sessions.create payload for message {}", msg_id);
                send_error(&outbound_tx, &msg_id, "Invalid sessions.create payload".to_string());
            }
        }
        "agents.list" => {
            handle_agents_list(outbound_tx, msg_id).await;
        }
        _ => {
            error!("Unknown message type: {}", msg_type);
            send_error(&outbound_tx, &msg_id, format!("Unknown message type: {}", msg_type));
        }
    }
}

async fn handle_auth(_msg: &GatewayMessage, outbound_tx: &mpsc::UnboundedSender<GatewayMessage>) {
    // 简化认证：直接返回 success
    let _ = outbound_tx.send(GatewayMessage {
        msg_type: "auth.success".to_string(),
        id: "system".to_string(), // 实际应从 msg 中提取或关联
        payload: json!({}),
    });
}

async fn handle_chat<C: LlmClient>(
    payload: ChatPayload,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let session = match state.sessions.get(&payload.session_id).await {
        Some(s) => s,
        None => {
            send_error(
                &outbound_tx,
                &request_id,
                format!("Session {} not found", payload.session_id),
            );
            return;
        }
    };

    // 使用 session 的 chat_lock 确保同一会话内的聊天串行执行
    let _lock = session.chat_lock.lock().unwrap();

    // 1. 发送 chat.start
    let _ = outbound_tx.send(GatewayMessage {
        msg_type: "chat.start".to_string(),
        id: request_id.clone(),
        payload: json!({ "session_id": payload.session_id }),
    });

    // 2. 创建事件转发通道
    let (event_tx, mut event_rx) = mpsc::channel(100);
    let outbound_tx_clone = outbound_tx.clone();
    let request_id_clone = request_id.clone();
    let session_id_clone = payload.session_id.clone();

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
            // 注意：run_turn 内部其实已经在 history 副本中追加了 user message，
            // 但我们这里需要把最终的完整消息序列存入持久化 history。
            // 根据 AgentRuntime 实现，new_messages 是当前 turn 产生的所有消息。
            // 我们需要补齐 user message。
            let user_msg = crate::message::Message {
                role: crate::message::Role::User,
                content: vec![crate::message::ContentBlock::Text { text: payload.message }],
            };
            h.push(user_msg);
            h.extend(new_messages);
        }
        Err(e) => {
            send_error(&outbound_tx, &request_id, format!("Agent execution error: {}", e));
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

async fn handle_agents_list<C: LlmClient>(outbound_tx: mpsc::UnboundedSender<GatewayMessage>, request_id: String) {
    // 简化实现：硬编码返回一个 agent
    let _ = outbound_tx.send(GatewayMessage {
        msg_type: "agents.list".to_string(),
        id: request_id,
        payload: json!({
            "agents": [
                {
                    "id": "nova",
                    "name": "Zero-Nova",
                    "description": "A powerful AI agent built on zero-nova",
                    "tools": ["bash", "read_file", "write_file", "list_dir"]
                }
            ]
        }),
    });
}

fn send_error(outbound_tx: &mpsc::UnboundedSender<GatewayMessage>, request_id: &str, error_msg: String) {
    let _ = outbound_tx.send(GatewayMessage {
        msg_type: "chat.error".to_string(),
        id: request_id.to_string(),
        payload: json!({ "error": error_msg }),
    });
}
