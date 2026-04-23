use crate::application::{AgentApplication, AgentApplicationImpl};
use crate::conversation_service::ConversationService;
use anyhow::{bail, Context, Result};
use nova_conversation::repository::SqliteSessionRepository;
use nova_conversation::sqlite_manager::SqliteManager;
use nova_core::agent::{AgentConfig, AgentRuntime};
use nova_core::agent_catalog::{AgentDescriptor, AgentRegistry};
use nova_core::config::AppConfig;
use nova_core::provider::LlmClient;
use nova_core::skill::SkillRegistry;
use nova_core::tool::ToolRegistry;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub struct BootstrapOptions {
    pub bind_addr: SocketAddr,
}

pub async fn build_application<C: LlmClient + 'static>(
    config: AppConfig,
    client: C,
    workspace: PathBuf,
) -> Result<Arc<dyn AgentApplication>> {
    let mut tools = ToolRegistry::new();
    nova_core::tool::builtin::register_builtin_tools(&mut tools, &config);

    let mut skill_registry = SkillRegistry::new();
    let skill_dir = workspace.join("skills");
    if let Err(e) = skill_registry.load_from_dir(&skill_dir) {
        log::warn!("Failed to load skills from {:?}: {}", skill_dir, e);
    }
    let skill_prompt = skill_registry.generate_system_prompt();

    let agent_config = AgentConfig {
        max_iterations: config.gateway.max_iterations,
        model_config: config.llm.model_config.clone(),
        tool_timeout: std::time::Duration::from_secs(config.gateway.tool_timeout_secs.unwrap_or(120)),
    };

    let mut agents = Vec::with_capacity(config.gateway.agents.len());
    for agent in &config.gateway.agents {
        let agent_prompt = match &agent.system_prompt_template {
            Some(prompt) => prompt.clone(),
            None => {
                let prompt_path = workspace.join("prompts").join(format!("agent-{}.md", agent.id));
                match tokio::fs::read_to_string(&prompt_path).await {
                    Ok(content) => content,
                    Err(e) => {
                        log::warn!("Failed to read prompt file {:?}: {}", prompt_path, e);
                        String::new()
                    }
                }
            }
        };

        let behavior_guards = r#"
## CRITICAL: Action Consistency
- You are a physical entity with real-world capabilities.
- If you state that you are going to perform an action (e.g., "running a command", "writing a file", "searching the web"), you MUST generate the corresponding tool_use block in the SAME response.
- NEVER claim you are doing something "in the background" or "internally" without an actual tool call.
- Textual confirmation of an action is only valid AFTER the tool has been invoked.
"#;

        let full_system_prompt = format!("{}\n\n{}\n\n{}", agent_prompt, skill_prompt, behavior_guards);

        agents.push(AgentDescriptor {
            id: agent.id.clone(),
            display_name: agent.display_name.clone(),
            description: agent.description.clone(),
            aliases: agent.aliases.clone(),
            system_prompt_template: full_system_prompt,
            tool_whitelist: agent.tool_whitelist.clone(),
            model_config: agent.model_config.clone(),
        });
    }

    if agents.is_empty() {
        bail!("No agents configured");
    }

    let mut agent_registry = AgentRegistry::new(agents.remove(0));
    for agent in agents {
        agent_registry.register(agent);
    }

    let agent = AgentRuntime::new(client, tools, agent_config);

    let config_arc = Arc::new(RwLock::new(config.clone()));
    let config_path = workspace.join("config.toml");

    let data_dir = workspace.join(".nova").join("data");
    let data_dir = data_dir
        .to_str()
        .context("Workspace data directory contains non-UTF8 characters")?;
    let sqlite_manager = SqliteManager::new(data_dir).await?;
    let repository = SqliteSessionRepository::new(sqlite_manager.pool);
    let session_cache = Arc::new(nova_conversation::SessionCache::new());
    let session_service = nova_conversation::SessionService::new(session_cache, repository);
    session_service.load_all().await?;

    let conversation_service = ConversationService::new(agent, agent_registry, session_service);

    Ok(Arc::new(AgentApplicationImpl::new(
        conversation_service,
        config_arc,
        config_path,
    )))
}
