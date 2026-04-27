use crate::bridge::app_agent_to_protocol;
use channel_core::ResponseSink;
use log::info;
use nova_agent::app::AgentApplication;
use nova_protocol::{
    AgentsListResponse, AgentsSwitchResponse, GatewayMessage, MessageEnvelope, SessionAgentSwitchPayload,
};

pub async fn handle_agents_list(
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    let agents = app.list_agents().into_iter().map(app_agent_to_protocol).collect();

    let _ = outbound_tx
        .send_async(GatewayMessage::new(
            request_id,
            MessageEnvelope::AgentsListResponse(AgentsListResponse { agents }),
        ))
        .await;
}

pub async fn handle_agents_switch(
    payload: SessionAgentSwitchPayload,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    info!("Switching session {} to agent {}", payload.session_id, payload.agent_id);

    match app.switch_agent(&payload.session_id, &payload.agent_id).await {
        Ok(agent) => {
            let agent = app_agent_to_protocol(agent);
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::AgentsSwitchResponse(AgentsSwitchResponse {
                        agent,
                        messages: vec![],
                    }),
                ))
                .await;
        }
        Err(error) => {
            super::system::send_general_error(
                &outbound_tx,
                &request_id,
                error.to_string(),
                Some(error_code(&error).to_string()),
            )
            .await;
        }
    }
}

pub async fn handle_agent_inspect(
    payload: nova_protocol::observability::AgentInspectRequest,
    app: &dyn AgentApplication,
    outbound_tx: ResponseSink<GatewayMessage>,
    request_id: String,
) {
    match app.inspect_agent(&payload.agent_id, &payload.session_id).await {
        Ok(resp) => {
            let _ = outbound_tx
                .send_async(GatewayMessage::new(
                    request_id,
                    MessageEnvelope::AgentInspectResponse(resp),
                ))
                .await;
        }
        Err(error) => {
            super::system::send_general_error(
                &outbound_tx,
                &request_id,
                error.to_string(),
                Some(error_code(&error).to_string()),
            )
            .await;
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
