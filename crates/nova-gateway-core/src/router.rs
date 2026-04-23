use crate::handlers::{agents, chat, config, sessions, system};
use channel_core::ResponseSink;
use log::warn;
use nova_app::AgentApplication;
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
