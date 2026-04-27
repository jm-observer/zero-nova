//! Core library module for zero-nova.
//!
//! This module re-exports the project sub-modules and provides the library entry point.

pub mod agent;
pub mod agent_catalog;
pub mod config;
pub mod event;
pub mod mcp;
pub mod message;
pub mod prompt;
pub mod provider;
pub mod skill;
pub mod tool;

pub mod app;
pub mod conversation;

pub use agent::{AgentConfig, AgentRuntime, TurnResult};
pub use agent_catalog::{AgentDescriptor, AgentRegistry};
pub use event::AgentEvent;
pub use mcp::{McpClient, McpToolDef, ServerInfo};
pub use message::{ContentBlock, Message, Role};
pub use prompt::{
    ActiveSkillState, SkillInvocationLevel, SkillRouteDecision, SkillSwitchResult, SystemPromptBuilder, TurnContext,
};
pub use provider::{LlmClient, ModelConfig, ProviderStreamEvent, StreamReceiver};
pub use skill::{CapabilityPolicy, FileToolPriority, PolicySource, Skill, SkillPackage, SkillRegistry, ToolPolicy};
pub use tool::{Tool, ToolContext, ToolDefinition, ToolRegistry};

pub async fn run() -> anyhow::Result<()> {
    log::info!("nova-core started");
    Ok(())
}
