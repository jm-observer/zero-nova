//! Independent WebSocket gateway binary
use zero_nova::gateway::start_server;
use zero_nova::gateway::GatewayConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // For now, just a placeholder to satisfy cargo
    println!("Nova Gateway binary placeholder. Implement CLI parsing in Phase 4.");
    Ok(())
}
