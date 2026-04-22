//! Core library module for zero-nova.
//!
//! This module re-exports the project sub-modules and provides the library entry point.

pub mod agent;
pub mod agent_catalog;
pub mod app;
pub mod config;
pub mod conversation;

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
pub async fn run() -> anyhow::Result<()> {
    log::info!("application started");
    Ok(())
}
