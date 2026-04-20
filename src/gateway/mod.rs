pub mod agents;
pub mod bridge;
pub mod control;
pub mod handlers;
pub mod protocol;
pub mod router;
pub mod server;
pub mod session;
pub mod workflow;

pub use protocol::GatewayMessage;

pub use router::handle_message;
pub use server::run_server;

use crate::agent::{AgentConfig, AgentRuntime};
use crate::gateway::agents::{AgentDescriptor, AgentRegistry};
use crate::gateway::router::AppState;
use crate::gateway::session::SessionStore;
use crate::prompt::SystemPromptBuilder;
use crate::tool::ToolRegistry;
use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::bail;

/// 启动 WebSocket Server 的主入口
pub async fn start_server<C: crate::provider::LlmClient + 'static>(
    config: crate::config::AppConfig,
    client: C,
    workspace: std::path::PathBuf,
) -> anyhow::Result<()> {
    let mut tools = ToolRegistry::new();
    crate::tool::builtin::register_builtin_tools(&mut tools, &config);

    // let prompt_builder = SystemPromptBuilder::new_from_path(&workspace);
    // let prompt = prompt_builder.with_tools(&tools).build();

    let agent_config = AgentConfig {
        max_iterations: config.gateway.max_iterations,
        model_config: config.llm.model_config.clone(),
        tool_timeout: std::time::Duration::from_secs(config.gateway.tool_timeout_secs.unwrap_or(120)),
    };

    let mut agents = config
        .gateway
        .agents
        .iter()
        .map(|a| AgentDescriptor {
            id: a.id.clone(),
            display_name: a.display_name.clone(),
            description: a.description.clone(),
            aliases: a.aliases.clone(),
            system_prompt_template: a
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
                .unwrap(),
            tool_whitelist: a.tool_whitelist.clone(),
            model_config: a.model_config.clone(),
        })
        .collect::<Vec<_>>();

    if agents.is_empty() {
        bail!("todo");
    }

    let mut agent_registry = AgentRegistry::new(agents.remove(0));
    for agent in agents {
        agent_registry.register(agent);
    }

    let agent = AgentRuntime::new(client, tools, agent_config);

    let config_arc = Arc::new(std::sync::RwLock::new(config.clone()));
    let config_path = workspace.join("config.toml");
    let session_store = SessionStore::new();
    let state = Arc::new(AppState {
        agent,
        agent_registry,
        sessions: session_store,
        config: config_arc,
        config_path,
    });

    let addr: SocketAddr = format!("{}:{}", config.gateway.host, config.gateway.port).parse()?;

    crate::gateway::server::run_server(addr, state).await
}
