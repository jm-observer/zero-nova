use crate::gateway::protocol::{
    GatewayMessage, SessionCreateRequest, SessionCreateResponse, SessionsListResponse, SessionsMessagesResponse,
    SuccessResponse,
};
use crate::gateway::router::AppState;
use log::error;
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn handle_sessions_list<C: crate::provider::LlmClient>(
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let sessions = state.conversation_service.sessions.list_sorted().await;
    let _ = outbound_tx.send(GatewayMessage::new(
        request_id,
        crate::gateway::protocol::MessageEnvelope::SessionsListResponse(SessionsListResponse { sessions }),
    ));
}

pub async fn handle_session_get<C: crate::provider::LlmClient>(
    session_id: String,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    match state.conversation_service.sessions.get(&session_id).await {
        Ok(Some(session)) => {
            let messages = session.get_messages_dto();
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                crate::gateway::protocol::MessageEnvelope::SessionsMessagesResponse(SessionsMessagesResponse {
                    messages,
                }),
            ));
        }
        Ok(None) => {
            crate::gateway::handlers::system::send_general_error(
                &outbound_tx,
                &request_id,
                "Session not found".to_string(),
                None::<String>,
            );
        }
        Err(e) => {
            error!("Failed to get session {}: {}", session_id, e);
            crate::gateway::handlers::system::send_general_error(
                &outbound_tx,
                &request_id,
                e.to_string(),
                None::<String>,
            );
        }
    }
}

pub async fn handle_session_create<C: crate::provider::LlmClient>(
    payload: SessionCreateRequest,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let system_prompt = state
        .conversation_service
        .agent_registry
        .get(&payload.agent_id)
        .map(|a| a.system_prompt_template.clone())
        .unwrap_or_default();

    match state
        .conversation_service
        .sessions
        .create(payload.title, payload.agent_id, system_prompt)
        .await
    {
        Ok(session) => {
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                crate::gateway::protocol::MessageEnvelope::SessionsCreateResponse(SessionCreateResponse {
                    session: crate::gateway::protocol::Session {
                        id: session.id.clone(),
                        title: Some(session.name.clone()),
                        agent_id: session.control.read().unwrap().active_agent.clone(),
                        created_at: session.created_at,
                        updated_at: session.updated_at.load(std::sync::atomic::Ordering::SeqCst),
                        message_count: session.history.read().unwrap().len(),
                    },
                }),
            ));
        }
        Err(e) => {
            error!("Failed to create session: {}", e);
            crate::gateway::handlers::system::send_general_error(
                &outbound_tx,
                &request_id,
                e.to_string(),
                None::<String>,
            );
        }
    }
}

pub async fn handle_session_delete<C: crate::provider::LlmClient>(
    payload: crate::gateway::protocol::SessionIdPayload,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    match state.conversation_service.sessions.delete(&payload.session_id).await {
        Ok(success) => {
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                crate::gateway::protocol::MessageEnvelope::SessionsDeleteResponse(SuccessResponse { success }),
            ));
        }
        Err(e) => {
            error!("Failed to delete session {}: {}", payload.session_id, e);
            crate::gateway::handlers::system::send_general_error(
                &outbound_tx,
                &request_id,
                e.to_string(),
                None::<String>,
            );
        }
    }
}

pub async fn handle_session_copy<C: crate::provider::LlmClient>(
    payload: crate::gateway::protocol::SessionCopyRequest,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    match state
        .conversation_service
        .sessions
        .copy_session(&payload.session_id, payload.index)
        .await
    {
        Ok(Some(session)) => {
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                crate::gateway::protocol::MessageEnvelope::SessionsCopyResponse(SessionCreateResponse {
                    session: crate::gateway::protocol::Session {
                        id: session.id.clone(),
                        title: Some(session.name.clone()),
                        agent_id: session.control.read().unwrap().active_agent.clone(),
                        created_at: session.created_at,
                        updated_at: session.updated_at.load(std::sync::atomic::Ordering::SeqCst),
                        message_count: session.history.read().unwrap().len(),
                    },
                }),
            ));
        }
        Ok(None) => {
            crate::gateway::handlers::system::send_general_error(
                &outbound_tx,
                &request_id,
                "Source session not found".to_string(),
                None::<String>,
            );
        }
        Err(e) => {
            error!("Failed to copy session {}: {}", payload.session_id, e);
            crate::gateway::handlers::system::send_general_error(
                &outbound_tx,
                &request_id,
                e.to_string(),
                None::<String>,
            );
        }
    }
}
