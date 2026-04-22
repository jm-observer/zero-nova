use crate::gateway::protocol::{Agent, AgentsListResponse, AgentsSwitchResponse, GatewayMessage};
use crate::gateway::router::AppState;
use log::info;
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn handle_agents_list<C: crate::provider::LlmClient>(
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    let agents = state.conversation_service.agent_registry.list();
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

pub async fn handle_agents_switch<C: crate::provider::LlmClient>(
    payload: crate::gateway::protocol::AgentIdPayload,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    info!("Switched to agent: {}", payload.agent_id);

    match state.conversation_service.agent_registry.get(&payload.agent_id) {
        Some(agent) => {
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                crate::gateway::protocol::MessageEnvelope::AgentsSwitchResponse(AgentsSwitchResponse {
                    agent: Agent {
                        id: agent.id.clone(),
                        name: agent.display_name.clone(),
                        description: Some(agent.description.clone()),
                        ..Default::default()
                    },
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
