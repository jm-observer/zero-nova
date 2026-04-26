use crate::bridge::{app_message_to_protocol, app_session_to_protocol};
use channel_core::ResponseSink;
use log::error;
use nova_app::AgentApplication;
use nova_protocol::{
    GatewayMessage, MessageEnvelope, SessionCopyRequest, SessionCreateRequest, SessionCreateResponse, SessionIdPayload,
    SessionsListResponse, SessionsMessagesResponse, SuccessResponse,
};

pub async fn handle_sessions_list(
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.list_sessions().await {
        Ok(sessions) => {
            let sessions = sessions.into_iter().map(app_session_to_protocol).collect();
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::SessionsListResponse(SessionsListResponse { sessions }),
                ))
                .await;
        }
        Err(e) => {
            error!("Failed to list sessions: {}", e);
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_session_get(
    session_id: String,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.session_messages(&session_id).await {
        Ok(messages) => {
            let messages = messages.into_iter().map(app_message_to_protocol).collect();
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::SessionsMessagesResponse(SessionsMessagesResponse { messages }),
                ))
                .await;
        }
        Err(e) if e.to_string().contains("Session not found") => {
            super::system::send_general_error(
                &outbound_tx,
                &request_id,
                "Session not found".to_string(),
                None::<String>,
            )
            .await;
        }
        Err(e) => {
            error!("Failed to get session {}: {}", session_id, e);
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_session_create(
    payload: SessionCreateRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.create_session(payload.title, payload.agent_id).await {
        Ok(session) => {
            let session = app_session_to_protocol(session);
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::SessionsCreateResponse(SessionCreateResponse { session }),
                ))
                .await;
        }
        Err(e) => {
            error!("Failed to create session: {}", e);
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_session_delete(
    payload: SessionIdPayload,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.delete_session(&payload.session_id).await {
        Ok(success) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::SessionsDeleteResponse(SuccessResponse { success }),
                ))
                .await;
        }
        Err(e) => {
            error!("Failed to delete session {}: {}", payload.session_id, e);
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_session_copy(
    payload: SessionCopyRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.copy_session(&payload.session_id, payload.index).await {
        Ok(session) => {
            let session = app_session_to_protocol(session);
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::SessionsCopyResponse(SessionCreateResponse { session }),
                ))
                .await;
        }
        Err(e) if e.to_string().contains("Source session not found") => {
            super::system::send_general_error(
                &outbound_tx,
                &request_id,
                "Source session not found".to_string(),
                None::<String>,
            )
            .await;
        }
        Err(e) => {
            error!("Failed to copy session {}: {}", payload.session_id, e);
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_session_runtime(
    session_id: String,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.get_session_runtime(&session_id).await {
        Ok(runtime) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::SessionRuntimeResponse(runtime),
                ))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_session_prompt_preview(
    payload: nova_protocol::observability::PromptPreviewRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app
        .preview_session_prompt(&payload.session_id, payload.message_id)
        .await
    {
        Ok(preview) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::SessionPromptPreviewResponse(preview),
                ))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_session_tools(
    session_id: String,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.list_session_tools(&session_id).await {
        Ok(tools) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::SessionToolsListResponse(tools),
                ))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_session_skill_bindings(
    session_id: String,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.list_session_skill_bindings(&session_id).await {
        Ok(skills) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::SessionSkillBindingsResponse(skills),
                ))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_session_memory_hits(
    payload: nova_protocol::observability::SessionMemoryHitsRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.get_session_memory_hits(&payload.session_id, payload.turn_id).await {
        Ok(hits) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::SessionMemoryHitsResponse(hits),
                ))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_session_model_override(
    payload: nova_protocol::observability::SessionModelOverrideRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    let session_id = payload.session_id.clone();
    match app.override_session_model(&session_id, payload).await {
        Ok(runtime) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::SessionModelOverrideResponse(runtime),
                ))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_session_token_usage(
    session_id: String,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.get_session_token_usage(&session_id).await {
        Ok(usage) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::SessionTokenUsageResponse(usage),
                ))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

// --- Plan 2: Execution Records & Control Handlers ---

pub async fn handle_session_runs(
    payload: nova_protocol::observability::SessionRunsRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.list_session_runs(&payload.session_id).await {
        Ok(res) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(request_id, MessageEnvelope::SessionRunsResponse(res)))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_run_detail(
    payload: nova_protocol::observability::RunDetailRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.get_run_detail(&payload.run_id).await {
        Ok(res) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(request_id, MessageEnvelope::RunDetailResponse(res)))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_run_control(
    payload: nova_protocol::observability::RunControlRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.control_run(&payload.run_id.clone(), payload).await {
        Ok(_) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::RunControlResponse(nova_protocol::session::SuccessResponse { success: true }),
                ))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_session_artifacts(
    payload: nova_protocol::observability::SessionArtifactsRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.list_session_artifacts(&payload.session_id).await {
        Ok(res) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(request_id, MessageEnvelope::SessionArtifactsResponse(res)))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_permission_pending(
    payload: nova_protocol::observability::PermissionPendingRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.list_pending_permissions(Some(payload.session_id.as_str())).await {
        Ok(res) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(request_id, MessageEnvelope::PermissionPendingResponse(res)))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_permission_respond(
    payload: nova_protocol::observability::PermissionRespondRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.respond_to_permission(payload).await {
        Ok(_) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::PermissionRespondResponse(nova_protocol::session::SuccessResponse { success: true }),
                ))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_audit_logs(
    payload: nova_protocol::observability::AuditLogsRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.list_audit_logs(&payload.session_id).await {
        Ok(res) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(request_id, MessageEnvelope::AuditLogsResponse(res)))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_diagnostics_current(
    payload: nova_protocol::observability::DiagnosticsCurrentRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.get_diagnostics(&payload.session_id).await {
        Ok(res) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(request_id, MessageEnvelope::DiagnosticsResponse(res)))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}

pub async fn handle_workspace_restore(
    _payload: nova_protocol::observability::WorkspaceRestoreRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.restore_workspace().await {
        Ok(res) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(request_id, MessageEnvelope::WorkspaceRestoreResponse(res)))
                .await;
        }
        Err(e) => {
            super::system::send_general_error(&outbound_tx, &request_id, e.to_string(), None::<String>).await;
        }
    }
}
