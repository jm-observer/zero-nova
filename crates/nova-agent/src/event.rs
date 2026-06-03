use crate::message::{ContentBlock, Message};
use crate::provider::types::Usage;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_session_request_serde_roundtrip() {
        let original = AgentEvent::ChildSessionRequest {
            tool_use_id: "tu_42".into(),
            tool_name: "start_review_session".into(),
            seed_user_message: "kickoff flagged_id=7 reason=路由错".into(),
            metadata: serde_json::json!({ "flagged_id": 7 }),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: AgentEvent = serde_json::from_str(&json).expect("deserialize");
        match parsed {
            AgentEvent::ChildSessionRequest {
                tool_use_id,
                tool_name,
                seed_user_message,
                metadata,
            } => {
                assert_eq!(tool_use_id, "tu_42");
                assert_eq!(tool_name, "start_review_session");
                assert_eq!(seed_user_message, "kickoff flagged_id=7 reason=路由错");
                assert_eq!(metadata["flagged_id"], 7);
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
/// Agent events emitted during a turn.
pub enum AgentEvent {
    /// Text delta emitted by the LLM.
    TextDelta(String),
    /// Thinking delta emitted by the LLM.
    ThinkingDelta(String),
    /// 单次 LLM 调用完成后的完整 HTTP trace（request body + 聚合后的 response body）。
    /// 供宿主（zero）做全生命周期追踪捕获 LLM body；nova 自身不消费。
    ProviderHttpTrace {
        request_body: serde_json::Value,
        response_body: serde_json::Value,
    },
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
    /// 某次工具调用声明了开启子会话副作用（来自 [`crate::tool::ChildSessionRequest`]）。
    ///
    /// nova SDK 自身不处理此事件，由外部宿主消费以执行实际的会话切换 / 隔离。
    /// 在同一工具调用的 [`AgentEvent::ToolEnd`] **之后**发出，保证语义"工具已结束 → 触发副作用"。
    ChildSessionRequest {
        /// 触发本次副作用的 tool_use id（关联到对应 ToolEnd）。
        tool_use_id: String,
        /// 工具名（调试便利）。
        tool_name: String,
        /// 工具声明的种子 user message。
        seed_user_message: String,
        /// 工具声明的结构化负载。
        metadata: serde_json::Value,
    },
    /// Orchestration progress event (typed, replaces string-based SystemLog for orchestration).
    OrchestrationProgress { kind: String, payload: serde_json::Value },
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
}
