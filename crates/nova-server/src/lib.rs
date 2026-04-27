use anyhow::Result;
use channel_core::{stdio::run_stdio as run_channel_stdio, websocket::run_server as run_channel_server};
use nova_agent::app::AgentApplication;
use nova_gateway_core::GatewayHandler;
use std::sync::Arc;

pub async fn run_server(addr: &str, app: Arc<dyn AgentApplication>) -> Result<()> {
    let handler = Arc::new(GatewayHandler::new(app));
    run_channel_server(addr, handler).await
}

pub async fn run_stdio(app: Arc<dyn AgentApplication>) -> Result<()> {
    let handler = Arc::new(GatewayHandler::new(app));
    run_channel_stdio(handler).await
}
