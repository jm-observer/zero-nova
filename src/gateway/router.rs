use crate::gateway::agents::AgentRegistry;
use crate::gateway::handlers::{agents, chat, config, scheduler, sessions, system};
use crate::gateway::protocol::{AuthRequest, GatewayMessage, MessageEnvelope};
use crate::provider::LlmClient;
use log::warn;
use std::sync::Arc;
use tokio::sync::mpsc;

/// 共享应用状态
pub struct AppState<C: LlmClient> {
    pub agent: crate::agent::AgentRuntime<C>,
    pub agent_registry: AgentRegistry,
    pub sessions: crate::gateway::session::SessionStore,
    pub config: std::sync::Arc<std::sync::RwLock<crate::config::AppConfig>>,
    pub config_path: std::path::PathBuf,
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
            system::send_general_error_direct(
                &outbound_tx,
                &msg_id,
                "Auth not implemented".to_string(),
                Some("NOT_IMPLEMENTED".to_string()),
            );
        }
        MessageEnvelope::Chat(payload) => {
            chat::handle_chat(payload, state, outbound_tx, msg_id).await;
        }
        MessageEnvelope::ChatStop(payload) => {
            chat::handle_chat_stop(payload, state, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsList => {
            sessions::handle_sessions_list(state, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsMessages(payload) => {
            sessions::handle_session_get(payload.session_id, state, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsCreate(payload) => {
            sessions::handle_session_create(payload, state, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsDelete(payload) => {
            sessions::handle_session_delete(payload, state, outbound_tx, msg_id).await;
        }
        MessageEnvelope::AgentsList => {
            agents::handle_agents_list::<C>(state, outbound_tx, msg_id).await;
        }
        MessageEnvelope::AgentsSwitch(payload) => {
            agents::handle_agents_switch(payload, outbound_tx, msg_id).await;
        }
        MessageEnvelope::ConfigGet => {
            config::handle_config_get(state, outbound_tx, msg_id).await;
        }
        MessageEnvelope::ConfigUpdate(payload) => {
            config::handle_config_update(payload, state, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SchedulerList => {
            scheduler::handle_scheduler_list(outbound_tx, msg_id).await;
        }
        MessageEnvelope::BrowserStatus
        | MessageEnvelope::ConfigGetLlmSource
        | MessageEnvelope::RouterConfigGet
        | MessageEnvelope::WeixinConfigGet
        | MessageEnvelope::SessionsArtifacts(_)
        | MessageEnvelope::SessionsLogs(_)
        | MessageEnvelope::LanguageUpdate(_)
        | MessageEnvelope::OpenFluxStatus
        | MessageEnvelope::VoiceGetStatus => {}

        // Stub / Not implemented handling
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
            );
        }
    }
}
