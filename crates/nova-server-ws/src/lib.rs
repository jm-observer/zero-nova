use anyhow::Result;
use nova_app::AgentApplication;
use nova_gateway_core::GatewayHandler;
use std::sync::Arc;

pub async fn run_server(addr: &str, app: Arc<dyn AgentApplication>) -> Result<()> {
    let handler = Arc::new(GatewayHandler::new(app));
    channel_websocket::run_server(addr, handler).await
}
