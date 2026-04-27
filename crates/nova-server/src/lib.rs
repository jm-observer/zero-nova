use anyhow::Result;
use nova_agent::app::AgentApplication;
use nova_gateway_core::GatewayHandler;
use std::sync::Arc;

pub async fn run_server(addr: &str, app: Arc<dyn AgentApplication>) -> Result<()> {
    let handler = Arc::new(GatewayHandler::new(app));
    channel_core::websocket::run_server(addr, handler).await
}

pub async fn run_stdio(app: Arc<dyn AgentApplication>) -> Result<()> {
    let handler = Arc::new(GatewayHandler::new(app));
    channel_core::stdio::run_stdio(handler).await
}
