use crate::app::application::GatewayApplication;
use crate::gateway::handlers::{agents, chat, config, sessions, system};
use crate::gateway::protocol::{GatewayMessage, MessageEnvelope};
use crate::provider::LlmClient;
use channel_websocket::ResponseSink;
use log::warn;
use std::sync::Arc;

/// 消息路由入口
pub async fn handle_message<C: LlmClient + 'static>(
    msg: GatewayMessage,
    app: Arc<GatewayApplication<C>>,
    outbound_tx: ResponseSink<GatewayMessage>,
) {
    let msg_id = match msg.id {
        Some(id) => id,
        None => {
            warn!("Received command without ID, ignoring: {:?}", msg.envelope);
            return;
        }
    };

    match msg.envelope {
        MessageEnvelope::Chat(payload) => {
            chat::handle_chat::<C>(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::ChatStop(payload) => {
            chat::handle_chat_stop::<C>(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsList => {
            sessions::handle_sessions_list::<C>(app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsMessages(payload) => {
            sessions::handle_session_get::<C>(payload.session_id, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsCreate(payload) => {
            sessions::handle_session_create::<C>(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsDelete(payload) => {
            sessions::handle_session_delete::<C>(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::SessionsCopy(payload) => {
            sessions::handle_session_copy::<C>(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::AgentsList => {
            agents::handle_agents_list::<C>(app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::AgentsSwitch(payload) => {
            agents::handle_agents_switch::<C>(payload, app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::ConfigGet => {
            config::handle_config_get::<C>(app, outbound_tx, msg_id).await;
        }
        MessageEnvelope::ConfigUpdate(payload) => {
            config::handle_config_update::<C>(payload, app, outbound_tx, msg_id).await;
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
            );
        }
    }
}
