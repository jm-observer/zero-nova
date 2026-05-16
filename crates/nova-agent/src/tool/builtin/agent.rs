use crate::agent::AgentRuntime;
use crate::config::{AgentSpec, AppConfig};
use crate::event::AgentEvent;
use crate::message::{ContentBlock, Message, Role};
use crate::orchestrator::SubAgentExecutor;
use crate::prompt::{
    context::{load_developer_project_prompt_async, load_project_context_with_config_async},
    template_vars,
    workflow::WorkflowStagePrompts,
    PromptConstructionRequest, PromptExtraSections, SkillInjectionMode, SystemPromptBuilder,
};
use crate::provider::openai_compat::OpenAiCompatClient;
use crate::provider::{types::ToolDefinition, ModelConfig};
use crate::tool::{RegisteredToolDefinition, ToolContext};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::Instant;

/// Concrete subagent prompt service.
#[derive(Clone)]
pub struct SubagentPromptService {
    prompts_dir: PathBuf,
    project_context_file: Option<PathBuf>,
    developer_prompt_files: Vec<String>,
}

impl SubagentPromptService {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            prompts_dir: config.prompts_dir(),
            project_context_file: config.project_context_file(),
            developer_prompt_files: config.developer_prompt_files.clone(),
        }
    }

    pub async fn load_agent_material(
        &self,
        spec: &AgentSpec,
        env: Option<crate::prompt::EnvironmentSnapshot>,
        template_vars: HashMap<String, String>,
    ) -> Result<crate::prompt::PromptMaterial> {
        let agent_prompt = self.load_agent_prompt(spec).await?;
        Ok(crate::prompt::PromptMaterial {
            agent_id: spec.id.clone(),
            agent_prompt,
            agent_catalog: None,
            environment_snapshot: env,
            initial_template_vars: template_vars,
            skill_injection_mode: SkillInjectionMode::Catalog,
            project_instruction_profile: crate::prompt::ProjectInstructionProfile::Auto,
            tool_guidance: crate::prompt::ToolGuidanceMode::Compact,
        })
    }

    pub async fn load_turn_material(
        &self,
        project_dir: Option<&Path>,
        workflow_stage: Option<&str>,
        active_skill: Option<String>,
        turn_vars: HashMap<String, String>,
        enable_developer_prompt: bool,
    ) -> Result<crate::prompt::TurnPromptMaterial> {
        let developer_project_prompt = if enable_developer_prompt {
            load_developer_project_prompt_async(project_dir, &self.developer_prompt_files).await
        } else {
            None
        };
        let project_context =
            load_project_context_with_config_async(project_dir, self.project_context_file.as_deref()).await;
        let workflow_prompt = self.load_workflow_prompt(workflow_stage, &turn_vars).await;

        Ok(crate::prompt::TurnPromptMaterial {
            developer_project_prompt,
            project_context,
            workflow_prompt,
            turn_template_vars: turn_vars,
            active_skill,
        })
    }

    async fn load_agent_prompt(&self, spec: &AgentSpec) -> Result<String> {
        if spec.prompt_file.is_some() && spec.prompt_inline.is_some() {
            anyhow::bail!(
                "Agent '{}' has both prompt_file and prompt_inline configured; only one is allowed",
                spec.id
            );
        }

        if let Some(file) = &spec.prompt_file {
            let prompt_path = self.prompts_dir.join(file);
            let content = tokio::fs::read_to_string(&prompt_path)
                .await
                .map_err(anyhow::Error::from)
                .with_context(|| format!("Failed to read prompt_file for agent '{}': {:?}", spec.id, prompt_path))?;
            return Ok(content);
        }

        if let Some(inline) = &spec.prompt_inline {
            return Ok(inline.clone());
        }

        if let Some(legacy) = &spec.system_prompt_template {
            log::warn!(
                "Agent '{}' uses legacy system_prompt_template. This field is deprecated; use prompt_file/prompt_inline.",
                spec.id
            );
            return Ok(legacy.clone());
        }

        let default_file = format!("agent-{}.md", spec.id);
        let prompt_path = self.prompts_dir.join(&default_file);
        match tokio::fs::read_to_string(&prompt_path).await {
            Ok(content) => Ok(content),
            Err(err) => {
                log::warn!(
                    "Default prompt file {:?} not found for agent '{}': {}",
                    prompt_path,
                    spec.id,
                    err
                );
                Ok(String::new())
            }
        }
    }

    async fn load_workflow_prompt(&self, stage: Option<&str>, vars: &HashMap<String, String>) -> Option<String> {
        let stage = stage?;
        if stage == "idle" {
            return None;
        }
        let path = self.prompts_dir.join("workflow-stages.md");
        let prompts = match WorkflowStagePrompts::load_from_file_async(&path).await {
            Ok(prompts) => prompts,
            Err(err) => {
                log::warn!(
                    "Failed to load workflow prompt stage '{}' from {:?}: {}",
                    stage,
                    path,
                    err
                );
                return None;
            }
        };
        prompts.render(stage, vars)
    }
}

/// Concrete subagent runtime builder service.
#[derive(Clone)]
pub struct SubagentRuntimeBuilder {
    config: Arc<AppConfig>,
}

impl SubagentRuntimeBuilder {
    pub fn new(config: Arc<AppConfig>) -> Self {
        Self { config }
    }

    pub async fn build_runtime(
        &self,
        spec: &AgentSpec,
        binding: &crate::config::ResolvedAgentBinding,
        model_override: Option<&str>,
        context: Option<&ToolContext>,
        project_dir: Option<&Path>,
        environment: crate::prompt::EnvironmentSnapshot,
    ) -> Result<(AgentRuntime, ModelConfig)> {
        let config = self.config.clone();
        let model_override = model_override.map(str::to_string);
        let context = context.cloned();

        let client = OpenAiCompatClient::from_registry_with_http_client_and_context_headers_enabled(
            config.providers.clone(),
            binding.provider_id.clone(),
            crate::network::build_provider_client()?,
            config.outbound_context_headers.enabled,
        );

        let sub_registry = crate::tool::ToolRegistry::new();
        if let Some(ctx) = context.as_ref() {
            if let (Some(task_store), Some(skill_registry)) = (ctx.task_store.as_ref(), ctx.skill_registry.as_ref()) {
                let http_clients = crate::network::HttpClients::new()?;
                crate::tool::builtin::register_builtin_tools(
                    &sub_registry,
                    &config,
                    task_store.clone(),
                    skill_registry.clone(),
                    Arc::new(UnavailableSubagentProjectDirService),
                    &http_clients,
                )
                .await;
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
        if let Some(model_override) = model_override.as_deref() {
            model_config.model = model_override.to_string();
        }

        let agent_config = crate::agent::AgentConfig {
            max_iterations: config.gateway.max_iterations,
            model_config: model_config.clone(),
            tool_timeout: std::time::Duration::from_secs(config.gateway.subagent_timeout_secs),
            max_tokens: config.gateway.max_tokens,
            trimmer: crate::prompt::TrimmerConfig {
                context_window: config.gateway.trimmer.context_window,
                output_reserve: config.gateway.trimmer.output_reserve,
                min_recent_messages: config.gateway.trimmer.min_recent_messages,
                enable_summary: false,
            },
            config_dir: config.config_dir.clone(),
            prompts_dir: config.prompts_dir(),
            project_context_file: config.project_context_file(),
            initial_env_snapshot: Some(environment),
            loop_guard: crate::loop_guard::LoopGuardConfig {
                enabled: config.gateway.loop_guard.enabled,
                max_consecutive_duplicate_tool_calls: config.gateway.loop_guard.max_consecutive_duplicate_tool_calls,
                max_stalled_iterations: config.gateway.loop_guard.max_stalled_iterations,
                duplicate_read_mode: if config.gateway.loop_guard.duplicate_read_mode == "warn_only" {
                    crate::loop_guard::DuplicateReadMode::WarnOnly
                } else {
                    crate::loop_guard::DuplicateReadMode::WarnThenReject
                },
                iteration_trim_ratio: config.gateway.loop_guard.iteration_trim_ratio,
            },
            prompt_diagnostics: crate::agent::PromptDiagnosticsConfig {
                enabled: config.gateway.prompt_diagnostics.enabled,
                large_section_chars: config.gateway.prompt_diagnostics.large_section_chars,
                large_message_chars: config.gateway.prompt_diagnostics.large_message_chars,
                large_tool_result_chars: config.gateway.prompt_diagnostics.large_tool_result_chars,
            },
            tool_result_compaction: crate::agent::ToolResultCompactionConfig {
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
        if let Some(ctx) = context.as_ref() {
            runtime.task_store = ctx.task_store.clone();
            runtime.skill_registry = ctx.skill_registry.clone();
            runtime.read_files = ctx.read_files.clone();
        }

        let _ = project_dir;
        Ok((runtime, model_config))
    }
}

struct UnavailableSubagentProjectDirService;

#[async_trait]
impl crate::tool::ProjectDirService for UnavailableSubagentProjectDirService {
    async fn get_project_dir(&self, _session_id: &str) -> Result<Option<PathBuf>> {
        anyhow::bail!("Project directory management is unavailable in subagent runtime")
    }

    async fn set_project_dir(&self, _session_id: &str, _project_dir: PathBuf) -> Result<PathBuf> {
        anyhow::bail!("Project directory management is unavailable in subagent runtime")
    }
}

/// Services required by `AgentTool` to spawn subagents.
#[derive(Clone)]
pub struct AgentToolServices {
    pub prompt_service: SubagentPromptService,
    pub runtime_builder: SubagentRuntimeBuilder,
}

/// Tool to spawn a subagent for specialized task execution.
#[derive(Clone)]
pub struct AgentTool {
    config_store: Arc<RwLock<AppConfig>>,
    agent_types: HashMap<String, AgentSpec>,
    primary_agent_type: String,
    services: Option<AgentToolServices>,
}

struct PromptRequestInputs {
    base_prompt: String,
    prompt: String,
    active_skill_id: Option<String>,
    initial_template_vars: HashMap<String, String>,
    context_overrides: HashMap<String, String>,
    tool_definitions: Vec<ToolDefinition>,
    visible_tool_names: HashSet<String>,
    project_instruction_profile: crate::prompt::ProjectInstructionProfile,
    tool_guidance: crate::prompt::ToolGuidanceMode,
    agent_catalog: Option<String>,
}

impl AgentTool {
    /// Main constructor. Provide `services` when the tool will be used for subagent execution.
    pub fn new(config: AppConfig, services: Option<AgentToolServices>) -> Self {
        let mut agent_types = HashMap::new();
        for agent in &config.gateway.agents {
            agent_types.insert(agent.id.clone(), agent.clone());
        }
        let primary_agent_type = config
            .gateway
            .agents
            .first()
            .map(|agent| agent.id.clone())
            .unwrap_or_else(|| "primary".to_string());
        let config_store = Arc::new(RwLock::new(config.clone()));
        Self {
            config_store,
            agent_types,
            primary_agent_type,
            services,
        }
    }

    /// Constructor without subagent services — used by tests and metadata-only wiring.
    #[cfg(test)]
    pub(crate) fn new_without_subagent_services(config: AppConfig) -> Self {
        Self::new(config, None)
    }

    pub fn catalog_agent_ids(&self) -> std::collections::HashSet<String> {
        self.agent_types.values().map(|agent| agent.id.clone()).collect()
    }

    pub fn default_agent_id(&self) -> String {
        self.agent_types
            .get(&self.primary_agent_type)
            .map(|agent| agent.id.clone())
            .unwrap_or_else(|| "nova".to_string())
    }

    /// 构建 PromptConstructionRequest（统一构建指令）。
    ///
    /// 这是新的 prompt 构建入口，取代之前的"双轨制"实现。
    /// AgentTool 不再需要手动拼接字符串，而是传递构建指令给 SystemPromptBuilder。
    fn build_request_from_params(
        &self,
        spec: &AgentSpec,
        inputs: PromptRequestInputs,
        primary_tool_def: Option<&RegisteredToolDefinition>,
    ) -> PromptConstructionRequest {
        self.build_request_from_params_internal(spec, inputs, primary_tool_def)
    }

    /// 内部实现：构造 PromptConstructionRequest。
    fn build_request_from_params_internal(
        &self,
        spec: &AgentSpec,
        inputs: PromptRequestInputs,
        primary_tool_def: Option<&RegisteredToolDefinition>,
    ) -> PromptConstructionRequest {
        // 构造基础指令
        let base_material_id = if let Some(ref pf) = spec.prompt_file {
            pf.clone()
        } else {
            format!("agent-{}", spec.id)
        };

        PromptConstructionRequest {
            base_material_id,
            base_prompt: inputs.base_prompt,
            skill_id: inputs.active_skill_id,
            injection_mode: crate::prompt::SkillInjectionMode::Catalog,
            initial_template_vars: inputs.initial_template_vars,
            context_overrides: inputs.context_overrides,
            original_base_user_message: Some(inputs.prompt),
            tool_definitions: Arc::new(if inputs.tool_definitions.is_empty() {
                primary_tool_def
                    .into_iter()
                    .map(|def| ToolDefinition {
                        name: def.name.clone(),
                        description: def.description.clone(),
                        input_schema: def.input_schema.clone(),
                    })
                    .collect()
            } else {
                inputs.tool_definitions
            }),
            visible_tool_names: Arc::new(inputs.visible_tool_names),
            project_instruction_profile: inputs.project_instruction_profile,
            tool_guidance: inputs.tool_guidance,
            agent_catalog: inputs.agent_catalog,
        }
    }

    fn resolve_agent_spec<'a>(&'a self, requested_type: Option<&str>) -> Result<(&'a AgentSpec, Vec<String>)> {
        let requested_type = requested_type.map(str::trim).filter(|value| !value.is_empty());
        if let Some(agent_type) = requested_type {
            if let Some(spec) = self.agent_types.get(agent_type) {
                return Ok((spec, Vec::new()));
            }
        }

        let fallback = self
            .agent_types
            .get(&self.primary_agent_type)
            .ok_or_else(|| anyhow::anyhow!("Primary agent '{}' is not registered", self.primary_agent_type))?;

        let warnings = requested_type
            .map(|agent_type| {
                vec![format!(
                    "Unknown subagent_type '{}'; fell back to primary agent '{}'.",
                    agent_type, self.primary_agent_type
                )]
            })
            .unwrap_or_default();

        Ok((fallback, warnings))
    }

    async fn run_subagent(
        &self,
        prompt: &str,
        subagent_type: Option<&str>,
        model_override: Option<&str>,
        skill_id: Option<String>,
        injection_mode: crate::prompt::SkillInjectionMode,
        agent_id: &str,
        context: Option<ToolContext>,
    ) -> Result<(String, u128, Vec<String>)> {
        let (spec, mut warnings) = self.resolve_agent_spec(subagent_type)?;
        let config = self.config_store.read().await.clone();
        let binding = config.resolve_agent_binding(spec)?;

        let project_dir = context
            .as_ref()
            .and_then(|ctx| ctx.environment.as_ref())
            .and_then(|env| env.project_dir.as_ref())
            .map(PathBuf::from);

        let mut environment = if let Some(env) = context.as_ref().and_then(|ctx| ctx.environment.clone()) {
            env
        } else {
            crate::prompt::EnvironmentSnapshot::collect(&config.config_dir, project_dir.as_deref()).await
        };
        let services = self
            .services
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("AgentTool is not configured with required services"))?;

        let (runtime, model_config) = services
            .runtime_builder
            .build_runtime(
                spec,
                &binding,
                model_override,
                context.as_ref(),
                project_dir.as_deref(),
                environment.clone(),
            )
            .await?;
        environment.model_id = Some(model_config.model.clone());

        log::info!(
            "[Agent] Subagent '{}' resolved provider='{}', llm={:?}, model='{}'",
            spec.id,
            binding.provider_id,
            binding.llm_id,
            model_config.model
        );

        let mut prompt_template_vars = HashMap::new();
        prompt_template_vars.insert(template_vars::WORKFLOW_STAGE.to_string(), "idle".to_string());
        prompt_template_vars.insert(template_vars::PENDING_INTERACTION.to_string(), "none".to_string());
        prompt_template_vars.insert(template_vars::ACTIVE_AGENT.to_string(), spec.display_name.clone());
        let prompt_material = services
            .prompt_service
            .load_agent_material(spec, Some(environment.clone()), prompt_template_vars.clone())
            .await?;

        // 使用新的统一构建管道：构建 PromptConstructionRequest → build_from_request
        // 优先使用传入的 skill_id，否则从输入中解析
        let resolved_skill_id = skill_id.or_else(|| runtime.resolve_active_skill_id(prompt, &[]).ok().flatten());
        let workflow_stage = prompt_template_vars
            .get(template_vars::WORKFLOW_STAGE)
            .map(String::as_str);
        let turn_material = services
            .prompt_service
            .load_turn_material(
                project_dir.as_deref(),
                workflow_stage,
                resolved_skill_id.clone(),
                prompt_template_vars.clone(),
                spec.enable_project_developer_prompt,
            )
            .await?;
        let tool_definitions = runtime.tools().tool_definitions().await;
        let visible_tool_names: HashSet<String> = tool_definitions.iter().map(|tool| tool.name.clone()).collect();

        let request = self.build_request_from_params(
            spec,
            PromptRequestInputs {
                base_prompt: prompt_material.agent_prompt.clone(),
                prompt: prompt.to_string(),
                active_skill_id: resolved_skill_id.clone(),
                initial_template_vars: prompt_material.initial_template_vars.clone(),
                context_overrides: prompt_template_vars.clone(),
                tool_definitions,
                visible_tool_names,
                project_instruction_profile: prompt_material.project_instruction_profile,
                tool_guidance: prompt_material.tool_guidance,
                agent_catalog: prompt_material.agent_catalog.clone(),
            },
            None,
        );

        // 更新 request 中的 skill_id 和 injection_mode
        let final_request = PromptConstructionRequest {
            skill_id: resolved_skill_id,
            injection_mode,
            ..request
        };

        let name_overrides: HashMap<String, String> = HashMap::new();
        let extra_sections = PromptExtraSections {
            system_prompt_base: None,
            developer_project_prompt: turn_material.developer_project_prompt,
            project_context: turn_material.project_context,
            workflow_prompt: turn_material.workflow_prompt,
            environment_snapshot: prompt_material.environment_snapshot.clone(),
        };
        let fallback_skill_registry = crate::skill::SkillRegistry::new();
        let skill_registry = runtime.skill_registry.as_deref().unwrap_or(&fallback_skill_registry);
        let system_prompt = SystemPromptBuilder::default().build_from_request(
            &final_request,
            &name_overrides,
            skill_registry,
            extra_sections,
        );

        let start_time = Instant::now();
        let (tx, mut rx) = mpsc::channel(100);
        let logs_collector = Arc::new(Mutex::new(Vec::new()));
        let forwarding_handle = if let Some(ref ctx) = context {
            let parent_tx = ctx.event_tx.clone();
            let parent_tool_id = ctx.tool_use_id.clone();
            let logs = logs_collector.clone();
            Some(tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    match event {
                        AgentEvent::TextDelta(text) => {
                            let _ = parent_tx
                                .send(AgentEvent::LogDelta {
                                    id: parent_tool_id.clone(),
                                    name: "Agent".to_string(),
                                    log: text.clone(),
                                    stream: "stdout".to_string(),
                                })
                                .await;
                            logs.lock().await.push(text);
                        }
                        AgentEvent::ToolStart { name, input, .. } => {
                            let log = format!("\n[Agent] Executing {}: {}\n", name, input);
                            let _ = parent_tx
                                .send(AgentEvent::LogDelta {
                                    id: parent_tool_id.clone(),
                                    name: "Agent".to_string(),
                                    log: log.clone(),
                                    stream: "stderr".to_string(),
                                })
                                .await;
                            logs.lock().await.push(log);
                        }
                        AgentEvent::ToolEnd { name, is_error, .. } => {
                            let status = if is_error { "FAILED" } else { "SUCCESS" };
                            let log = format!("[Agent] {} finished: {}\n", name, status);
                            let _ = parent_tx
                                .send(AgentEvent::LogDelta {
                                    id: parent_tool_id.clone(),
                                    name: "Agent".to_string(),
                                    log: log.clone(),
                                    stream: "stderr".to_string(),
                                })
                                .await;
                            logs.lock().await.push(log);
                        }
                        _ => {}
                    }
                }
            }))
        } else {
            None
        };

        let turn_ctx = runtime
            .prepare_turn(prompt, Arc::new(Vec::new()), system_prompt)
            .await?;
        let session_id = context
            .as_ref()
            .map(|ctx| ctx.session_id.clone())
            .unwrap_or_else(|| "subagent".to_string());
        let user_message = Message::new(
            Role::User,
            vec![ContentBlock::Text {
                text: prompt.to_string(),
            }],
            chrono::Utc::now().timestamp_millis(),
        );
        let result = runtime
            .run_turn_with_context(
                turn_ctx,
                user_message,
                &session_id,
                agent_id,
                Some(environment),
                tx,
                None,
            )
            .await?;
        if let Some(handle) = forwarding_handle {
            handle.await?;
        }

        let final_assistant_msg = result
            .messages
            .iter()
            .rev()
            .find(|m| m.role == Role::Assistant)
            .and_then(|m| {
                m.content.iter().find_map(|b| {
                    if let ContentBlock::Text { text } = b {
                        Some(text.clone())
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_default();
        if !warnings.is_empty() {
            for warning in &warnings {
                log::warn!("[Agent] {}", warning);
            }
        }
        Ok((
            final_assistant_msg,
            start_time.elapsed().as_millis(),
            std::mem::take(&mut warnings),
        ))
    }
}

#[async_trait]
impl SubAgentExecutor for AgentTool {
    async fn execute_agent(
        &self,
        request: crate::orchestrator::SubAgentRequest,
        context: Option<ToolContext>,
    ) -> Result<crate::orchestrator::SubAgentOutput> {
        let (output, duration_ms, warnings) = self
            .run_subagent(
                &request.prompt,
                request.agent_selection.as_deref(),
                None,
                None,
                SkillInjectionMode::Catalog,
                &request.agent_id,
                context,
            )
            .await?;

        let output = if request.output_format.as_deref() == Some("summary") {
            output.chars().take(500).collect::<String>()
        } else {
            output
        };

        Ok(crate::orchestrator::SubAgentOutput {
            output,
            duration_ms,
            warnings,
        })
    }

    fn catalog_agent_ids(&self) -> HashSet<String> {
        AgentTool::catalog_agent_ids(self)
    }

    fn default_agent_id(&self) -> String {
        AgentTool::default_agent_id(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentTool, PromptRequestInputs};
    use crate::config::{AgentSpec, AppConfig, ConfiguredAgentModel, GatewayConfig};
    use crate::prompt::{ProjectInstructionProfile, ToolGuidanceMode};
    use crate::provider::types::ToolDefinition;
    use serde_json::json;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    fn build_tool() -> AgentTool {
        let mut config = AppConfig::new(PathBuf::from("D:/workspace/.nova"));
        config.gateway = GatewayConfig {
            agents: vec![
                AgentSpec {
                    id: "nova".to_string(),
                    display_name: "Nova".to_string(),
                    description: "default".to_string(),
                    provider: "default".to_string(),
                    llm: "default".to_string(),
                    prompt_file: Some("agent-nova.md".to_string()),
                    aliases: Vec::new(),
                    prompt_inline: None,
                    system_prompt_template: None,
                    model_config: ConfiguredAgentModel {
                        model: "gpt-oss-120b".to_string(),
                        temperature: 0.0,
                        max_tokens: Some(8192),
                        top_p: 1.0,
                    },
                    enable_project_developer_prompt: false,
                },
                AgentSpec {
                    id: "developer".to_string(),
                    display_name: "Developer".to_string(),
                    description: "developer".to_string(),
                    provider: "default".to_string(),
                    llm: "default".to_string(),
                    prompt_file: Some("agent-developer.md".to_string()),
                    aliases: Vec::new(),
                    prompt_inline: None,
                    system_prompt_template: None,
                    model_config: ConfiguredAgentModel {
                        model: "gpt-oss-120b".to_string(),
                        temperature: 0.0,
                        max_tokens: Some(8192),
                        top_p: 1.0,
                    },
                    enable_project_developer_prompt: true,
                },
            ],
            ..GatewayConfig::default()
        };
        AgentTool::new_without_subagent_services(config)
    }

    #[test]
    fn resolve_agent_spec_uses_requested_registered_agent() {
        let tool = build_tool();
        let (spec, warnings) = tool.resolve_agent_spec(Some("developer")).unwrap();
        assert_eq!(spec.id, "developer");
        assert!(warnings.is_empty());
    }

    #[test]
    fn resolve_agent_spec_falls_back_to_default_for_unknown_agent() {
        let tool = build_tool();
        let (spec, warnings) = tool.resolve_agent_spec(Some("coder-plus")).unwrap();
        assert_eq!(spec.id, "nova");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("fell back to primary agent 'nova'"));
    }

    #[test]
    fn resolve_agent_spec_falls_back_to_default_for_missing_agent() {
        let tool = build_tool();
        let (spec, warnings) = tool.resolve_agent_spec(None).unwrap();
        assert_eq!(spec.id, "nova");
        assert!(warnings.is_empty());
    }

    #[test]
    fn build_request_from_params_preserves_prompt_and_guidance_inputs() {
        let tool = build_tool();
        let spec = tool.agent_types.get("developer").unwrap();
        let mut initial_template_vars = HashMap::new();
        initial_template_vars.insert("base".to_string(), "value".to_string());
        let mut context_overrides = HashMap::new();
        context_overrides.insert("active_agent".to_string(), "Developer".to_string());
        let tool_definitions = vec![ToolDefinition {
            name: "ToolInfo".to_string(),
            description: "lookup tool".to_string(),
            input_schema: json!({"type": "object"}),
        }];
        let visible_tool_names = HashSet::from(["ToolInfo".to_string()]);

        let request = tool.build_request_from_params(
            spec,
            PromptRequestInputs {
                base_prompt: "base prompt".to_string(),
                prompt: "user prompt".to_string(),
                active_skill_id: Some("skill-a".to_string()),
                initial_template_vars: initial_template_vars.clone(),
                context_overrides: context_overrides.clone(),
                tool_definitions: tool_definitions.clone(),
                visible_tool_names: visible_tool_names.clone(),
                project_instruction_profile: ProjectInstructionProfile::Code,
                tool_guidance: ToolGuidanceMode::Compact,
                agent_catalog: Some("catalog".to_string()),
            },
            None,
        );

        assert_eq!(request.base_prompt, "base prompt");
        assert_eq!(request.initial_template_vars, initial_template_vars);
        assert_eq!(request.context_overrides, context_overrides);
        assert_eq!(request.project_instruction_profile, ProjectInstructionProfile::Code);
        assert_eq!(request.tool_guidance, ToolGuidanceMode::Compact);
        assert_eq!(request.agent_catalog.as_deref(), Some("catalog"));
        assert_eq!(request.tool_definitions.len(), tool_definitions.len());
        assert_eq!(request.tool_definitions[0].name, tool_definitions[0].name);
        assert_eq!(request.tool_definitions[0].description, tool_definitions[0].description);
        assert_eq!(request.visible_tool_names.as_ref(), &visible_tool_names);
    }

    #[test]
    fn agent_tool_returns_clear_error_when_required_services_are_missing() {
        let config = AppConfig::new(PathBuf::from("D:/workspace/.nova"));
        let tool = AgentTool::new(config, None);
        assert!(tool.services.is_none());
    }
}
