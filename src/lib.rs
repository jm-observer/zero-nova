//! Core library module for zero-nova.
//!
//! This module re-exports the project sub-modules and provides the library entry point.

pub mod agent;
pub mod config;

pub mod event;
pub mod mcp;
pub mod message;
pub mod prompt;
pub mod provider;
pub mod skill;
pub mod tool;

#[cfg(feature = "gateway")]
pub mod gateway;

/// Runs the library initialization and entry point.
///
/// This function currently only logs that the application has started. It is a placeholder
/// for any future initialization steps such as configuration parsing, environment setup,
/// or establishing connections.
///
/// Returns an `anyhow::Result<()>` which will be `Ok(())` on success.
pub async fn run() -> anyhow::Result<()> {
    log::info!("application started");
    // Placeholder for future initialization
    Ok(())
}
