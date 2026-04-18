use crate::gateway::protocol::{Agent, AgentsListResponse, AgentsSwitchResponse};
use crate::gateway::router::AppState;
use log::info;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::gateway::protocol::GatewayMessage;

pub async fn handle_agents_list<C: crate::provider::LlmClient>(
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let agents = state.agent_registry.list();
    let agents_dto = agents
        .into_iter()
        .map(|a| Agent {
            id: a.id.clone(),
            name: a.display_name.clone(),
            description: Some(a.description.clone()),
            icon: None,
            system_prompt: None,
        })
        .collect();

    let _ = outbound_tx.send(GatewayMessage::new(
        request_id,
        crate::gateway::protocol::MessageEnvelope::AgentsListResponse(AgentsListResponse { agents: agents_dto }),
    ));
}

pub async fn handle_agents_switch(
    payload: crate::gateway::protocol::AgentIdPayload,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    // Note: session switching is handled via chat flows normally,
    // this handler is for forced switching or UI direct updates.
    info!("Switched to agent: {}", payload.agent_id);
    let _ = outbound_tx.send(GatewayMessage::new(
        request_id,
        crate::gateway::protocol::MessageEnvelope::AgentsSwitchResponse(AgentsSwitchResponse {
            agent: Agent {
                id: payload.agent_id,
                name: "Agent".to_string(), // Dummy name, frontend usually refreshes list
                ..Default::default()
            },
            messages: vec![],
        }),
    ));
}
