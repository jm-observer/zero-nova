use super::application::{AgentApplication, AgentApplicationImpl};
use super::conversation_service::ConversationService;
use crate::agent::{AgentConfig, AgentRuntime};
use crate::agent_catalog::{AgentDescriptor, AgentRegistry};
use crate::config::AppConfig;
use crate::conversation::repository::SqliteSessionRepository;
use crate::conversation::sqlite_manager::SqliteManager;
use crate::conversation::{SessionCache, SessionService};
use crate::prompt::{
    EnvironmentSnapshot, PromptConfig, SideChannelConfig, SideChannelInjector, SystemPromptBuilder, TrimmerConfig,
};
use crate::provider::openai_compat::OpenAiCompatClient;
use crate::skill::SkillRegistry;
use crate::tool::builtin::register_builtin_tools;
use crate::tool::builtin::task::TaskStore;
use crate::tool::ToolRegistry;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::Mutex;

pub struct BootstrapOptions {
    pub bind_addr: SocketAddr,
}

pub async fn build_application(config: AppConfig) -> Result<Arc<dyn AgentApplication>> {
    warn_unused_gateway_sections(&config)?;

    let mut skill_registry = SkillRegistry::new();
    let skill_dir = config.skills_dir();
    if let Err(err) = skill_registry.load_from_dir(&skill_dir) {
        log::warn!("Failed to load skills from {:?}: {}", skill_dir, err);
    }
    let skill_registry = Arc::new(skill_registry);

    let mut env_snapshot = EnvironmentSnapshot::collect(&config.config_dir, None).await;
    let root_agent = config.primary_agent()?;
    let root_binding = config.resolve_agent_binding(root_agent)?;
    env_snapshot.model_id = Some(root_binding.model_config.model.clone());

    let task_store = Arc::new(Mutex::new(TaskStore::new()));
    let data_dir_path = config.data_dir_path();
    let sqlite_manager = SqliteManager::new(&data_dir_path).await?;
    let repository = SqliteSessionRepository::new(sqlite_manager.pool);
    let session_cache = Arc::new(SessionCache::new());
    let session_service = SessionService::new(session_cache, repository);
    session_service.load_all().await?;

    let tools = ToolRegistry::new();
    register_builtin_tools(
        &tools,
        &config,
        task_store.clone(),
        skill_registry.clone(),
        None,
        Arc::new(session_service.clone()),
    );

    let agent_config = AgentConfig {
        max_iterations: config.gateway.max_iterations,
        model_config: root_binding.model_config.clone(),
        tool_timeout: Duration::from_secs(config.gateway.tool_timeout_secs.unwrap_or(120)),
        max_tokens: config.gateway.max_tokens,
        use_turn_context: config.gateway.use_turn_context,
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
    };

    let mut agents = Vec::with_capacity(config.gateway.agents.len());
    for agent in &config.gateway.agents {
        let binding = config.resolve_agent_binding(agent)?;
        let agent_prompt = load_agent_prompt(agent, &config).await?;

        let mut template_vars = HashMap::new();
        template_vars.insert("workflow_stage".to_string(), "idle".to_string());
        template_vars.insert("pending_interaction".to_string(), "none".to_string());
        template_vars.insert("active_agent".to_string(), agent.display_name.clone());

        let prompt_config = PromptConfig::new(agent.id.clone(), agent_prompt.clone(), None)
            .with_environment(env_snapshot.clone())
            .with_project_context_path_opt(config.project_context_file())
            .with_workflow_prompt_path(config.prompts_dir().join("workflow-stages.md"))
            .with_template_vars(template_vars.clone());

        let full_system_prompt = SystemPromptBuilder::from_config(&prompt_config, &skill_registry).build();

        agents.push(AgentDescriptor {
            id: agent.id.clone(),
            display_name: agent.display_name.clone(),
            description: agent.description.clone(),
            aliases: agent.aliases.clone(),
            system_prompt_template: full_system_prompt,
            system_prompt_base: agent_prompt,
            initial_template_vars: template_vars,
            tool_whitelist: agent.tool_whitelist.clone(),
            model_config: Some(agent.model_config.clone()),
            provider_id: binding.provider_id.clone(),
            llm_id: binding
                .llm_id
                .clone()
                .expect("configured agent binding must always resolve to a concrete llm"),
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

    let client = OpenAiCompatClient::from_registry(config.providers.clone(), root_binding.provider_id.clone());
    let mut agent = AgentRuntime::new(client, tools, agent_config);
    agent.task_store = Some(task_store);
    agent.skill_registry = Some(skill_registry);

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
    let config_path = config.config_path();

    let conversation_service =
        ConversationService::new(agent, agent_registry.clone(), session_service.clone(), config.clone());
    let workspace_service =
        super::agent_workspace_service::AgentWorkspaceService::new(agent_registry, session_service, config_arc.clone());
    // let voice_service = build_voice_service(&config);

    Ok(Arc::new(AgentApplicationImpl::new(
        conversation_service,
        workspace_service,
        config_arc,
        config_path,
    )))
}
async fn load_agent_prompt(agent: &crate::config::AgentSpec, config: &AppConfig) -> Result<String> {
    if agent.prompt_file.is_some() && agent.prompt_inline.is_some() {
        bail!(
            "Agent '{}' has both prompt_file and prompt_inline configured; only one is allowed",
            agent.id
        );
    }

    if let Some(file) = &agent.prompt_file {
        let prompt_path = config.prompts_dir().join(file);
        let content = tokio::fs::read_to_string(&prompt_path)
            .await
            .with_context(|| format!("Failed to read prompt_file for agent '{}': {:?}", agent.id, prompt_path))?;
        return Ok(content);
    }

    if let Some(inline) = &agent.prompt_inline {
        return Ok(inline.clone());
    }

    if let Some(legacy) = &agent.system_prompt_template {
        log::warn!(
            "Agent '{}' uses legacy system_prompt_template. This field is deprecated; use prompt_file/prompt_inline.",
            agent.id
        );
        return Ok(legacy.clone());
    }

    let prompt_file = format!("agent-{}.md", agent.id);
    let prompt_path = config.prompts_dir().join(&prompt_file);
    match tokio::fs::read_to_string(&prompt_path).await {
        Ok(content) => Ok(content),
        Err(err) => {
            log::warn!("Failed to read prompt file {:?}: {}", prompt_path, err);
            Ok(String::new())
        }
    }
}

fn warn_unused_gateway_sections(config: &AppConfig) -> Result<()> {
    let config_path = config.config_path();
    let content = std::fs::read_to_string(&config_path).ok();
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
