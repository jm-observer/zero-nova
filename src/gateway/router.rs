use crate::agent::AgentRuntime;
use crate::gateway::protocol::{
    Agent, AuthRequest, ChatPayload, ErrorPayload, GatewayMessage, MessageEnvelope, Session, SessionCreateRequest, SessionIdPayload,
};
use crate::gateway::session::SessionStore;
use crate::provider::LlmClient;
use log::{error, info, warn};
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
    let msg_id = match msg.id {
        Some(id) => id,
        None => {
            warn!("Received command without ID, ignoring: {:?}", msg.envelope);
            return;
        }
    };

    match msg.envelope {
        MessageEnvelope::Auth(AuthRequest { token: _ }) => {
            handle_auth(&msg_id, &outbound_tx).await;
        }
        MessageEnvelope::Chat(payload) => {
            handle_chat(payload, state, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsList => {
            handle_sessions_list(state, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsMessages(payload) => {
            handle_session_get(payload.session_id, state, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsCreate(payload) => {
            handle_session_create(payload, state, outbound_tx, msg_id).await;
        }
        MessageEnvelope::AgentsList => {
            handle_agents_list::<C>(state, outbound_tx, msg_id).await;
        }
        MessageEnvelope::AgentsSwitch(payload) => {
            info!("Switched to agent: {}", payload.agent_id);
            let _ = outbound_tx.send(GatewayMessage::new(
                msg_id,
                MessageEnvelope::AgentsSwitchResponse(crate::gateway::protocol::AgentsSwitchResponse {
                    agent: Agent {
                        id: payload.agent_id,
                        name: "Zero-Nova".to_string(),
                        ..Default::default()
                    },
                    messages: vec![],
                }),
            ));
        }
        MessageEnvelope::BrowserStatus => {
            let _ = outbound_tx.send(GatewayMessage::new(
                msg_id,
                MessageEnvelope::BrowserStatusResponse(serde_json::json!({ "status": "ok" })),
            ));
        }
        MessageEnvelope::ConfigGetLlmSource => {
            let _ = outbound_tx.send(GatewayMessage::new(
                msg_id,
                MessageEnvelope::ConfigGetLlmSourceResponse(serde_json::json!({ "source": "default" })),
            ));
        }
        MessageEnvelope::RouterConfigGet => {
            let _ = outbound_tx.send(GatewayMessage::new(
                msg_id,
                MessageEnvelope::ConfigGetResponse(serde_json::json!({ "connected": false, "config": {} })),
            ));
        }
        MessageEnvelope::VoiceGetStatus => {
            let _ = outbound_tx.send(GatewayMessage::new(
                msg_id,
                MessageEnvelope::VoiceGetStatusResponse(serde_json::json!({ "status": "idle" })),
            ));
        }
        MessageEnvelope::OpenFluxStatus => {
            let _ = outbound_tx.send(GatewayMessage::new(
                msg_id,
                MessageEnvelope::OpenFluxStatusResponse(serde_json::json!({ "loggedIn": false })),
            ));
        }
        MessageEnvelope::LanguageUpdate(payload) => {
            info!("Language updated to: {}", payload.language);
            let _ = outbound_tx.send(GatewayMessage::new(
                msg_id,
                MessageEnvelope::LanguageUpdateResponse(serde_json::json!({ "success": true })),
            ));
        }
        MessageEnvelope::Unknown => {
            error!("Unknown message type received: id={}", msg_id);
            send_general_error(
                &outbound_tx,
                &msg_id,
                "Unknown message type".to_string(),
                Some("UNKNOWN_MESSAGE_TYPE".to_string()),
            );
        }
        _ => {
            warn!(
                "Unhandled or response-only message envelope for id={}: {:?}",
                msg_id, msg.envelope
            );
        }
    }
}

async fn handle_auth(request_id: &str, outbound_tx: &mpsc::UnboundedSender<GatewayMessage>) {
    let _ = outbound_tx.send(GatewayMessage::new(
        request_id.to_string(),
        MessageEnvelope::AuthSuccess,
    ));
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
                Some("SESSION_NOT_FOUND".to_string()),
            );
            return;
        }
    };

    let _lock = session.chat_lock.lock().await;

    // 1. 发送 chat.start
    let _ = outbound_tx.send(GatewayMessage::new(
        request_id.clone(),
        MessageEnvelope::ChatStart(SessionIdPayload {
            session_id: session_id.clone(),
        }),
    ));

    // 2. 创建事件转发通道
    let (event_tx, mut event_rx) = mpsc::channel(100);
    let outbound_tx_clone = outbound_tx.clone();
    let request_id_clone = request_id.clone();
    let session_id_clone = session_id.clone();

    // 3. Spawn 转发任务
    let bridge_handle = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let gateway_msg =
                crate::gateway::bridge::agent_event_to_gateway(event, &request_id_clone, &session_id_clone);
            if outbound_tx_clone.send(gateway_msg).is_err() {
                break;
            }
        }
    });

    // 4. 准备历史上下文
    let history = {
        let h = session.history.read().unwrap();
        h.clone()
    };

    // 5. 调用 agent.run_turn
    match state.agent.run_turn(&history, &payload.input, event_tx).await {
        Ok(new_messages) => {
            let mut h = session.history.write().unwrap();
            let user_msg = crate::message::Message {
                role: crate::message::Role::User,
                content: vec![crate::message::ContentBlock::Text { text: payload.input }],
            };
            h.push(user_msg);
            h.extend(new_messages);
        }
        Err(e) => {
            send_general_error(
                &outbound_tx,
                &request_id,
                format!("Agent execution error: {}", e),
                Some("AGENT_EXECUTION_ERROR".to_string()),
            );
        }
    }

    let _ = bridge_handle.await;
}

async fn handle_sessions_list<C: LlmClient>(
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let internal_sessions = state.sessions.get_all().await;
    let sessions: Vec<Session> = internal_sessions
        .into_iter()
        .map(|s| Session {
            id: s.id.clone(),
            title: Some(s.name.clone()),
            agent_id: "nova".to_string(),
            created_at: s.created_at,
            updated_at: s.created_at,
        })
        .collect();

    let _ = outbound_tx.send(GatewayMessage::new(
        request_id,
        MessageEnvelope::SessionsListResponse(crate::gateway::protocol::SessionsListResponse { sessions }),
    ));
}

async fn handle_session_get<C: LlmClient>(
    session_id: String,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    if let Some(session) = state.sessions.get(&session_id).await {
        let history = session.history.read().unwrap().clone();
        let messages: Vec<serde_json::Value> = history.into_iter().map(|m| serde_json::to_value(m).unwrap()).collect();

        let _ = outbound_tx.send(GatewayMessage::new(
            request_id,
            MessageEnvelope::SessionsMessagesResponse(crate::gateway::protocol::SessionsMessagesResponse { messages }),
        ));
    } else {
        send_general_error(
            &outbound_tx,
            &request_id,
            "Session not found".to_string(),
            Some("SESSION_NOT_FOUND".to_string()),
        );
    }
}

async fn handle_session_create<C: LlmClient>(
    payload: SessionCreateRequest,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let internal_session = state.sessions.create(payload.title).await;
    let session = Session {
        id: internal_session.id.clone(),
        title: Some(internal_session.name.clone()),
        agent_id: payload.agent_id.unwrap_or_else(|| "nova".to_string()),
        created_at: internal_session.created_at,
        updated_at: internal_session.created_at,
    };

    let _ = outbound_tx.send(GatewayMessage::new(
        request_id,
        MessageEnvelope::SessionsCreateResponse(crate::gateway::protocol::SessionCreateResponse { session }),
    ));
}

async fn handle_agents_list<C: LlmClient>(
    _state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let _ = outbound_tx.send(GatewayMessage::new(
        request_id,
        MessageEnvelope::AgentsListResponse(crate::gateway::protocol::AgentsListResponse {
            agents: vec![Agent {
                id: "nova".to_string(),
                name: "Zero-Nova".to_string(),
                description: Some("A powerful AI agent built on zero-nova".to_string()),
                icon: None,
                system_prompt: None,
            }],
        }),
    ));
}

fn send_general_error(
    outbound_tx: &mpsc::UnboundedSender<GatewayMessage>,
    request_id: &str,
    error_msg: String,
    code: Option<String>,
) {
    let _ = outbound_tx.send(GatewayMessage::new(
        request_id.to_string(),
        MessageEnvelope::Error(ErrorPayload {
            message: error_msg,
            code,
        }),
    ));
}
