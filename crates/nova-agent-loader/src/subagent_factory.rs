use nova_agent::config::AppConfig;
use std::sync::Arc;

/// Build a concrete subagent runtime builder from config.
pub fn build_subagent_runtime_builder(
    config: Arc<AppConfig>,
) -> nova_agent::tool::builtin::agent::SubagentRuntimeBuilder {
    nova_agent::tool::builtin::agent::SubagentRuntimeBuilder::new(config)
}

/// Build a concrete subagent prompt service from config.
pub fn build_agent_prompt_service(config: Arc<AppConfig>) -> nova_agent::tool::builtin::agent::SubagentPromptService {
    nova_agent::tool::builtin::agent::SubagentPromptService::from_config(&config)
}

/// Build a `nova_agent::app::agent_workspace_service::ReloadSessionPromptService` from config.
pub fn build_reload_session_prompt_service(
    config: Arc<AppConfig>,
) -> nova_agent::app::agent_workspace_service::ReloadSessionPromptService {
    nova_agent::app::agent_workspace_service::ReloadSessionPromptService::from_config(&config)
}
