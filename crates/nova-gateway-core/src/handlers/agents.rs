use crate::bridge::app_agent_to_protocol;
use channel_core::ResponseSink;
use log::info;
use nova_agent::app::AgentApplication;
use nova_protocol::{
    AgentsListResponse, AgentsSwitchResponse, GatewayMessage, MessageEnvelope, SessionAgentSwitchPayload,
};
