use crate::app::application::GatewayApplication;
use crate::gateway::protocol::{
    GatewayMessage, SessionCreateRequest, SessionCreateResponse, SessionsListResponse, SessionsMessagesResponse,
    SuccessResponse,
};
use channel_websocket::ResponseSink;
use log::error;

pub async fn handle_sessions_list<C: crate::provider::LlmClient + 'static>(
    app: &GatewayApplication<C>,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    let sessions = app.list_sessions().await;
    let _ = outbound_tx.send(GatewayMessage::new(
        request_id,
        crate::gateway::protocol::MessageEnvelope::SessionsListResponse(SessionsListResponse { sessions }),
    ));
}

pub async fn handle_session_get<C: crate::provider::LlmClient + 'static>(
    session_id: String,
    app: &GatewayApplication<C>,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.session_messages(&session_id).await {
        Ok(messages) => {
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                crate::gateway::protocol::MessageEnvelope::SessionsMessagesResponse(SessionsMessagesResponse {
                    messages,
                }),
            ));
        }
        Err(e) if e.to_string().contains("Session not found") => {
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

pub async fn handle_session_create<C: crate::provider::LlmClient + 'static>(
    payload: SessionCreateRequest,
    app: &GatewayApplication<C>,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.create_session(payload.title, payload.agent_id).await {
        Ok(session) => {
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                crate::gateway::protocol::MessageEnvelope::SessionsCreateResponse(SessionCreateResponse { session }),
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

pub async fn handle_session_delete<C: crate::provider::LlmClient + 'static>(
    payload: crate::gateway::protocol::SessionIdPayload,
    app: &GatewayApplication<C>,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.delete_session(&payload.session_id).await {
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

pub async fn handle_session_copy<C: crate::provider::LlmClient + 'static>(
    payload: crate::gateway::protocol::SessionCopyRequest,
    app: &GatewayApplication<C>,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.copy_session(&payload.session_id, payload.index).await {
        Ok(session) => {
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                crate::gateway::protocol::MessageEnvelope::SessionsCopyResponse(SessionCreateResponse { session }),
            ));
        }
        Err(e) if e.to_string().contains("Source session not found") => {
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
