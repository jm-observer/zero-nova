use crate::handlers::{agents, chat, config, sessions, system};
use channel_core::ResponseSink;
use log::warn;
use nova_agent::AgentApplication;
use nova_protocol::{GatewayMessage, MessageEnvelope};

/// 消息路由将请求分发到具体处理器
pub async fn dispatch(msg: GatewayMessage, app: &dyn AgentApplication, outbound_tx: ResponseSink<GatewayMessage>) {
    let msg_id = match msg.id {
        Some(id) => id,
        None => {
            warn!("Received command without ID, ignoring: {:?}", msg.envelope);
            return;
        }
    };

    match msg.envelope {
        MessageEnvelope::Chat(payload) => {
            chat::handle_chat(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::ChatStop(payload) => {
            chat::handle_chat_stop(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsList => {
            sessions::handle_sessions_list(app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsMessages(payload) => {
            sessions::handle_session_get(payload.session_id, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsCreate(payload) => {
            sessions::handle_session_create(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsDelete(payload) => {
            sessions::handle_session_delete(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsCopy(payload) => {
            sessions::handle_session_copy(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::AgentsList => {
            agents::handle_agents_list(app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::AgentsSwitch(payload) => {
            agents::handle_agents_switch(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::ConfigGet => {
            config::handle_config_get(app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::ConfigUpdate(payload) => {
            config::handle_config_update(payload, app, outbound_tx, msg_id).await;
        }
        // --- Observability & Control (Plan 1 & 2) ---
        MessageEnvelope::AgentInspect(payload) => {
            agents::handle_agent_inspect(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionRuntime(payload) => {
            sessions::handle_session_runtime(payload.session_id, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionPromptPreview(payload) => {
            sessions::handle_session_prompt_preview(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionToolsList(payload) => {
            sessions::handle_session_tools(payload.session_id, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionSkillBindings(payload) => {
            sessions::handle_session_skill_bindings(payload.session_id, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionMemoryHits(payload) => {
            sessions::handle_session_memory_hits(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionModelOverride(payload) => {
            sessions::handle_session_model_override(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionTokenUsage(payload) => {
            sessions::handle_session_token_usage(payload.session_id, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionRuns(payload) => {
            sessions::handle_session_runs(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::RunDetail(payload) => {
            sessions::handle_run_detail(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::RunControl(payload) => {
            sessions::handle_run_control(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionArtifacts(payload) => {
            sessions::handle_session_artifacts(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::PermissionPending(payload) => {
            sessions::handle_permission_pending(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::PermissionRespond(payload) => {
            sessions::handle_permission_respond(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::AuditLogs(payload) => {
            sessions::handle_audit_logs(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::DiagnosticsCurrent(payload) => {
            sessions::handle_diagnostics_current(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::WorkspaceRestore(payload) => {
            sessions::handle_workspace_restore(payload, app, outbound_tx, msg_id).await;
        }
        _ => {
            warn!(
                "Unhandled or not implemented message envelope for id={}: {:?}",
                msg_id, msg.envelope
            );
            system::send_general_error_direct(
                &outbound_tx,
                &msg_id,
                "Not implemented".to_string(),
                Some("NOT_IMPLEMENTED".to_string()),
            )
            .await;
        }
    }
}
