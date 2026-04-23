use crate::app::application::GatewayApplication;
use crate::gateway::bridge::app_agent_to_protocol;
use crate::gateway::protocol::{AgentsListResponse, AgentsSwitchResponse, GatewayMessage, SessionAgentSwitchPayload};
use channel_websocket::ResponseSink;
use log::info;

pub async fn handle_agents_list(
    app: &dyn GatewayApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    let agents = app.list_agents().into_iter().map(app_agent_to_protocol).collect();

    let _ = outbound_tx.send(GatewayMessage::new(
        request_id,
        crate::gateway::protocol::MessageEnvelope::AgentsListResponse(AgentsListResponse { agents }),
    ));
}

pub async fn handle_agents_switch(
    payload: SessionAgentSwitchPayload,
    app: &dyn GatewayApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    info!("Switching session {} to agent {}", payload.session_id, payload.agent_id);

    match app.switch_agent(&payload.session_id, &payload.agent_id).await {
        Ok(agent) => {
            let agent = app_agent_to_protocol(agent);
            let _ = outbound_tx.send(GatewayMessage::new(
                request_id,
                crate::gateway::protocol::MessageEnvelope::AgentsSwitchResponse(AgentsSwitchResponse {
                    agent,
                    messages: vec![],
                }),
            ));
        }
        Err(error) => {
            crate::gateway::handlers::system::send_general_error(
                &outbound_tx,
                &request_id,
                error.to_string(),
                Some(error_code(&error).to_string()),
            );
        }
    }
}

fn error_code(error: &anyhow::Error) -> &'static str {
    if error.to_string().contains("Session not found") {
        "SESSION_NOT_FOUND"
    } else if error.to_string().contains("Agent '") && error.to_string().contains("not found") {
        "AGENT_NOT_FOUND"
    } else {
        "SERVICE_ERROR"
    }
}
