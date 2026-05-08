use crate::{
    handlers::{agents, chat, config, sessions, system, voice},
    PushCenter,
};
use channel_core::ResponseSink;
use log::{debug, warn};
use nova_agent::app::AgentApplication;
use nova_protocol::{GatewayMessage, MessageEnvelope};
use std::sync::Arc;

/// 消息路由将请求分发到具体处理器
pub async fn dispatch(
    msg: GatewayMessage,
    peer_id: &str,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    push_center: Arc<PushCenter>,
) {
    let msg_id = match msg.id {
        Some(id) => id,
        None => {
            warn!("Received command without ID, ignoring: {:?}", msg.envelope);
            return;
        }
    };

    debug!(
        "[INBOUND] dispatch: msg_id={}, envelope_type={}",
        msg_id,
        std::any::type_name_of_val(&msg.envelope)
    );

    match msg.envelope {
        MessageEnvelope::Chat(payload) => {
            chat::handle_chat(payload, app, outbound_tx, msg_id, peer_id, push_center).await;
        }
        MessageEnvelope::ChatStop(payload) => {
            chat::handle_chat_stop(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsList => {
            sessions::handle_sessions_list(app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsMessages(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
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
        MessageEnvelope::VoiceCapabilitiesGet(_) => {
            voice::handle_voice_capabilities(app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::VoiceTranscribeRequest(payload) => {
            voice::handle_voice_transcribe(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::VoiceTtsRequest(payload) => {
            voice::handle_voice_tts(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::AgentInspect(payload) => {
            agents::handle_agent_inspect(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::ProviderHealth(_) => {
            sessions::handle_provider_health(app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionRuntime(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
            sessions::handle_session_runtime(payload.session_id, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionPromptPreview(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
            sessions::handle_session_prompt_preview(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionSystemPromptReload(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
            sessions::handle_session_system_prompt_reload(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionToolsList(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
            sessions::handle_session_tools(payload.session_id, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionFileTreeList(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
            sessions::handle_session_file_tree_list(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionSkillBindings(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
            sessions::handle_session_skill_bindings(payload.session_id, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionMemoryHits(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
            sessions::handle_session_memory_hits(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionModelOverride(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
            sessions::handle_session_model_override(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionTokenUsage(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
            sessions::handle_session_token_usage(payload.session_id, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionTokenUsageDetail(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
            sessions::handle_session_token_usage_detail(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionRuns(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
            sessions::handle_session_runs(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::RunDetail(payload) => {
            sessions::handle_run_detail(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::RunControl(payload) => {
            sessions::handle_run_control(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionArtifacts(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
            sessions::handle_session_artifacts(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::PermissionPending(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
            sessions::handle_permission_pending(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::PermissionRespond(payload) => {
            sessions::handle_permission_respond(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::AuditLogs(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
            sessions::handle_audit_logs(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::DiagnosticsCurrent(payload) => {
            push_center
                .subscribe_peer_to_session(peer_id, &payload.session_id)
                .await;
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
