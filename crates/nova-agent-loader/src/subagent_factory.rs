use anyhow::Result;
use async_trait::async_trait;
use nova_agent::agent::{AgentConfig, AgentRuntime, PromptDiagnosticsConfig, ToolResultCompactionConfig};
use nova_agent::app::ConfigSnapshot;
use nova_agent::config::AgentSpec;
use nova_agent::loop_guard::{DuplicateReadMode, LoopGuardConfig};
use nova_agent::network::HttpClients;
use nova_agent::prompt::TrimmerConfig;
use nova_agent::provider::openai_compat::OpenAiCompatClient;
use nova_agent::provider::ModelConfig;
use nova_agent::tool::builtin::agent::SubagentRuntimeFactory;
use nova_agent::tool::builtin::register_builtin_tools;
use nova_agent::tool::{ProjectDirService, ToolContext, ToolRegistry};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

struct NoopProjectDirService;

#[async_trait]
impl ProjectDirService for NoopProjectDirService {
    async fn get_project_dir(&self, _session_id: &str) -> Result<Option<PathBuf>> {
        anyhow::bail!("Project directory management is unavailable in subagent runtime")
    }

    async fn set_project_dir(&self, _session_id: &str, _project_dir: PathBuf) -> Result<PathBuf> {
        anyhow::bail!("Project directory management is unavailable in subagent runtime")
    }
}

#[derive(Clone)]
pub struct LoaderSubagentRuntimeFactory {
    config_snapshot: Arc<dyn ConfigSnapshot>,
}

impl LoaderSubagentRuntimeFactory {
    pub fn new(config_snapshot: Arc<dyn ConfigSnapshot>) -> Self {
        Self { config_snapshot }
    }
}

#[async_trait]
impl SubagentRuntimeFactory for LoaderSubagentRuntimeFactory {
    async fn build_runtime(
        &self,
        spec: &AgentSpec,
        binding: &nova_agent::config::ResolvedAgentBinding,
        model_override: Option<&str>,
        context: Option<&ToolContext>,
        _project_dir: Option<&Path>,
        environment: nova_agent::prompt::EnvironmentSnapshot,
    ) -> Result<(AgentRuntime<OpenAiCompatClient>, ModelConfig)> {
        let config = self.config_snapshot.current().await;
        let client = OpenAiCompatClient::from_registry_with_http_client_and_context_headers_enabled(
            config.providers.clone(),
            binding.provider_id.clone(),
            nova_agent::network::build_provider_client()?,
            config.outbound_context_headers.enabled,
        );

        let sub_registry = ToolRegistry::new();
        if let Some(ctx) = context {
            if let (Some(task_store), Some(skill_registry)) = (ctx.task_store.as_ref(), ctx.skill_registry.as_ref()) {
                let http_clients = HttpClients::new()?;
                register_builtin_tools(
                    &sub_registry,
                    &config,
                    task_store.clone(),
                    skill_registry.clone(),
                    spec.tool_whitelist.as_deref(),
                    Arc::new(NoopProjectDirService),
                    &http_clients,
                );
            }
        }

        let mut model_config = ModelConfig {
            provider: Some(binding.provider_id.clone()),
            model: spec.model_config.model.clone(),
            max_tokens: spec.model_config.max_tokens.unwrap_or(binding.model_config.max_tokens),
            temperature: Some(spec.model_config.temperature),
            top_p: Some(spec.model_config.top_p),
            thinking_budget: None,
            reasoning_effort: None,
            max_tokens_field: binding.model_config.max_tokens_field.clone(),
            extra_body: binding.model_config.extra_body.clone(),
        };
        if let Some(m) = model_override {
            model_config.model = m.to_string();
        }

        let agent_config = AgentConfig {
            max_iterations: config.gateway.max_iterations,
            model_config: model_config.clone(),
            tool_timeout: Duration::from_secs(config.gateway.subagent_timeout_secs),
            max_tokens: config.gateway.max_tokens,
            trimmer: TrimmerConfig {
                context_window: config.gateway.trimmer.context_window,
                output_reserve: config.gateway.trimmer.output_reserve,
                min_recent_messages: config.gateway.trimmer.min_recent_messages,
                enable_summary: false,
            },
            config_dir: config.config_dir.clone(),
            prompts_dir: config.prompts_dir(),
            project_context_file: config.project_context_file(),
            initial_env_snapshot: Some(environment),
            loop_guard: LoopGuardConfig {
                enabled: config.gateway.loop_guard.enabled,
                max_consecutive_duplicate_tool_calls: config.gateway.loop_guard.max_consecutive_duplicate_tool_calls,
                max_stalled_iterations: config.gateway.loop_guard.max_stalled_iterations,
                duplicate_read_mode: if config.gateway.loop_guard.duplicate_read_mode == "warn_only" {
                    DuplicateReadMode::WarnOnly
                } else {
                    DuplicateReadMode::WarnThenReject
                },
                iteration_trim_ratio: config.gateway.loop_guard.iteration_trim_ratio,
            },
            prompt_diagnostics: PromptDiagnosticsConfig {
                enabled: config.gateway.prompt_diagnostics.enabled,
                large_section_chars: config.gateway.prompt_diagnostics.large_section_chars,
                large_message_chars: config.gateway.prompt_diagnostics.large_message_chars,
                large_tool_result_chars: config.gateway.prompt_diagnostics.large_tool_result_chars,
            },
            tool_result_compaction: ToolResultCompactionConfig {
                enabled: config.gateway.tool_result_compaction.enabled,
                max_chars: config.gateway.tool_result_compaction.max_chars,
                head_chars: config.gateway.tool_result_compaction.head_chars,
                tail_chars: config.gateway.tool_result_compaction.tail_chars,
                disable_for_tools: config
                    .gateway
                    .tool_result_compaction
                    .disable_for_tools
                    .iter()
                    .map(|name| name.to_ascii_lowercase())
                    .collect(),
            },
        };

        let mut runtime = AgentRuntime::new(client, sub_registry, agent_config);
        if let Some(ctx) = context {
            runtime.task_store = ctx.task_store.clone();
            runtime.skill_registry = ctx.skill_registry.clone();
            runtime.read_files = ctx.read_files.clone();
        }
        Ok((runtime, model_config))
    }
}
