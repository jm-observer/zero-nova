use crate::agent_registry_store::AgentRegistryStore;
use crate::config_store::{ConfigListener, ConfigStore};
use crate::descriptor_factory::{AgentDescriptorFactory, AgentMaterialInputs};
use crate::prompt_loader::{PromptLoaderConfig, PromptMaterialLoader};
use crate::skill_adapter::load_skills;
use crate::subagent_factory::LoaderSubagentRuntimeFactory;
use anyhow::{bail, Context, Result};
use arc_swap::ArcSwap;
use async_trait::async_trait;
use nova_agent::agent::{AgentConfig, AgentRuntime, PromptDiagnosticsConfig, ToolResultCompactionConfig};
use nova_agent::agent_catalog::AgentRegistry;
use nova_agent::app::agent_workspace_service::{AgentWorkspaceService, ReloadedSessionPrompt, SessionPromptReloader};
use nova_agent::app::application::{AgentApplication, AgentApplicationImpl};
use nova_agent::app::conversation_service::{ConversationService, TurnPromptMaterialLoader};
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
use nova_agent::tool::builtin::agent::{AgentPromptLoader, AgentToolServices};
use nova_agent::tool::builtin::task::{TaskStore, TaskStoreHandle};
use nova_agent::tool::builtin::{register_builtin_tools_with_services, BuiltinToolWiring};
use nova_agent::tool::ToolRegistry;
use nova_agent_config::{AgentSpec, AppConfig};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

struct ConfigBackedSessionPromptReloader {
    config_store: Arc<ConfigStore>,
    agent_registry_store: AgentRegistryStore,
    skill_registry: Arc<SkillRegistry>,
}

struct ConfigBackedTurnPromptMaterialLoader {
    config_store: ConfigStore,
}

struct ConfigBackedAgentPromptLoader {
    config_store: ConfigStore,
}

struct ConfigSnapshotCacheUpdater {
    cache: Arc<ArcSwap<serde_json::Value>>,
}

pub struct BuiltAgentRuntime {
    pub runtime: AgentRuntime<OpenAiCompatClient>,
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
    let wiring = BuiltinToolWiring {
        services: Some(AgentToolServices {
            prompt_loader: Arc::new(ConfigBackedAgentPromptLoader {
                config_store: ConfigStore::new(config.clone()),
            }),
            runtime_builder: Arc::new(LoaderSubagentRuntimeFactory::new(Arc::new(ConfigStore::new(
                config.clone(),
            )))),
        }),
    };
    register_builtin_tools_with_services(
        &tools,
        config,
        task_store.clone(),
        skill_registry.clone(),
        None,
        options.project_dir_service,
        &http_clients,
        wiring,
    );

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

    let client = OpenAiCompatClient::from_registry_with_http_client_and_context_headers_enabled(
        config.providers.clone(),
        root_binding.provider_id.clone(),
        http_clients.provider.clone(),
        config.outbound_context_headers.enabled,
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
impl AgentPromptLoader for ConfigBackedAgentPromptLoader {
    async fn load_agent_material(
        &self,
        spec: &AgentSpec,
        env: Option<nova_agent::prompt::EnvironmentSnapshot>,
        template_vars: HashMap<String, String>,
    ) -> Result<nova_agent::prompt::PromptMaterial> {
        let config = self.config_store.current().await;
        let loader = PromptMaterialLoader::from_config(&PromptLoaderConfig::from(&*config));
        loader.load_agent_material(spec, env, None, template_vars).await
    }

    async fn load_turn_material(
        &self,
        project_dir: Option<&Path>,
        workflow_stage: Option<&str>,
        active_skill: Option<String>,
        turn_vars: HashMap<String, String>,
        enable_developer_prompt: bool,
    ) -> Result<nova_agent::prompt::TurnPromptMaterial> {
        let config = self.config_store.current().await;
        let loader = PromptMaterialLoader::from_config(&PromptLoaderConfig::from(&*config));
        loader
            .load_turn_material(
                project_dir,
                workflow_stage,
                active_skill,
                turn_vars,
                enable_developer_prompt,
            )
            .await
    }
}

#[async_trait]
impl TurnPromptMaterialLoader for ConfigBackedTurnPromptMaterialLoader {
    async fn load_turn_material(
        &self,
        project_dir: Option<&Path>,
        workflow_stage: Option<&str>,
        active_skill: Option<String>,
        turn_vars: HashMap<String, String>,
        enable_developer_prompt: bool,
    ) -> Result<nova_agent::prompt::TurnPromptMaterial> {
        let config = self.config_store.current().await;
        let loader = PromptMaterialLoader::from_config(&PromptLoaderConfig::from(&*config));
        loader
            .load_turn_material(
                project_dir,
                workflow_stage,
                active_skill,
                turn_vars,
                enable_developer_prompt,
            )
            .await
    }
}

#[async_trait]
impl ConfigListener for ConfigSnapshotCacheUpdater {
    async fn on_config_changed(&self, config: Arc<AppConfig>) -> Result<()> {
        let snapshot = serde_json::to_value(&config).context("Failed to serialize config")?;
        self.cache.store(Arc::new(snapshot));
        Ok(())
    }
}

#[async_trait]
impl SessionPromptReloader for ConfigBackedSessionPromptReloader {
    async fn reload_session_prompt(
        &self,
        _session_id: &str,
        agent_id: &str,
        initial_template_vars: &HashMap<String, String>,
        project_dir: Option<&Path>,
    ) -> Result<ReloadedSessionPrompt> {
        let reloaded_config = self.config_store.reload_from_disk().await?;
        let next_registry = build_agent_registry(&reloaded_config, &self.skill_registry).await?;
        self.agent_registry_store.replace(next_registry);
        let agent_spec = reloaded_config
            .gateway
            .agents
            .iter()
            .find(|agent| agent.id == agent_id)
            .cloned()
            .with_context(|| format!("Agent '{}' missing in config", agent_id))?;
        let prompt_loader = PromptMaterialLoader::from_config(&PromptLoaderConfig::from(&reloaded_config));
        let prompt_base = prompt_loader.load_agent_prompt(&agent_spec).await?;
        let env = nova_agent::prompt::EnvironmentSnapshot::collect(&reloaded_config.config_dir, None).await;
        let turn_material = prompt_loader
            .load_turn_material(
                project_dir,
                Some("idle"),
                None,
                HashMap::new(),
                agent_spec.enable_project_developer_prompt,
            )
            .await?;

        let prompt_material = nova_agent::prompt::PromptMaterial {
            agent_id: agent_id.to_string(),
            agent_prompt: prompt_base.clone(),
            agent_catalog: None,
            environment_snapshot: Some(env),
            initial_template_vars: initial_template_vars.clone(),
            skill_injection_mode: nova_agent::prompt::SkillInjectionMode::Catalog,
            project_instruction_profile: nova_agent::prompt::ProjectInstructionProfile::Auto,
            tool_guidance: nova_agent::prompt::ToolGuidanceMode::Compact,
        };
        let compiled_prompt = nova_agent::prompt::SystemPromptBuilder::from_material(
            &prompt_material,
            &turn_material,
            &self.skill_registry,
        )
        .build();
        let prompt_version = fingerprint_text(&compiled_prompt);
        let source_revision = source_revision(&reloaded_config).await;

        Ok(ReloadedSessionPrompt {
            prompt_base,
            prompt_version,
            source_revision,
        })
    }
}

pub async fn build_application(config: AppConfig) -> Result<Arc<dyn AgentApplication>> {
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
    let config_snapshot: Arc<dyn nova_agent::app::ConfigSnapshot> = Arc::new(config_store.clone());

    let tools = ToolRegistry::new();
    let wiring = BuiltinToolWiring {
        services: Some(AgentToolServices {
            prompt_loader: Arc::new(ConfigBackedAgentPromptLoader {
                config_store: config_store.clone(),
            }),
            runtime_builder: Arc::new(LoaderSubagentRuntimeFactory::new(config_snapshot.clone())),
        }),
    };
    register_builtin_tools_with_services(
        &tools,
        &config,
        task_store.clone(),
        skill_registry.clone(),
        None,
        Arc::new(session_service.clone()),
        &http_clients,
        wiring,
    );

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
    let agent_registry_store = AgentRegistryStore::new(agent_registry.clone());

    let client = OpenAiCompatClient::from_registry_with_http_client_and_context_headers_enabled(
        config.providers.clone(),
        root_binding.provider_id.clone(),
        http_clients.provider.clone(),
        config.outbound_context_headers.enabled,
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

    let agent_registry_snapshot: Arc<dyn nova_agent::app::AgentRegistrySnapshot> =
        Arc::new(agent_registry_store.clone());
    let conversation_service = ConversationService::new_with_registry_snapshot(
        runtime,
        agent_registry_snapshot.clone(),
        session_service.clone(),
        config_snapshot.clone(),
        Arc::new(ConfigBackedTurnPromptMaterialLoader {
            config_store: config_store.clone(),
        }),
    );
    let workspace_service = AgentWorkspaceService::new_with_registry_snapshot(
        agent_registry_snapshot,
        session_service,
        config_snapshot.clone(),
        skill_registry.clone(),
        Some(Arc::new(ConfigBackedSessionPromptReloader {
            config_store: Arc::new(config_store.clone()),
            agent_registry_store,
            skill_registry,
        })),
    );

    Ok(Arc::new(AgentApplicationImpl::new(
        conversation_service,
        workspace_service,
        config_snapshot,
        config_snapshot_cache,
        config_path,
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

fn fingerprint_text(value: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

async fn source_revision(config: &AppConfig) -> String {
    let path = config.config_path();
    match tokio::fs::metadata(&path).await {
        Ok(meta) => {
            let modified = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis())
                .unwrap_or_default();
            format!("mtime:{}:len:{}", modified, meta.len())
        }
        Err(_) => "unknown".to_string(),
    }
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
