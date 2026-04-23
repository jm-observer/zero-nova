use crate::agent::{AgentConfig, AgentRuntime};
use crate::agent_catalog::{AgentDescriptor, AgentRegistry};
use crate::app::application::GatewayApplication;
use crate::app::conversation_service::ConversationService;
use crate::config::AppConfig;
use crate::conversation::repository::SqliteSessionRepository;
use crate::conversation::sqlite_manager::SqliteManager;
use crate::conversation::SessionStore;
use crate::provider::LlmClient;
use crate::skill::SkillRegistry;
use crate::tool::ToolRegistry;
use anyhow::{bail, Result};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// 初始化应用服务、加载资源并构造 GatewayApplication 实例
pub async fn bootstrap<C: LlmClient + 'static>(
    config: AppConfig,
    client: C,
    workspace: PathBuf,
) -> Result<GatewayApplication<C>> {
    let mut tools = ToolRegistry::new();
    crate::tool::builtin::register_builtin_tools(&mut tools, &config);

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

    let mut agents = config
        .gateway
        .agents
        .iter()
        .map(|a| {
            let agent_prompt = a
                .system_prompt_template
                .clone()
                .or_else(|| {
                    let prompt_path = workspace.join("prompts").join(format!("agent-{}.md", a.id));
                    match std::fs::read_to_string(&prompt_path) {
                        Ok(content) => Some(content),
                        Err(e) => {
                            log::warn!("Failed to read prompt file {:?}: {}", prompt_path, e);
                            None
                        }
                    }
                })
                .unwrap_or_default();

            let behavior_guards = r#"
## CRITICAL: Action Consistency
- You are a physical entity with real-world capabilities.
- If you state that you are going to perform an action (e.g., "running a command", "writing a file", "searching the web"), you MUST generate the corresponding tool_use block in the SAME response.
- NEVER claim you are doing something "in the background" or "internally" without an actual tool call.
- Textual confirmation of an action is only valid AFTER the tool has been invoked.
"#;

            let full_system_prompt = format!("{}\n\n{}\n\n{}", agent_prompt, skill_prompt, behavior_guards);

            AgentDescriptor {
                id: a.id.clone(),
                display_name: a.display_name.clone(),
                description: a.description.clone(),
                aliases: a.aliases.clone(),
                system_prompt_template: full_system_prompt,
                tool_whitelist: a.tool_whitelist.clone(),
                model_config: a.model_config.clone(),
            }
        })
        .collect::<Vec<_>>();

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

    // Initialize SQLite and SessionStore
    let data_dir = workspace.join(".nova").join("data");
    let sqlite_manager = SqliteManager::new(data_dir.to_str().unwrap()).await?;
    let repository = SqliteSessionRepository::new(sqlite_manager.pool);
    let session_store = SessionStore::new(repository);
    session_store.load_all().await?;

    // Construct ConversationService
    let conversation_service = ConversationService::new(agent, agent_registry, session_store);

    Ok(GatewayApplication::new(conversation_service, config_arc, config_path))
}
