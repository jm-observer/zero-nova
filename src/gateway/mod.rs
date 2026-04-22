pub mod bridge;
pub mod handlers;
pub mod protocol;
pub mod router;
pub mod server;

pub use protocol::GatewayMessage;
pub use router::handle_message;
pub use server::run_server;

use crate::agent::{AgentConfig, AgentRuntime};
use crate::agent_catalog::{AgentDescriptor, AgentRegistry};
use crate::app::conversation_service::ConversationService;
use crate::conversation::repository::SqliteSessionRepository;
use crate::conversation::sqlite_manager::SqliteManager;
use crate::conversation::SessionStore;
use crate::gateway::router::AppState;

use crate::skill::SkillRegistry;
use crate::tool::ToolRegistry;
use anyhow::bail;
use std::net::SocketAddr;
use std::sync::Arc;

/// 启动 WebSocket Server 的主入口
pub async fn start_server<C: crate::provider::LlmClient + 'static>(
    config: crate::config::AppConfig,
    client: C,
    workspace: std::path::PathBuf,
) -> anyhow::Result<()> {
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

    let config_arc = Arc::new(std::sync::RwLock::new(config.clone()));
    let config_path = workspace.join("config.toml");

    // Initialize SQLite and SessionStore
    let data_dir = workspace.join(".nova").join("data");
    let sqlite_manager = SqliteManager::new(data_dir.to_str().unwrap()).await?;
    let repository = SqliteSessionRepository::new(sqlite_manager.pool);
    let session_store = SessionStore::new(repository);
    session_store.load_all().await?;

    // Construct ConversationService
    let conversation_service = ConversationService::new(agent, agent_registry, session_store);

    let state = Arc::new(AppState {
        conversation_service,
        config: config_arc,
        config_path,
    });

    let addr: SocketAddr = format!("{}:{}", config.gateway.host, config.gateway.port).parse()?;

    crate::gateway::server::run_server(addr, state).await
}
