use crate::handlers::{agents, chat, config, sessions, system};
use channel_core::ResponseSink;
use log::warn;
use nova_agent::app::AgentApplication;
use nova_protocol::{GatewayMessage, MessageEnvelope};
