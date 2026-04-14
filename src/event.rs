use crate::message::Message;
use crate::provider::types::Usage;
use anyhow::Error;

#[derive(Debug)]
pub enum AgentEvent {
    TextDelta(String),
    ToolStart {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolEnd {
        id: String,
        name: String,
        output: String,
        is_error: bool,
    },
    TurnComplete {
        new_messages: Vec<Message>,
        usage: Usage,
    },
    Error(Error),
}
