use super::application::{AgentApplication, AgentApplicationImpl};
use super::conversation_service::ConversationService;
use super::prompt_loader::PromptMaterialLoader;
use super::skill_adapter::convert_loaded_skills;
use crate::agent::{AgentConfig, AgentRuntime, PromptDiagnosticsConfig, ToolResultCompactionConfig};
use crate::agent_catalog::{AgentDescriptor, AgentRegistry};
use crate::config::AppConfig;
use crate::conversation::repository::SqliteSessionRepository;
use crate::conversation::sqlite_manager::SqliteManager;
use crate::conversation::{SessionCache, SessionService};
use crate::loop_guard::{DuplicateReadMode, LoopGuardConfig};
use crate::network::HttpClients;
use crate::prompt::{
    build_agent_catalog_section, EnvironmentSnapshot, SideChannelConfig, SideChannelInjector, SystemPromptBuilder,
    TrimmerConfig, TurnPromptMaterial,
};
use crate::provider::openai_compat::OpenAiCompatClient;
use crate::skill::SkillRegistry;
use crate::tool::builtin::register_builtin_tools;
use crate::tool::builtin::task::{TaskStore, TaskStoreHandle};
use crate::tool::ToolRegistry;
use anyhow::{bail, Context, Result};
use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

pub struct BootstrapOptions {
    pub bind_addr: SocketAddr,
}

pub async fn build_application(config: AppConfig) -> Result<Arc<dyn AgentApplication>> {
    warn_unused_gateway_sections(&config).await?;

    let skill_dir = config.skills_dir();
    let loaded_skills = match nova_skill_loader::load_skills_from_dir_async(&skill_dir).await {
        Ok(skills) => {
            log::info!("Loaded {} skills from {:?}", skills.len(), skill_dir);
            skills
        }
        Err(err) => {
            log::warn!("Failed to load skills from {:?}: {}", skill_dir, err);
            Vec::new()
        }
    };
    let skill_packages = convert_loaded_skills(loaded_skills);
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

    let tools = ToolRegistry::new();
    register_builtin_tools(
        &tools,
        &config,
        task_store.clone(),
        skill_registry.clone(),
        None,
        Arc::new(session_service.clone()),
        &http_clients,
    );

    let agent_config = AgentConfig {
        max_iterations: config.gateway.max_iterations,
        model_config: root_binding.model_config.clone(),
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

    let prompt_loader = PromptMaterialLoader::from_config(&config);
    let mut agents = Vec::with_capacity(config.gateway.agents.len());
    let catalog_text = build_agent_catalog_section(&config.gateway.agents, &config.primary_agent()?.id);
    for agent in &config.gateway.agents {
        let binding = config.resolve_agent_binding(agent)?;

        let mut template_vars = HashMap::new();
        template_vars.insert("workflow_stage".to_string(), "idle".to_string());
        template_vars.insert("pending_interaction".to_string(), "none".to_string());
        template_vars.insert("active_agent".to_string(), agent.display_name.clone());

        let prompt_material = prompt_loader
            .load_agent_material(
                agent,
                Some(env_snapshot.clone()),
                if catalog_text.is_empty() {
                    None
                } else {
                    Some(catalog_text.clone())
                },
                template_vars.clone(),
            )
            .await?;

        let full_system_prompt =
            SystemPromptBuilder::from_material(&prompt_material, &TurnPromptMaterial::default(), &skill_registry)
                .build();

        agents.push(AgentDescriptor {
            id: agent.id.clone(),
            display_name: agent.display_name.clone(),
            description: agent.description.clone(),
            aliases: agent.aliases.clone(),
            system_prompt_template: full_system_prompt,
            system_prompt_base: prompt_material.agent_prompt.clone(),
            initial_template_vars: template_vars,
            tool_whitelist: agent.tool_whitelist.clone(),
            model_config: Some(agent.model_config.clone()),
            provider_id: binding.provider_id.clone(),
            llm_id: binding
                .llm_id
                .clone()
                .expect("configured agent binding must always resolve to a concrete llm"),
            enable_project_developer_prompt: agent.enable_project_developer_prompt,
        });
        log::info!(
            "Bootstrapped agent '{}' with provider='{}', llm={:?}, model='{}'",
            agent.id,
            binding.provider_id,
            binding.llm_id,
            binding.model_config.model
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
    let mut agent = AgentRuntime::new(client, tools, agent_config);
    agent.task_store = Some(task_store);
    agent.skill_registry = Some(skill_registry.clone());

    if config.gateway.side_channel.enabled {
        let side_channel = SideChannelConfig {
            enabled: config.gateway.side_channel.enabled,
            skill_reminder_interval: config.gateway.side_channel.skill_reminder_interval,
            inject_date: config.gateway.side_channel.inject_date.unwrap_or(true),
            custom_reminders: vec![],
        };
        agent.set_side_channel_injector(SideChannelInjector::new(side_channel));
    }

    let config_arc = Arc::new(RwLock::new(config.clone()));
    let config_snapshot_cache = Arc::new(ArcSwap::from_pointee(
        serde_json::to_value(&config).context("Failed to serialize config")?,
    ));
    let config_path = config.config_path();

    let conversation_service =
        ConversationService::new(agent, agent_registry.clone(), session_service.clone(), config.clone());
    let workspace_service = super::agent_workspace_service::AgentWorkspaceService::new(
        agent_registry,
        session_service,
        config_arc.clone(),
        skill_registry.clone(),
    );
    // let voice_service = build_voice_service(&config);

    Ok(Arc::new(AgentApplicationImpl::new(
        conversation_service,
        workspace_service,
        config_arc,
        config_snapshot_cache,
        config_path,
    )))
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
