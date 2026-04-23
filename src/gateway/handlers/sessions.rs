use crate::app::application::GatewayApplication;
use crate::gateway::bridge::{app_message_to_protocol, app_session_to_protocol};
use crate::gateway::protocol::{
    GatewayMessage, MessageEnvelope, SessionCreateRequest, SessionCreateResponse, SessionsListResponse,
    SessionsMessagesResponse, SuccessResponse,
};
use channel_websocket::ResponseSink;
use log::error;

pub async fn handle_sessions_list(
    app: &dyn GatewayApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.list_sessions().await {
        Ok(sessions) => {
            let sessions = sessions.into_iter().map(app_session_to_protocol).collect();
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                MessageEnvelope::SessionsListResponse(SessionsListResponse { sessions }),
            ));
        }
        Err(e) => {
            error!("Failed to list sessions: {}", e);
            crate::gateway::handlers::system::send_general_error(
                &outbound_tx,
                &request_id,
                e.to_string(),
                None::<String>,
            );
        }
    }
}

pub async fn handle_session_get(
    session_id: String,
    app: &dyn GatewayApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.session_messages(&session_id).await {
        Ok(messages) => {
            let messages = messages.into_iter().map(app_message_to_protocol).collect();
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                MessageEnvelope::SessionsMessagesResponse(SessionsMessagesResponse { messages }),
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

pub async fn handle_session_create(
    payload: SessionCreateRequest,
    app: &dyn GatewayApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.create_session(payload.title, payload.agent_id).await {
        Ok(session) => {
            let session = app_session_to_protocol(session);
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                MessageEnvelope::SessionsCreateResponse(SessionCreateResponse { session }),
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

pub async fn handle_session_delete(
    payload: crate::gateway::protocol::SessionIdPayload,
    app: &dyn GatewayApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.delete_session(&payload.session_id).await {
        Ok(success) => {
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                MessageEnvelope::SessionsDeleteResponse(SuccessResponse { success }),
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

pub async fn handle_session_copy(
    payload: crate::gateway::protocol::SessionCopyRequest,
    app: &dyn GatewayApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.copy_session(&payload.session_id, payload.index).await {
        Ok(session) => {
            let session = app_session_to_protocol(session);
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                MessageEnvelope::SessionsCopyResponse(SessionCreateResponse { session }),
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
