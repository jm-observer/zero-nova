use crate::gateway::handlers::system::send_general_error;
use crate::gateway::protocol::GatewayMessage;
use crate::gateway::protocol::{Session, SessionCreateRequest, SessionCreateResponse, SessionIdPayload};
use crate::gateway::router::AppState;
use log::{error, info, warn};
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn handle_sessions_list<C: crate::provider::LlmClient>(
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
        crate::gateway::protocol::MessageEnvelope::SessionsListResponse(
            crate::gateway::protocol::SessionsListResponse { sessions },
        ),
    ));
}

pub async fn handle_session_get<C: crate::provider::LlmClient>(
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
            crate::gateway::protocol::MessageEnvelope::SessionsMessagesResponse(
                crate::gateway::protocol::SessionsMessagesResponse { messages },
            ),
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

pub async fn handle_session_create<C: crate::provider::LlmClient>(
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
        crate::gateway::protocol::MessageEnvelope::SessionsCreateResponse(
            crate::gateway::protocol::SessionCreateResponse { session },
        ),
    ));
}
