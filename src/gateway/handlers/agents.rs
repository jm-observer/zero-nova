use crate::gateway::protocol::{Agent, AgentsListResponse, AgentsSwitchResponse};
use crate::gateway::router::AppState;
use log::info;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::gateway::protocol::GatewayMessage;

pub async fn handle_agents_list<C: crate::provider::LlmClient>(
    _state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let _ = outbound_tx.send(GatewayMessage::new(
        request_id,
        crate::gateway::protocol::MessageEnvelope::AgentsListResponse(AgentsListResponse {
            agents: vec![Agent {
                id: "nova".to_string(),
                name: "Zero-Nova".to_string(),
                description: Some("A powerful AI agent built on zero-nova".to_string()),
                icon: None,
                system_prompt: None,
            }],
        }),
    ));
}

pub async fn handle_agents_switch(
    payload: crate::gateway::protocol::AgentIdPayload,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    info!("Switched to agent: {}", payload.agent_id);
    let _ = outbound_tx.send(GatewayMessage::new(
        request_id,
        crate::gateway::protocol::MessageEnvelope::AgentsSwitchResponse(AgentsSwitchResponse {
            agent: Agent {
                id: payload.agent_id,
                name: "Zero-Nova".to_string(),
                ..Default::default()
            },
            messages: vec![],
        }),
    ));
}
