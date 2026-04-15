//! Core library module for zero-nova.
//!
// This module re-exports the project sub-modules and provides the library entry point.

pub mod agent;
pub mod event;
pub mod mcp;
pub mod message;
pub mod prompt;
pub mod provider;
pub mod tool;

/// Runs the library initialization and entry point.
///
/// This function currently only logs that the application has started. It is a placeholder
/// for any future initialization steps such as configuration parsing, environment setup,
/// or establishing connections.
///
/// Returns an `anyhow::Result<()>` which will be `Ok(())` on success.
/// Runs the library initialization and entry point.
pub async fn run() -> anyhow::Result<()> {
    log::info!("application started");
    // Placeholder for future initialization
    Ok(())
}
