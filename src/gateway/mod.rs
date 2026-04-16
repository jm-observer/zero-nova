pub mod bridge;
pub mod protocol;
pub mod router;
pub mod server;
pub mod session;

pub use protocol::GatewayConfig;
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
    config: GatewayConfig,
    client: C,
) -> anyhow::Result<()> {
    let session_store = SessionStore::new();
    let tools = ToolRegistry::new();

    let agent_config = AgentConfig {
        max_iterations: config.max_iterations,
        model_config: crate::provider::ModelConfig {
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            temperature: Some(0.7),
            top_p: Some(0.9),
        },
    };

    let agent = AgentRuntime::new(client, tools, "You are a helpful assistant.".to_string(), agent_config);

    let state = Arc::new(AppState {
        agent,
        sessions: session_store,
    });

    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;

    crate::gateway::server::run_server(addr, state).await
}
