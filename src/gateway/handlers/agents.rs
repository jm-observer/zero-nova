use crate::app::application::GatewayApplication;
use crate::gateway::protocol::{Agent, AgentsListResponse, AgentsSwitchResponse, GatewayMessage};
use channel_websocket::ResponseSink;
use log::info;

pub async fn handle_agents_list<C: crate::provider::LlmClient + 'static>(
    app: &GatewayApplication<C>,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    let agents_dto = app.list_agents();

    let _ = outbound_tx.send(GatewayMessage::new(
        request_id,
        crate::gateway::protocol::MessageEnvelope::AgentsListResponse(AgentsListResponse { agents: agents_dto }),
    ));
}

pub async fn handle_agents_switch<C: crate::provider::LlmClient + 'static>(
    payload: crate::gateway::protocol::AgentIdPayload,
    app: &GatewayApplication<C>,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    info!("Switched to agent: {}", payload.agent_id);

    match app.get_agent(&payload.agent_id) {
        Some(agent) => {
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                crate::gateway::protocol::MessageEnvelope::AgentsSwitchResponse(AgentsSwitchResponse {
                    agent: Agent { ..agent },
                    messages: vec![],
                }),
            ));
        }
        None => {
            crate::gateway::handlers::system::send_general_error(
                &outbound_tx,
                &request_id,
                format!("Agent {} not found", payload.agent_id),
                Some("AGENT_NOT_FOUND".to_string()),
            );
        }
    }
}
