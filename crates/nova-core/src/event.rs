use crate::message::{ContentBlock, Message};
use crate::prompt::{SkillInvocationLevel, SkillRouteDecision};
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
    /// A task was created.
    TaskCreated { id: String, subject: String },
    /// A task status changed.
    TaskStatusChanged {
        id: String,
        subject: String,
        status: String,
        active_form: Option<String>,
    },
    /// A background task was completed.
    BackgroundTaskComplete { id: String, name: String },
    /// A skill was loaded.
    SkillLoaded { skill_name: String },
    /// Skill was activated during a turn.
    SkillActivated {
        skill_id: String,
        skill_name: String,
        sticky: bool,
        // "auto" | "explicit" | "fallback"
        reason: String,
    },
    /// Skill was switched from one to another.
    SkillSwitched {
        from_skill: String,
        to_skill: String,
        reason: String,
    },
    /// Skill was exited/deactivated.
    SkillExited {
        skill_id: String,
        // Reason for deactivation
        reason: String,
    },
    /// Skill route evaluation was done.
    SkillRouteEvaluated {
        result: SkillRouteDecision,
        confidence: f64, // 0.0 - 1.0
        // Free-text reasoning from LLM or rule engine
        reasoning: String,
    },
    /// Tool was unlocked via deferred loading (e.g., ToolSearch).
    ToolUnlocked { tool_name: String },
    /// Skill invocation at a specific level (三层模型).
    SkillInvocation {
        skill_id: String,
        skill_name: String,
        level: SkillInvocationLevel,
    },
}
