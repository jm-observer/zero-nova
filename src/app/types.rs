use crate::message::ContentBlock;
use crate::provider::types::Usage;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct AppSession {
    pub id: String,
    pub title: Option<String>,
    pub agent_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: usize,
}

#[derive(Debug, Clone)]
pub struct AppAgent {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppMessage {
    pub role: String,
    pub content: Vec<ContentBlock>,
    pub timestamp: i64,
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    Token(String),
    ThinkingDelta(String),
    ToolStart {
        id: String,
        name: String,
        input: Value,
    },
    ToolEnd {
        id: String,
        name: String,
        output: String,
        is_error: bool,
    },
    ToolLog {
        id: String,
        name: String,
        log: String,
        stream: String,
    },
    Iteration {
        current: usize,
        total: usize,
    },
    IterationLimitReached {
        iterations: usize,
    },
    AssistantMessage {
        content: Vec<ContentBlock>,
    },
    TurnComplete {
        usage: Usage,
    },
    Error(String),
    SystemLog(String),
    AgentSwitched {
        agent: AppAgent,
    },
}

impl From<crate::event::AgentEvent> for AppEvent {
    fn from(event: crate::event::AgentEvent) -> Self {
        match event {
            crate::event::AgentEvent::TextDelta(text) => AppEvent::Token(text),
            crate::event::AgentEvent::ThinkingDelta(text) => AppEvent::ThinkingDelta(text),
            crate::event::AgentEvent::ToolStart { id, name, input } => AppEvent::ToolStart { id, name, input },
            crate::event::AgentEvent::ToolEnd {
                id,
                name,
                output,
                is_error,
            } => AppEvent::ToolEnd {
                id,
                name,
                output,
                is_error,
            },
            crate::event::AgentEvent::LogDelta { id, name, log, stream } => AppEvent::ToolLog { id, name, log, stream },
            crate::event::AgentEvent::Iteration { current, total } => AppEvent::Iteration { current, total },
            crate::event::AgentEvent::IterationLimitReached { iterations } => {
                AppEvent::IterationLimitReached { iterations }
            }
            crate::event::AgentEvent::AssistantMessage { content } => AppEvent::AssistantMessage { content },
            crate::event::AgentEvent::TurnComplete { usage, .. } => AppEvent::TurnComplete { usage },
            crate::event::AgentEvent::Error(msg) => AppEvent::Error(msg),
            crate::event::AgentEvent::SystemLog(msg) => AppEvent::SystemLog(msg),
            crate::event::AgentEvent::AgentSwitched {
                agent_id,
                agent_name,
                description,
            } => AppEvent::AgentSwitched {
                agent: AppAgent {
                    id: agent_id,
                    name: agent_name,
                    description,
                },
            },
        }
    }
}
