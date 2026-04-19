use crate::gateway::handlers::system::send_general_error;
use crate::gateway::protocol::GatewayMessage;
use crate::gateway::protocol::{
    MessageEnvelope, Session, SessionCreateRequest, SessionCreateResponse, SessionIdPayload, SessionsListResponse,
    SessionsMessagesResponse, SuccessResponse,
};
use crate::gateway::router::AppState;
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn handle_sessions_list<C: crate::provider::LlmClient>(
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let sessions = state.sessions.list_sorted().await;

    let _ = outbound_tx.send(GatewayMessage::new(
        request_id,
        MessageEnvelope::SessionsListResponse(SessionsListResponse { sessions }),
    ));
}

pub async fn handle_session_get<C: crate::provider::LlmClient>(
    session_id: String,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    if let Some(session) = state.sessions.get(&session_id).await {
        let messages = session.get_messages_dto();

        let _ = outbound_tx.send(GatewayMessage::new(
            request_id,
            MessageEnvelope::SessionsMessagesResponse(SessionsMessagesResponse { messages }),
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
    let internal_session = state.sessions.create(payload.title.clone(), payload.agent_id.clone()).await;
    let session = Session {
        id: internal_session.id.clone(),
        title: Some(internal_session.name.clone()),
        agent_id: payload.agent_id.unwrap_or_else(|| "default".to_string()),
        created_at: internal_session.created_at,
        updated_at: internal_session.created_at,
        message_count: 0,
    };

    let _ = outbound_tx.send(GatewayMessage::new(
        request_id,
        MessageEnvelope::SessionsCreateResponse(SessionCreateResponse { session }),
    ));
}

pub async fn handle_session_delete<C: crate::provider::LlmClient>(
    payload: SessionIdPayload,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let success = state.sessions.delete(&payload.session_id).await;

    if success {
        let _ = outbound_tx.send(GatewayMessage::new(
            request_id,
            MessageEnvelope::SessionsDeleteResponse(SuccessResponse { success: true }),
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
