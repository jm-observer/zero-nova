use crate::config_store::{ConfigListener, ConfigStore};
use crate::descriptor_factory::{AgentDescriptorFactory, AgentMaterialInputs};
use crate::prompt_loader::{PromptLoaderConfig, PromptMaterialLoader};
use crate::skill_adapter::load_skills;
use crate::subagent_factory::{
    build_agent_prompt_service, build_reload_session_prompt_service, build_subagent_runtime_builder,
};
use anyhow::{bail, Context, Result};
use arc_swap::ArcSwap;
use async_trait::async_trait;
use nova_agent::agent::{AgentConfig, AgentRuntime, PromptDiagnosticsConfig, ToolResultCompactionConfig};
use nova_agent::agent_catalog::AgentRegistry;
use nova_agent::app::agent_workspace_service::AgentWorkspaceService;
use nova_agent::app::application::AgentApplicationImpl;
use nova_agent::app::conversation_service::{ConversationService, TurnPromptService};
use nova_agent::conversation::repository::SqliteSessionRepository;
use nova_agent::conversation::sqlite_manager::SqliteManager;
use nova_agent::conversation::{SessionCache, SessionService};
use nova_agent::loop_guard::{DuplicateReadMode, LoopGuardConfig};
use nova_agent::network::HttpClients;
use nova_agent::prompt::{
    build_agent_catalog_section, EnvironmentSnapshot, SideChannelConfig, SideChannelInjector, TrimmerConfig,
};
use nova_agent::provider::openai_compat::OpenAiCompatClient;
use nova_agent::skill::SkillRegistry;
use nova_agent::tool::builtin::register_builtin_tools_with_services;
use nova_agent::tool::builtin::task::{TaskStore, TaskStoreHandle};
use nova_agent::tool::external::register_external_tools;
use nova_agent::tool::ToolRegistry;
use nova_agent_config::AppConfig;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

struct ConfigSnapshotCacheUpdater {
    cache: Arc<ArcSwap<serde_json::Value>>,
}

pub struct BuiltAgentRuntime {
    pub runtime: AgentRuntime,
    pub agent_registry: AgentRegistry,
    pub skill_registry: Arc<SkillRegistry>,
}

pub struct AgentRuntimeBuildOptions {
    pub extra_skill_paths: Vec<std::path::PathBuf>,
    pub project_dir_service: Arc<dyn nova_agent::tool::ProjectDirService>,
}

pub async fn build_agent_runtime(config: &AppConfig, options: AgentRuntimeBuildOptions) -> Result<BuiltAgentRuntime> {
    let skill_packages = load_skills(config.skills_dir().as_path(), &options.extra_skill_paths).await?;
    let skill_registry =
        Arc::new(SkillRegistry::from_packages(skill_packages).context("Failed to initialize skill registry")?);

    let mut env_snapshot = EnvironmentSnapshot::collect(&config.config_dir, None).await;
    let root_agent = config.primary_agent()?;
    let root_binding = config.resolve_agent_binding(root_agent)?;
    env_snapshot.model_id = Some(root_binding.model_config.model.clone());

    let task_store = TaskStoreHandle::new(TaskStore::new());
    let http_clients = HttpClients::new()?;
    let tools = ToolRegistry::new();
    let agent_prompt_service = build_agent_prompt_service(Arc::new(config.clone()));
    let runtime_builder = build_subagent_runtime_builder(Arc::new(config.clone()));
    register_builtin_tools_with_services(
        &tools,
        config,
        task_store.clone(),
        skill_registry.clone(),
        options.project_dir_service,
        &http_clients,
        Some(nova_agent::tool::builtin::agent::AgentToolServices {
            prompt_service: agent_prompt_service,
            runtime_builder,
            // 该 build_agent_runtime 路径无 SessionService（CLI / 一次性独立运行），
            // 子 Agent 派生回退到老语义（不持久化）。
            conversation_writer: None,
        }),
    )
    .await;

    if let Some(tools_dir) = &config.tool.tools_dir {
        let tools_path = config.config_dir.join(tools_dir);
        register_external_tools(&tools, &tools_path).await;
    }

    let agent_config = AgentConfig {
        max_iterations: config.gateway.max_iterations,
        model_config: root_binding.model_config.clone().into(),
        tool_timeout: Duration::from_secs(config.gateway.tool_timeout_secs.unwrap_or(120)),
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
        initial_env_snapshot: Some(env_snapshot.clone()),
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

    let prompt_loader = PromptMaterialLoader::from_config(&PromptLoaderConfig::from(config));
    let descriptor_factory = AgentDescriptorFactory::new(prompt_loader);
    let mut agents = Vec::with_capacity(config.gateway.agents.len());
    let catalog_text = build_agent_catalog_section(&config.gateway.agents, &config.primary_agent()?.id);
    for agent in &config.gateway.agents {
        let binding = config.resolve_agent_binding(agent)?;
        let mut template_vars = HashMap::new();
        template_vars.insert("workflow_stage".to_string(), "idle".to_string());
        template_vars.insert("pending_interaction".to_string(), "none".to_string());
        template_vars.insert("active_agent".to_string(), agent.display_name.clone());
        agents.push(
            descriptor_factory
                .build_descriptor(
                    agent,
                    &binding,
                    AgentMaterialInputs {
                        environment_snapshot: Some(env_snapshot.clone()),
                        agent_catalog: if catalog_text.is_empty() {
                            None
                        } else {
                            Some(catalog_text.clone())
                        },
                        initial_template_vars: template_vars,
                    },
                    &skill_registry,
                )
                .await?,
        );
    }
    if agents.is_empty() {
        bail!("No agents configured");
    }

    let mut agent_registry = AgentRegistry::new(agents.remove(0));
    for agent in agents {
        agent_registry.register(agent);
    }

    let client = OpenAiCompatClient::from_registry_with_http_client(
        config.providers.clone(),
        root_binding.provider_id.clone(),
        http_clients.provider.clone(),
    );
    let mut runtime = AgentRuntime::new(client, tools, agent_config);
    runtime.task_store = Some(task_store);
    runtime.skill_registry = Some(skill_registry.clone());

    if config.gateway.side_channel.enabled {
        let side_channel = SideChannelConfig {
            enabled: config.gateway.side_channel.enabled,
            skill_reminder_interval: config.gateway.side_channel.skill_reminder_interval,
            inject_date: config.gateway.side_channel.inject_date.unwrap_or(true),
            custom_reminders: vec![],
        };
        runtime.set_side_channel_injector(SideChannelInjector::new(side_channel));
    }

    Ok(BuiltAgentRuntime {
        runtime,
        agent_registry,
        skill_registry,
    })
}

#[async_trait]
impl ConfigListener for ConfigSnapshotCacheUpdater {
    async fn on_config_changed(&self, config: Arc<AppConfig>) -> Result<()> {
        let snapshot = serde_json::to_value(&config).context("Failed to serialize config")?;
        self.cache.store(Arc::new(snapshot));
        Ok(())
    }
}

pub async fn build_application(config: AppConfig) -> Result<Arc<AgentApplicationImpl>> {
    warn_unused_gateway_sections(&config).await?;

    let skill_packages = load_skills(config.skills_dir().as_path(), &[]).await?;
    let skill_registry =
        Arc::new(SkillRegistry::from_packages(skill_packages).context("Failed to initialize skill registry")?);

    let mut env_snapshot = EnvironmentSnapshot::collect(&config.config_dir, None).await;
    let root_agent = config.primary_agent()?;
    let root_binding = config.resolve_agent_binding(root_agent)?;
    env_snapshot.model_id = Some(root_binding.model_config.model.clone());

    let task_store = TaskStoreHandle::new(TaskStore::new());
    let data_dir_path = config.data_dir_path();
    let sqlite_manager = SqliteManager::new(&data_dir_path).await?;
    let repository = SqliteSessionRepository::new(sqlite_manager.pool);
    let session_cache = Arc::new(SessionCache::new());
    let session_service = SessionService::new(session_cache, repository);
    session_service.load_session_index().await?;

    let http_clients = HttpClients::new()?;
    let config_store = ConfigStore::new(config.clone());

    let tools = ToolRegistry::new();
    let agent_prompt_service = build_agent_prompt_service(Arc::new(config.clone()));
    let runtime_builder = build_subagent_runtime_builder(Arc::new(config.clone()));
    // 克隆一份 builder 句柄给 AgentApplicationImpl。SubagentRuntimeBuilder 的
    // native deferred 种子表在 Arc 之后，克隆与下方移交给 AgentToolServices
    // 的那份共享同一张表——宿主经 app 注册的种子对子 Agent 派生路径可见。
    let subagent_runtime_builder = runtime_builder.clone();
    let conversation_writer = Arc::new(nova_agent::app::conversation_service::ConversationWriteHandle::new(
        session_service.clone(),
    ));
    let hook_slots = register_builtin_tools_with_services(
        &tools,
        &config,
        task_store.clone(),
        skill_registry.clone(),
        Arc::new(session_service.clone()),
        &http_clients,
        Some(nova_agent::tool::builtin::agent::AgentToolServices {
            prompt_service: agent_prompt_service,
            runtime_builder,
            conversation_writer: Some(conversation_writer),
        }),
    )
    .await;

    if let Some(tools_dir) = &config.tool.tools_dir {
        let tools_path = config.config_dir.join(tools_dir);
        register_external_tools(&tools, &tools_path).await;
    }

    let agent_config = AgentConfig {
        max_iterations: config.gateway.max_iterations,
        model_config: root_binding.model_config.clone().into(),
        tool_timeout: Duration::from_secs(config.gateway.tool_timeout_secs.unwrap_or(120)),
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
        initial_env_snapshot: Some(env_snapshot.clone()),
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

    let agent_registry = build_agent_registry(&config, &skill_registry).await?;
    let client = OpenAiCompatClient::from_registry_with_http_client(
        config.providers.clone(),
        root_binding.provider_id.clone(),
        http_clients.provider.clone(),
    );
    let mut runtime = AgentRuntime::new(client, tools, agent_config);
    runtime.task_store = Some(task_store);
    runtime.skill_registry = Some(skill_registry.clone());

    if config.gateway.side_channel.enabled {
        let side_channel = SideChannelConfig {
            enabled: config.gateway.side_channel.enabled,
            skill_reminder_interval: config.gateway.side_channel.skill_reminder_interval,
            inject_date: config.gateway.side_channel.inject_date.unwrap_or(true),
            custom_reminders: vec![],
        };
        runtime.set_side_channel_injector(SideChannelInjector::new(side_channel));
    }

    let config_snapshot_cache = Arc::new(ArcSwap::from_pointee(
        serde_json::to_value(&config).context("Failed to serialize config")?,
    ));
    config_store
        .add_listener(Arc::new(ConfigSnapshotCacheUpdater {
            cache: config_snapshot_cache.clone(),
        }))
        .await;
    let config_path = config.config_path();

    let turn_prompt_service = TurnPromptService::from_config(&config);
    let conversation_service = ConversationService::new(
        runtime,
        agent_registry.clone(),
        session_service.clone(),
        Arc::new(config.clone()),
        turn_prompt_service,
    );
    let workspace_service = AgentWorkspaceService::new(
        agent_registry,
        session_service,
        Arc::new(config.clone()),
        skill_registry.clone(),
        Some(Arc::new(build_reload_session_prompt_service(Arc::new(config.clone())))),
    );

    Ok(Arc::new(AgentApplicationImpl::new(
        conversation_service,
        workspace_service,
        Arc::new(config),
        config_snapshot_cache,
        config_path,
        hook_slots.orchestrate_task,
        hook_slots.skill_system,
        subagent_runtime_builder,
    )))
}

async fn build_agent_registry(config: &AppConfig, skill_registry: &Arc<SkillRegistry>) -> Result<AgentRegistry> {
    let env_snapshot = EnvironmentSnapshot::collect(&config.config_dir, None).await;
    let prompt_loader = PromptMaterialLoader::from_config(&PromptLoaderConfig::from(config));
    let descriptor_factory = AgentDescriptorFactory::new(prompt_loader);
    let mut agents = Vec::with_capacity(config.gateway.agents.len());
    let primary_agent_id = config.primary_agent()?.id.clone();
    let catalog_text = build_agent_catalog_section(&config.gateway.agents, &primary_agent_id);

    for agent in &config.gateway.agents {
        let binding = config.resolve_agent_binding(agent)?;
        let mut template_vars = HashMap::new();
        template_vars.insert("workflow_stage".to_string(), "idle".to_string());
        template_vars.insert("pending_interaction".to_string(), "none".to_string());
        template_vars.insert("active_agent".to_string(), agent.display_name.clone());
        agents.push(
            descriptor_factory
                .build_descriptor(
                    agent,
                    &binding,
                    AgentMaterialInputs {
                        environment_snapshot: Some(env_snapshot.clone()),
                        agent_catalog: if catalog_text.is_empty() {
                            None
                        } else {
                            Some(catalog_text.clone())
                        },
                        initial_template_vars: template_vars,
                    },
                    skill_registry,
                )
                .await?,
        );
    }

    if agents.is_empty() {
        bail!("No agents configured");
    }

    let mut agent_registry = AgentRegistry::new(agents.remove(0));
    for agent in agents {
        agent_registry.register(agent);
    }
    Ok(agent_registry)
}

async fn warn_unused_gateway_sections(config: &AppConfig) -> Result<()> {
    let config_path = config.config_path();
    let content = tokio::fs::read_to_string(&config_path).await.ok();
    if let Some(content) = content {
        let legacy_sections = [
            "[gateway.router]",
            "[gateway.interaction]",
            "[gateway.interaction.risk]",
            "[gateway.workflow]",
        ];
        let mut warned = false;
        for section in legacy_sections {
            if content.contains(section) {
                if !warned {
                    log::warn!(
                        "Found unimplemented gateway sections in {:?}; these sections are currently ignored.",
                        config_path
                    );
                    warned = true;
                }
                log::warn!("Ignored section: {}", section);
            }
        }
    }
    Ok(())
}
