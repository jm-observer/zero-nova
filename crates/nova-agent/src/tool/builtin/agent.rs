use crate::agent::{AgentConfig, AgentRuntime, PromptDiagnosticsConfig, ToolResultCompactionConfig};
use crate::config::{AgentSpec, AppConfig};
use crate::event::AgentEvent;
use crate::loop_guard::{DuplicateReadMode, LoopGuardConfig};
use crate::message::{ContentBlock, Message, Role};
use crate::network::HttpClients;
use crate::prompt::TrimmerConfig;
use crate::prompt::{
    load_developer_project_prompt_async, load_project_context_with_config_async, template_vars, PromptConfig,
    PromptLoadContext,
};
use crate::provider::openai_compat::OpenAiCompatClient;
use crate::provider::ModelConfig;
use crate::tool::builtin::register_builtin_tools;
use crate::tool::{ProjectDirService, RegisteredToolDefinition, Tool, ToolContext, ToolOutput, ToolRegistry};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Tool to spawn a subagent for specialized task execution.
#[derive(Clone)]
pub struct AgentTool {
    config: AppConfig,
    agent_types: HashMap<String, AgentSpec>,
    primary_agent_type: String,
}

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

impl AgentTool {
    fn selected_agent_type(input: &Value) -> Option<&str> {
        input["agent_selection"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| input["subagent_type"].as_str().filter(|value| !value.trim().is_empty()))
    }

    pub fn new(config: AppConfig) -> Self {
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
        Self {
            config,
            agent_types,
            primary_agent_type,
        }
    }

    pub fn catalog_agent_ids(&self) -> std::collections::HashSet<String> {
        self.config
            .gateway
            .agents
            .iter()
            .map(|agent| agent.id.clone())
            .collect()
    }

    pub fn default_agent_id(&self) -> String {
        self.config
            .gateway
            .agents
            .first()
            .map(|agent| agent.id.clone())
            .unwrap_or_else(|| "nova".to_string())
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
        context: Option<ToolContext>,
    ) -> Result<(String, u128, Vec<String>)> {
        let (spec, mut warnings) = self.resolve_agent_spec(subagent_type)?;
        let binding = self.config.resolve_agent_binding(spec)?;
        let client = OpenAiCompatClient::from_registry_with_http_client_and_context_headers_enabled(
            self.config.providers.clone(),
            binding.provider_id.clone(),
            crate::network::build_provider_client()?,
            self.config.outbound_context_headers.enabled,
        );
        let sub_registry = ToolRegistry::new();
        if let Some(ctx) = &context {
            if let (Some(task_store), Some(skill_registry)) = (ctx.task_store.as_ref(), ctx.skill_registry.as_ref()) {
                let http_clients = HttpClients::new()?;
                register_builtin_tools(
                    &sub_registry,
                    &self.config,
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

        let project_dir = context
            .as_ref()
            .and_then(|ctx| ctx.environment.as_ref())
            .and_then(|env| env.project_dir.as_ref())
            .map(PathBuf::from);

        let mut environment = if let Some(env) = context.as_ref().and_then(|ctx| ctx.environment.clone()) {
            env
        } else {
            crate::prompt::EnvironmentSnapshot::collect(&self.config.config_dir, project_dir.as_deref()).await
        };
        environment.model_id = Some(model_config.model.clone());

        log::info!(
            "[Agent] Subagent '{}' resolved provider='{}', llm={:?}, model='{}'",
            spec.id,
            binding.provider_id,
            binding.llm_id,
            model_config.model
        );

        let agent_config = AgentConfig {
            max_iterations: self.config.gateway.max_iterations,
            model_config,
            tool_timeout: Duration::from_secs(self.config.gateway.subagent_timeout_secs),
            max_tokens: self.config.gateway.max_tokens,
            trimmer: TrimmerConfig {
                context_window: self.config.gateway.trimmer.context_window,
                output_reserve: self.config.gateway.trimmer.output_reserve,
                min_recent_messages: self.config.gateway.trimmer.min_recent_messages,
                enable_summary: false,
            },
            config_dir: self.config.config_dir.clone(),
            prompts_dir: self.config.prompts_dir(),
            project_context_file: self.config.project_context_file(),
            initial_env_snapshot: Some(environment.clone()),
            loop_guard: LoopGuardConfig {
                enabled: self.config.gateway.loop_guard.enabled,
                max_consecutive_duplicate_tool_calls: self
                    .config
                    .gateway
                    .loop_guard
                    .max_consecutive_duplicate_tool_calls,
                max_stalled_iterations: self.config.gateway.loop_guard.max_stalled_iterations,
                duplicate_read_mode: if self.config.gateway.loop_guard.duplicate_read_mode == "warn_only" {
                    DuplicateReadMode::WarnOnly
                } else {
                    DuplicateReadMode::WarnThenReject
                },
                iteration_trim_ratio: self.config.gateway.loop_guard.iteration_trim_ratio,
            },
            prompt_diagnostics: PromptDiagnosticsConfig {
                enabled: self.config.gateway.prompt_diagnostics.enabled,
                large_section_chars: self.config.gateway.prompt_diagnostics.large_section_chars,
                large_message_chars: self.config.gateway.prompt_diagnostics.large_message_chars,
                large_tool_result_chars: self.config.gateway.prompt_diagnostics.large_tool_result_chars,
            },
            tool_result_compaction: ToolResultCompactionConfig {
                enabled: self.config.gateway.tool_result_compaction.enabled,
                max_chars: self.config.gateway.tool_result_compaction.max_chars,
                head_chars: self.config.gateway.tool_result_compaction.head_chars,
                tail_chars: self.config.gateway.tool_result_compaction.tail_chars,
                disable_for_tools: self
                    .config
                    .gateway
                    .tool_result_compaction
                    .disable_for_tools
                    .iter()
                    .map(|name| name.to_ascii_lowercase())
                    .collect(),
            },
        };
        let mut runtime = AgentRuntime::new(client, sub_registry, agent_config);
        if let Some(ctx) = &context {
            runtime.task_store = ctx.task_store.clone();
            runtime.skill_registry = ctx.skill_registry.clone();
            runtime.read_files = ctx.read_files.clone();
        }

        let mut prompt_config = PromptConfig::new(
            spec.id.clone(),
            self.load_agent_prompt(spec).await?,
            PromptLoadContext {
                project_dir: project_dir.clone(),
                project_context_path: self.config.project_context_file(),
                developer_prompt_files: Vec::new(),
            },
        )
        .with_environment(environment.clone())
        .with_workflow_prompt_path(self.config.prompts_dir().join("workflow-stages.md"));

        let mut prompt_template_vars = HashMap::new();
        prompt_template_vars.insert(template_vars::WORKFLOW_STAGE.to_string(), "idle".to_string());
        prompt_template_vars.insert(template_vars::PENDING_INTERACTION.to_string(), "none".to_string());
        prompt_template_vars.insert(template_vars::ACTIVE_AGENT.to_string(), spec.display_name.clone());
        prompt_config = prompt_config.with_template_vars(prompt_template_vars);

        let project_context_path = self.config.project_context_file();
        let project_context =
            load_project_context_with_config_async(project_dir.as_deref(), project_context_path.as_deref()).await;
        if let Some(content) = project_context {
            prompt_config = prompt_config.with_project_context_content(content);
        }

        if spec.enable_project_developer_prompt {
            let files = self.config.developer_prompt_files.clone();
            prompt_config.load_context.developer_prompt_files = files.clone();
            if let Some(content) = load_developer_project_prompt_async(project_dir.as_deref(), &files).await {
                prompt_config = prompt_config.with_developer_project_prompt_content(content);
            }
        }

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
                            let log = format!("\n[Agent] 🚀 Executing {}: {}\n", name, input);
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
                            let status = if is_error { "❌ FAILED" } else { "✅ SUCCESS" };
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
            .prepare_turn(prompt, Arc::new(Vec::new()), &prompt_config)
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
            .run_turn_with_context(turn_ctx, user_message, &session_id, None, Some(environment), tx, None)
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

    async fn load_agent_prompt(&self, spec: &AgentSpec) -> Result<String> {
        if let Some(prompt_inline) = spec.prompt_inline.as_ref().filter(|value| !value.trim().is_empty()) {
            return Ok(prompt_inline.clone());
        }

        if let Some(file) = spec.prompt_file.as_ref().filter(|value| !value.trim().is_empty()) {
            let prompt_path = self.config.prompts_dir().join(file);
            let content = tokio::fs::read_to_string(&prompt_path).await?;
            return Ok(content);
        }

        if let Some(legacy_template) = spec
            .system_prompt_template
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(legacy_template.clone());
        }

        Ok("You are a helpful assistant.".to_string())
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn definition(&self) -> RegisteredToolDefinition {
        let catalog_hint =
            crate::prompt::build_agent_catalog_hint(&self.config.gateway.agents, &self.primary_agent_type);

        RegisteredToolDefinition {
            name: "Agent".to_string(),
            description:
                "Spawn a specialized agent to perform a task. Supports multiple agent types and isolated execution."
                    .to_string()
                    + "\n\n"
                    + &catalog_hint,
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "Specific task for the agent to perform" },
                    "description": { "type": "string", "description": "3-5 word summary of what the agent is doing" },
                    "subagent_type": {
                        "type": "string",
                        "description": "DEPRECATED: Registered agent id to execute this subtask with (e.g., 'nova', 'developer'). Unknown values fall back to the primary agent."
                    },
                    "agent_selection": {
                        "type": "string",
                        "description": "Agent ID from the catalog to execute this subtask with. Takes precedence over subagent_type."
                    },
                    "run_in_background": { "type": "boolean", "default": false, "description": "Whether to run the agent in the background" },
                    "isolation": { "type": "string", "enum": ["none", "worktree"], "default": "none", "description": "Isolation mode for the agent" },
                    "model": { "type": "string", "description": "Optional model override" },
                    "agent_id": {
                        "type": "string",
                        "description": "Unique identifier for this agent within the current orchestration plan (e.g. 'agent-1'). Required in orchestration mode."
                    },
                    "parent_plan_id": {
                        "type": "string",
                        "description": "ID of the orchestration plan this agent belongs to. Required in orchestration mode."
                    },
                    "stage_id": {
                        "type": "string",
                        "description": "ID of the execution stage this agent belongs to. Required in orchestration mode."
                    },
                    "output_format": {
                        "type": "string",
                        "enum": ["full", "summary"],
                        "default": "full",
                        "description": "In 'summary' mode the agent returns a structured summary only, reducing context usage for the Review Agent."
                    }
                },
                "required": ["prompt", "description"]
            }),
            defer_loading: false,
        }
    }

    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        let prompt = input["prompt"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'prompt'"))?;
        let description = input["description"].as_str().unwrap_or("Executing task");
        let selected_agent_type = Self::selected_agent_type(&input);
        let run_in_background = input["run_in_background"].as_bool().unwrap_or(false);
        let isolation = input["isolation"].as_str().unwrap_or("none");
        let model_override = input["model"].as_str();
        let mut warnings = Vec::new();

        if isolation == "worktree" {
            warnings
                .push("worktree isolation is not implemented yet; the agent ran in the current workspace.".to_string());
        }

        log::info!(
            "[Agent] Starting {} agent: {}. Model: {:?}",
            selected_agent_type.unwrap_or("default"),
            description,
            model_override
        );

        if run_in_background {
            let Some(ctx) = context.clone() else {
                anyhow::bail!("run_in_background requires tool context");
            };
            let agent_id = input["agent_id"].as_str().unwrap_or("background-agent").to_string();
            let plan_id = input["parent_plan_id"].as_str().unwrap_or("unknown-plan").to_string();
            let stage_id = input["stage_id"].as_str().unwrap_or("unknown-stage").to_string();
            let output_format = input["output_format"].as_str().unwrap_or("full").to_string();
            let this = self.clone();
            let response_agent_id = agent_id.clone();
            let response_stage_id = stage_id.clone();
            let prompt_owned = prompt.to_string();
            let selected_agent_type_owned = selected_agent_type.map(ToString::to_string);
            let model_override_owned = model_override.map(ToString::to_string);
            let description_owned = description.to_string();
            let parent_tx = ctx.event_tx.clone();

            tokio::spawn(async move {
                let _ = parent_tx
                    .send(AgentEvent::SystemLog(format!(
                        "orchestration_progress kind=sub_agent_spawn planId={} agentId={} stageId={} subagentType={} description={}",
                        plan_id,
                        agent_id,
                        stage_id,
                        selected_agent_type_owned.as_deref().unwrap_or("default"),
                        description_owned
                    )))
                    .await;

                let run = this
                    .run_subagent(
                        &prompt_owned,
                        selected_agent_type_owned.as_deref(),
                        model_override_owned.as_deref(),
                        Some(ctx),
                    )
                    .await;

                let completion_event = match run {
                    Ok((output, _duration, run_warnings)) => {
                        let output_summary = if output_format == "summary" {
                            output.chars().take(500).collect::<String>()
                        } else {
                            output
                        };
                        if !run_warnings.is_empty() {
                            log::warn!(
                                "[Agent] Background subagent {} completed with warnings: {:?}",
                                agent_id,
                                run_warnings
                            );
                        }
                        AgentEvent::SystemLog(format!(
                            "orchestration_progress kind=sub_agent_complete planId={} agentId={} stageId={} status=success error=<none> outputSummary={}",
                            plan_id,
                            agent_id,
                            stage_id,
                            output_summary
                        ))
                    }
                    Err(err) => AgentEvent::SystemLog(format!(
                        "orchestration_progress kind=sub_agent_complete planId={} agentId={} stageId={} status=failed error={}",
                        plan_id, agent_id, stage_id, err
                    )),
                };
                let _ = parent_tx.send(completion_event).await;
            });

            return Ok(ToolOutput {
                content: serde_json::to_string_pretty(&json!({
                    "status": "started",
                    "agent_id": response_agent_id,
                    "stage_id": response_stage_id,
                }))?,
                is_error: false,
            });
        }

        let (final_assistant_msg, duration_ms, run_warnings) = self
            .run_subagent(prompt, selected_agent_type, model_override, context.clone())
            .await?;
        warnings.extend(run_warnings);

        let output_json = json!({
            "output": final_assistant_msg,
            "usage": {
                "duration_ms": duration_ms,
            },
            "warnings": warnings,
        });

        Ok(ToolOutput {
            content: serde_json::to_string_pretty(&output_json)?,
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::AgentTool;
    use crate::agent_catalog::AgentModelOverride;
    use crate::config::{AgentSpec, AppConfig, GatewayConfig};
    use serde_json::json;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

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
                    tool_whitelist: None,
                    model_config: AgentModelOverride {
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
                    tool_whitelist: None,
                    model_config: AgentModelOverride {
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
        AgentTool::new(config)
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
    fn selected_agent_type_prefers_agent_selection() {
        let input = json!({
            "agent_selection": "feature-developer",
            "subagent_type": "nova"
        });
        assert_eq!(AgentTool::selected_agent_type(&input), Some("feature-developer"));
    }

    #[test]
    fn selected_agent_type_falls_back_to_subagent_type() {
        let input = json!({
            "subagent_type": "developer"
        });
        assert_eq!(AgentTool::selected_agent_type(&input), Some("developer"));
    }

    #[tokio::test]
    async fn load_agent_prompt_prefers_prompt_inline() {
        let mut tool = build_tool();
        tool.config.gateway.agents[0].prompt_inline = Some("inline prompt".to_string());
        tool.config.gateway.agents[0].prompt_file = Some("ignored.md".to_string());

        let prompt = tool.load_agent_prompt(&tool.config.gateway.agents[0]).await.unwrap();
        assert_eq!(prompt, "inline prompt");
    }

    #[tokio::test]
    async fn load_agent_prompt_reads_prompt_file() {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let temp_root = std::env::temp_dir().join(format!("zero-nova-agent-tool-{}", nanos));
        let prompts_dir = temp_root.join("prompts");
        std::fs::create_dir_all(&prompts_dir).unwrap();
        std::fs::write(prompts_dir.join("agent.md"), "file prompt").unwrap();

        let mut config = AppConfig::new(temp_root.clone());
        config.tool.prompts_dir = Some("prompts".to_string());
        config.gateway = GatewayConfig {
            agents: vec![AgentSpec {
                id: "nova".to_string(),
                display_name: "Nova".to_string(),
                description: "default".to_string(),
                provider: "default".to_string(),
                llm: "default".to_string(),
                prompt_file: Some("agent.md".to_string()),
                aliases: Vec::new(),
                prompt_inline: None,
                system_prompt_template: None,
                tool_whitelist: None,
                model_config: AgentModelOverride {
                    model: "gpt-oss-120b".to_string(),
                    temperature: 0.0,
                    max_tokens: Some(8192),
                    top_p: 1.0,
                },
                enable_project_developer_prompt: false,
            }],
            ..GatewayConfig::default()
        };
        let tool = AgentTool::new(config);

        let prompt = tool.load_agent_prompt(&tool.config.gateway.agents[0]).await.unwrap();
        assert_eq!(prompt, "file prompt");

        std::fs::remove_dir_all(temp_root).unwrap();
    }
}
