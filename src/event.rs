use crate::message::Message;
use crate::provider::types::Usage;
use anyhow::Error;

#[derive(Debug)]
/// Turn complete event, containing new messages and usage information.
pub enum AgentEvent {
    /// Text delta emitted by the LLM.
    TextDelta(String),
    /// Tool invocation start event.
    ToolStart {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Tool invocation end event.
    ToolEnd {
        id: String,
        name: String,
        output: String,
        is_error: bool,
    },
    /// Turn complete event, containing new messages and usage information.
    TurnComplete { new_messages: Vec<Message>, usage: Usage },
    /// Agent reached the maximum number of iterations.
    IterationLimitReached { iterations: usize },
    /// Generic error event.
    Error(Error),
}
