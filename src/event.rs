use crate::message::{ContentBlock, Message};
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
    /// Agent working iteration info
    Iteration { current: usize, total: usize },
    /// System-level logs (e.g. Iteration progress, internal errors)
    SystemLog(String),
    /// Tool execution process streaming output (e.g., bash stdout/stderr)
    LogDelta {
        id: String,
        name: String,
        log: String,
        stream: String,
    },
    /// 发送完整的 Assistant 消息块
    AssistantMessage { content: Vec<ContentBlock> },
    /// Agent 切换完成
    AgentSwitched {
        agent_id: String,
        agent_name: String,
        description: Option<String>,
    },
    /// 发送交互请求
    InteractionRequest {
        interaction_id: String,
        kind: String,
        subject: String,
        prompt: String,
        options: Vec<InteractionOptionEvent>,
    },
    /// 发送交互解决事件
    InteractionResolved { interaction_id: String, result: String },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InteractionOptionEvent {
    pub id: String,
    pub label: String,
    pub aliases: Vec<String>,
}
