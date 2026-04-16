pub mod bridge;
pub mod protocol;
pub mod router;
pub mod server;
pub mod session;

pub use protocol::GatewayMessage;

pub use router::handle_message;
pub use server::run_server;

use crate::agent::{AgentConfig, AgentRuntime};
use crate::gateway::router::AppState;
use crate::gateway::session::SessionStore;
use crate::tool::ToolRegistry;
use std::net::SocketAddr;
use std::sync::Arc;

/// 启动 WebSocket Server 的主入口
pub async fn start_server<C: crate::provider::LlmClient + 'static>(
    config: crate::config::AppConfig,
    client: C,
) -> anyhow::Result<()> {
    let session_store = SessionStore::new();
    let mut tools = ToolRegistry::new();
    crate::tool::builtin::register_builtin_tools(&mut tools, &config);

    let agent_config = AgentConfig {
        max_iterations: config.gateway.max_iterations,
        model_config: config.llm.model_config.clone(),
    };

    let agent = AgentRuntime::new(client, tools, "You are a helpful assistant.".to_string(), agent_config);

    let state = Arc::new(AppState {
        agent,
        sessions: session_store,
    });

    let addr: SocketAddr = format!("{}:{}", config.gateway.host, config.gateway.port).parse()?;

    crate::gateway::server::run_server(addr, state).await
}
