pub mod builder;
pub mod context;
pub mod side_channel;
pub mod templates;
pub mod trimmer;
pub mod types;
pub mod workflow;

pub use builder::{
    build_agent_catalog_hint, build_agent_catalog_section, filter_project_instruction_by_profile, SystemPromptBuilder,
};
pub use context::{
    detect_shell_command, load_developer_project_prompt_async, load_project_context_async,
    load_project_context_with_config_async, EnvironmentSnapshot,
};
pub use side_channel::{SideChannelConfig, SideChannelInjector};
pub use templates::{template_vars, TemplateContext, BEHAVIOR_GUARDS};
pub use trimmer::{HistoryTrimmer, TrimResult, TrimmerConfig};
pub use types::{
    ActiveSkillState, AgentCatalogEntry, NamedSection, ProjectInstructionProfile, PromptConstructionRequest,
    PromptExtraSections, PromptMaterial, PromptPriority, PromptSectionSize, SectionName, SkillInjectionMode,
    SkillInvocationLevel, SkillRouteDecision, SkillSwitchResult, ToolGuidanceMode, ToolSize, TurnContext,
    TurnPromptMaterial,
};
pub use workflow::WorkflowStagePrompts;
