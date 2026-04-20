use crate::message::Message;
use crate::provider::types::Usage;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
/// Agent events emitted during a turn.
pub enum AgentEvent {
    /// Text delta emitted by the LLM.
    TextDelta(String),
    /// Thinking delta emitted by the LLM.
    ThinkingDelta(String),
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
    Error(String),
    /// Tool execution process streaming output (e.g., bash stdout/stderr)
    LogDelta {
        /// Corresponds to tool_use_id in ToolStart
        id: String,
        /// Tool name
        name: String,
        /// Log content (one or multiple aggregated lines)
        log: String,
        /// Source stream: "stdout" | "stderr"
        stream: String,
    },
}
