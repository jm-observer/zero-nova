pub use nova_agent_config::*;

impl From<ConfiguredModel> for crate::provider::ModelConfig {
    fn from(value: ConfiguredModel) -> Self {
        Self {
            provider: value.provider,
            model: value.model,
            max_tokens: value.max_tokens,
            temperature: value.temperature,
            top_p: value.top_p,
            thinking_budget: value.thinking_budget,
            reasoning_effort: value.reasoning_effort,
            max_tokens_field: value.max_tokens_field,
            extra_body: value.extra_body,
        }
    }
}

impl From<ConfiguredAgentModel> for crate::agent_catalog::AgentModelOverride {
    fn from(value: ConfiguredAgentModel) -> Self {
        Self {
            model: value.model,
            temperature: value.temperature,
            max_tokens: value.max_tokens,
            top_p: value.top_p,
        }
    }
}
