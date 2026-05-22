use crate::agent::AgentRuntime;
use crate::app::conversation_service::ConversationWriteHandle;
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
use tokio_util::sync::CancellationToken;

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

/// 宿主通过 Rust API 显式声明、需在每个 sub-agent registry 都注册一遍的
/// native deferred 工具。
///
/// `factory` 用 `Arc` 而非 `Box`，以便在多次 `build_runtime` 之间复用同一
/// 闭包（`Box<dyn Fn>` 不可 Clone）。每次 `build_runtime` 注册时把 `Arc`
/// 再包一层 `Box` 透出，以匹配 `ToolRegistry::register_deferred` 的现有签名。
#[derive(Clone)]
pub struct NativeDeferredToolSeed {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub factory: Arc<dyn Fn() -> Arc<dyn crate::tool::Tool> + Send + Sync>,
}

/// Concrete subagent runtime builder service.
#[derive(Clone)]
pub struct SubagentRuntimeBuilder {
    config: Arc<AppConfig>,
    /// 宿主通过 `register_native_deferred_seed` 推入的种子。`Arc<RwLock<_>>`
    /// 使 builder 保持 `Clone`（被 `AgentToolServices` 持有需要），且任一
    /// 克隆上的注册对所有克隆可见——`AgentApplicationImpl` 与 `AgentTool`
    /// 各持一个克隆，共享同一份种子表。
    native_deferred_seeds: Arc<RwLock<Vec<NativeDeferredToolSeed>>>,
}

impl SubagentRuntimeBuilder {
    pub fn new(config: Arc<AppConfig>) -> Self {
        Self {
            config,
            native_deferred_seeds: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 注册一个 native deferred 工具种子。此后每次 `build_runtime` 派生的
    /// sub-agent registry 都会注册该工具（注册为 deferred，与 builtin /
    /// tools.d 同层级），使命中该工具的 skill `preload` 能 `resolve_deferred`。
    pub async fn register_native_deferred_seed(&self, seed: NativeDeferredToolSeed) {
        self.native_deferred_seeds.write().await.push(seed);
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

        let client = OpenAiCompatClient::from_registry_with_http_client(
            config.providers.clone(),
            binding.provider_id.clone(),
            crate::network::build_provider_client()?,
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

        // 子 Agent 的 sub_registry 默认只有 builtin；外部 tools.d 工具仅注册进主
        // registry（nova-agent-loader/bootstrap）。这里按同源逻辑把 tools.d 也注册进
        // sub_registry（注册为 deferred），使 skill 的 preload 能对其 resolve_deferred。
        if let Some(tools_dir) = &config.tool.tools_dir {
            let tools_path = config.config_dir.join(tools_dir);
            crate::tool::external::register_external_tools(&sub_registry, &tools_path).await;
        }

        // native deferred 种子：宿主通过 `register_native_deferred_seed` 显式
        // 声明的 Rust 工具。主 app 的 `register_deferred_tool` 只作用于主 Agent
        // registry，不会传播到这里新建的 `sub_registry`——种子表是把同一组
        // 工具补注册进每个 sub-agent registry 的唯一通道。
        {
            let seeds = self.native_deferred_seeds.read().await;
            apply_native_deferred_seeds(&sub_registry, &seeds).await;
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

/// 把一组 native deferred 种子注册进给定 registry（注册为 deferred 工具）。
///
/// 抽成独立函数：`build_runtime` 调用它，单测也可绕开整套 runtime 脚手架
/// 直接对一个空 `ToolRegistry` 验证种子注册行为。
async fn apply_native_deferred_seeds(registry: &crate::tool::ToolRegistry, seeds: &[NativeDeferredToolSeed]) {
    for seed in seeds {
        // `Arc::clone` 廉价；再包一层 `Box` 以匹配 `register_deferred` 现有签名。
        let factory = seed.factory.clone();
        registry
            .register_deferred(
                seed.name.clone(),
                seed.description.clone(),
                seed.input_schema.clone(),
                Box::new(move || factory()),
            )
            .await;
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
    /// 子 Agent 派生路径的会话持久化句柄；CLI / 一次性独立调用路径为 None，仅 Gateway 路径填值。
    /// None 视为 fallback 老语义（子 turn messages 不持久化）。
    pub conversation_writer: Option<Arc<ConversationWriteHandle>>,
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

/// `run_subagent` 入参打包（避免过多位置参数）。
struct SubagentRunParams<'a> {
    prompt: &'a str,
    subagent_type: Option<&'a str>,
    model_override: Option<&'a str>,
    skill_id: Option<String>,
    injection_mode: crate::prompt::SkillInjectionMode,
    agent_id: &'a str,
    /// 委派 skill slug：命中即用其 instructions 作为完整 system prompt 并预激活 preload。
    skill_slug: Option<&'a str>,
    /// 完整 system prompt 覆盖（优先于 skill 推导）。
    system_prompt_override: Option<String>,
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
        params: SubagentRunParams<'_>,
        context: Option<ToolContext>,
    ) -> Result<(String, u128, Vec<String>)> {
        let SubagentRunParams {
            prompt,
            subagent_type,
            model_override,
            skill_id,
            injection_mode,
            agent_id,
            skill_slug,
            system_prompt_override,
        } = params;
        let (spec, mut warnings) = self.resolve_agent_spec(subagent_type)?;

        // skill 委派短路：命中 skill 则其 instructions 作为子 Agent 完整 system prompt，
        // 并收集 preload 工具；未命中即报错、不回退、不跑 turn。
        let mut sys_override = system_prompt_override;
        let mut preload_tools: Vec<String> = Vec::new();
        if let Some(slug) = skill_slug {
            match context
                .as_ref()
                .and_then(|ctx| ctx.skill_registry.as_ref())
                .and_then(|registry| registry.find_by_slug(slug).cloned())
            {
                Some(pkg) => {
                    if sys_override.is_none() {
                        sys_override = Some(pkg.instructions.clone());
                    }
                    preload_tools = pkg.preload.clone();
                }
                None => {
                    log::warn!("[Agent] skill '{}' not found in registry; aborting subagent", slug);
                    return Ok((String::new(), 0, vec![format!("skill '{}' not found", slug)]));
                }
            }
        }
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

        let system_prompt = if let Some(system_prompt_override) = sys_override {
            system_prompt_override
        } else {
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
            SystemPromptBuilder::default().build_from_request(
                &final_request,
                &name_overrides,
                skill_registry,
                extra_sections,
            )
        };

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

        // ============================================================
        // 子 Session 派生（Plan 2）—— 判定逻辑见 `decide_child_session_path`
        // ============================================================
        let parent_context = decide_child_session_path(services.conversation_writer.as_ref(), context.as_ref())?;
        let child_session_info = if let Some((writer, parent_session_id, parent_tool_use_id)) = parent_context {
            let child_id = writer
                .create_child_session(&parent_session_id, &parent_tool_use_id, agent_id, None)
                .await?;
            Some((writer, child_id))
        } else {
            None
        };

        let session_id = match &child_session_info {
            Some((_, child_id)) => child_id.clone(),
            None => context
                .as_ref()
                .map(|ctx| ctx.session_id.clone())
                .unwrap_or_else(|| "subagent".to_string()),
        };

        // 预激活该 skill 声明的 deferred 工具（注意：用 session_id，可能是 child_session_id 或 fallback id）。
        for tool_name in &preload_tools {
            if !runtime.tools().resolve_deferred(&session_id, tool_name).await {
                log::warn!(
                    "[Agent] preload tool '{}' not resolvable in subagent registry",
                    tool_name
                );
            }
        }

        // CancellationToken 父子链：父被取消时子也跟着取消。父若无 token 则子独立创建。
        let child_cancel = context
            .as_ref()
            .and_then(|ctx| ctx.cancellation_token.clone())
            .map(|parent_tok| parent_tok.child_token())
            .or_else(|| Some(CancellationToken::new()));

        let turn_ctx = runtime
            .prepare_turn(prompt, Arc::new(Vec::new()), system_prompt, &session_id)
            .await?;
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
                child_cancel,
            )
            .await?;
        if let Some(handle) = forwarding_handle {
            handle.await?;
        }

        // 子 turn 完成后持久化 messages 到子 Session.history（含 ProviderHttpTrace）。
        if let Some((writer, child_id)) = &child_session_info {
            if let Err(err) = writer.persist_subagent_turn(child_id, &result).await {
                log::warn!("[Agent] persist_subagent_turn failed for child {}: {}", child_id, err);
            }
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
                SubagentRunParams {
                    prompt: &request.prompt,
                    subagent_type: request.agent_selection.as_deref(),
                    model_override: None,
                    skill_id: None,
                    injection_mode: SkillInjectionMode::Catalog,
                    agent_id: &request.agent_id,
                    skill_slug: request.skill.as_deref(),
                    system_prompt_override: request.system_prompt_override.clone(),
                },
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

/// 判定 `run_subagent` 是否应该走子 Session 派生路径。
///
/// 见设计稿「已收敛的待澄清点」#2：
/// - `writer == None`（CLI / 独立调用）→ `Ok(None)` fallback；
/// - `writer == Some` + `context == None` → `Ok(None)` fallback；
/// - `writer == Some` + `context == Some` 但 `tool_use_id` 为空 → `Err`（上游 bug）；
/// - 其它（正常路径）→ `Ok(Some((writer, parent_session_id, parent_tool_use_id)))`。
fn decide_child_session_path(
    writer: Option<&Arc<ConversationWriteHandle>>,
    context: Option<&ToolContext>,
) -> Result<Option<(Arc<ConversationWriteHandle>, String, String)>> {
    match (writer, context) {
        (Some(_), Some(ctx)) if ctx.tool_use_id.is_empty() => {
            anyhow::bail!("[Agent] ToolContext present but tool_use_id is empty (upstream bug)")
        }
        (Some(writer), Some(ctx)) => Ok(Some((writer.clone(), ctx.session_id.clone(), ctx.tool_use_id.clone()))),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_native_deferred_seeds, AgentTool, NativeDeferredToolSeed, PromptRequestInputs, SubagentRuntimeBuilder,
    };
    use crate::config::{AgentSpec, AppConfig, ConfiguredAgentModel, GatewayConfig};
    use crate::prompt::{ProjectInstructionProfile, ToolGuidanceMode};
    use crate::provider::types::ToolDefinition;
    use crate::tool::{RegisteredToolDefinition, Tool, ToolContext, ToolOutput, ToolRegistry};
    use anyhow::Result;
    use serde_json::json;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

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

    // --- native deferred 种子机制 -----------------------------------------

    /// 计数 factory 调用次数的 mock Tool。
    struct MockSeedTool {
        name: String,
    }

    #[async_trait::async_trait]
    impl Tool for MockSeedTool {
        fn definition(&self) -> RegisteredToolDefinition {
            RegisteredToolDefinition {
                name: self.name.clone(),
                description: format!("{} mock", self.name),
                input_schema: json!({"type": "object"}),
                defer_loading: true,
            }
        }

        async fn execute(&self, _input: serde_json::Value, _context: Option<ToolContext>) -> Result<ToolOutput> {
            Ok(ToolOutput {
                content: self.name.clone(),
                is_error: false,
            })
        }
    }

    fn seed_with_counter(name: &str, calls: Arc<AtomicUsize>) -> NativeDeferredToolSeed {
        let tool_name = name.to_string();
        NativeDeferredToolSeed {
            name: name.to_string(),
            description: format!("{name} seed"),
            input_schema: json!({"type": "object"}),
            factory: Arc::new(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Arc::new(MockSeedTool {
                    name: tool_name.clone(),
                }) as Arc<dyn Tool>
            }),
        }
    }

    #[tokio::test]
    async fn register_native_deferred_seed_appends_to_builder_table() {
        let builder = SubagentRuntimeBuilder::new(Arc::new(AppConfig::new(PathBuf::from("D:/workspace/.nova"))));
        assert_eq!(builder.native_deferred_seeds.read().await.len(), 0);

        builder
            .register_native_deferred_seed(seed_with_counter("session_flag", Arc::new(AtomicUsize::new(0))))
            .await;

        let seeds = builder.native_deferred_seeds.read().await;
        assert_eq!(seeds.len(), 1);
        assert_eq!(seeds[0].name, "session_flag");
    }

    #[tokio::test]
    async fn apply_native_deferred_seeds_makes_tool_resolvable() {
        let registry = ToolRegistry::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let seeds = vec![seed_with_counter("session_flag", calls.clone())];

        apply_native_deferred_seeds(&registry, &seeds).await;

        // 注册阶段不实例化 factory（deferred 语义）。
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        // 子 Agent 命中 preload 时 resolve_deferred 应成功，并实例化 factory 一次。
        assert!(registry.resolve_deferred("sub-session", "session_flag").await);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn apply_empty_native_deferred_seeds_leaves_registry_unchanged() {
        let registry = ToolRegistry::new();
        apply_native_deferred_seeds(&registry, &[]).await;
        assert!(!registry.resolve_deferred("sub-session", "session_flag").await);
    }

    #[tokio::test]
    async fn multiple_native_deferred_seeds_all_resolvable() {
        let registry = ToolRegistry::new();
        let seeds = vec![
            seed_with_counter("session_flag", Arc::new(AtomicUsize::new(0))),
            seed_with_counter("evolution_propose", Arc::new(AtomicUsize::new(0))),
        ];

        apply_native_deferred_seeds(&registry, &seeds).await;

        assert!(registry.resolve_deferred("sub-session", "session_flag").await);
        assert!(registry.resolve_deferred("sub-session", "evolution_propose").await);
        assert!(!registry.resolve_deferred("sub-session", "not_a_seed").await);
    }
}
