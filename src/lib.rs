pub mod agent;
pub mod event;
pub mod mcp;
pub mod message;
pub mod prompt;
pub mod provider;
pub mod tool;

pub async fn run() -> anyhow::Result<()> {
    log::info!("application started");
    // Placeholder for future initialization
    Ok(())
}
