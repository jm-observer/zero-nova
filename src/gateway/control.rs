// Control layer for conversation handling

use crate::gateway::agents::AgentRegistry;
use crate::gateway::protocol::InteractionOptionDTO;
use crate::gateway::workflow::{WorkflowEngine, WorkflowState};
use std::time::{SystemTime, UNIX_EPOCH};

/// Represents the overall control state attached to a Session.
pub struct ControlState {
    /// The currently active agent identifier (e.g., "default").
    pub active_agent: String,
    /// Optional pending interaction awaiting user response.
    pub pending_interaction: Option<PendingInteraction>,
    /// Optional ongoing workflow.
    pub workflow: Option<WorkflowState>,
}

impl ControlState {
    pub fn new(default_agent: &str) -> Self {
        Self {
            active_agent: default_agent.to_string(),
            pending_interaction: None,
            workflow: None,
        }
    }
}

/// Simple representation of a pending interaction.
pub struct PendingInteraction {
    pub id: String,
    pub kind: InteractionKind,
    pub subject: String,
    pub prompt: String,
    pub options: Vec<InteractionOption>,
    pub risk_level: RiskLevel,
    pub created_at: i64,
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionKind {
    Approve,
    Select,
    Input,
}

#[derive(Clone)]
pub struct InteractionOption {
    pub id: String,
    pub label: String,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

/// The result of interpreting a user's response to a pending interaction.
pub struct ResolutionResult {
    pub intent: ResolutionIntent,
    pub selected_option_id: Option<String>,
    pub free_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionIntent {
    Approve,
    Reject,
    Select,
    ProvideInput,
    Unclear,
}

pub struct InteractionResolver;

impl InteractionResolver {
    /// Pure rule‑based resolver for the Phase 4 prototype.
    /// Returns a `ResolutionResult` describing the user's intent.
    pub fn resolve(input: &str, pending: &PendingInteraction) -> ResolutionResult {
        // Helper to make case‑insensitive matching.
        let lower = input.to_ascii_lowercase();
        // Approve keywords
        let approve_words = [
            "好的", "ok", "ok.", "ok!", "ok?", "ok.", "ok!", "continue", "继续", "yes", "是", "confirm", "确认",
        ];
        let reject_words = ["不", "取消", "算了", "停", "no", "否", "不要", "reject", "reject."];
        // Check approve
        for w in &approve_words {
            if lower.contains(w) {
                return ResolutionResult {
                    intent: ResolutionIntent::Approve,
                    selected_option_id: None,
                    free_text: None,
                };
            }
        }
        // Check reject
        for w in &reject_words {
            if lower.contains(w) {
                return ResolutionResult {
                    intent: ResolutionIntent::Reject,
                    selected_option_id: None,
                    free_text: None,
                };
            }
        }
        // If pending is of Select kind, attempt to match options.
        if let InteractionKind::Select = pending.kind {
            // Try match by option id or label directly.
            for opt in &pending.options {
                if lower == opt.id.to_ascii_lowercase()
                    || lower == opt.label.to_ascii_lowercase()
                    || opt.aliases.iter().any(|a| lower == a.to_ascii_lowercase())
                {
                    return ResolutionResult {
                        intent: ResolutionIntent::Select,
                        selected_option_id: Some(opt.id.clone()),
                        free_text: None,
                    };
                }
            }
            // Simple ordinal matching: "第N个" or just a digit.
            // Expect "1" or "第一" patterns – for brevity we only support digits.
            if let Ok(idx) = lower.trim().parse::<usize>() {
                if idx > 0 && idx <= pending.options.len() {
                    let opt = &pending.options[idx - 1];
                    return ResolutionResult {
                        intent: ResolutionIntent::Select,
                        selected_option_id: Some(opt.id.clone()),
                        free_text: None,
                    };
                }
            }
        }

        // Input kind – everything else is treated as free text.
        if let InteractionKind::Input = pending.kind {
            return ResolutionResult {
                intent: ResolutionIntent::ProvideInput,
                selected_option_id: None,
                free_text: Some(input.to_string()),
            };
        }
        // Fallback
        ResolutionResult {
            intent: ResolutionIntent::Unclear,
            selected_option_id: None,
            free_text: None,
        }
    }
}

/// Simple router that decides the turn intent based on the current control state.
#[derive(Debug)]
pub enum TurnIntent {
    ResolvePendingInteraction,
    AddressAgent { agent_id: String },
    ContinueWorkflow,
    ExecuteChat,
}

pub struct TurnRouter;

impl TurnRouter {
    /// Determine the intent for the current user input.
    pub fn classify(input: &str, control: &ControlState, agent_registry: Option<&AgentRegistry>) -> TurnIntent {
        // 1️⃣ pending interaction takes highest priority
        if let Some(pending) = &control.pending_interaction {
            // Check expiration (ttl_seconds)
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64;
            if now - pending.created_at > pending.ttl_seconds as i64 {
                return TurnIntent::ExecuteChat;
            }
            return TurnIntent::ResolvePendingInteraction;
        }

        // 2️⃣ agent address (Phase 5 real implementation)
        if let Some(registry) = agent_registry {
            if let Some(agent_id) = registry.resolve_addressing(input) {
                if agent_id != control.active_agent {
                    return TurnIntent::AddressAgent { agent_id };
                }
            }
        }

        // 3️⃣ workflow continuation (Phase 5 real implementation)
        if let Some(wf) = &control.workflow {
            if WorkflowEngine::looks_like_continuation(wf) {
                return TurnIntent::ContinueWorkflow;
            }
        }

        // Default path – normal chat
        TurnIntent::ExecuteChat
    }
}

// Helper to create InteractionOptionDTO for protocol messages.
impl From<&InteractionOption> for InteractionOptionDTO {
    fn from(opt: &InteractionOption) -> Self {
        InteractionOptionDTO {
            id: opt.id.clone(),
            label: opt.label.clone(),
            aliases: opt.aliases.clone(),
        }
    }
}
