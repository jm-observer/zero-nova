//! Core library module for zero-nova.
//!
//! This module re-exports the project sub-modules and provides the library entry point.

pub mod agent;
pub mod agent_catalog;
pub mod config;
pub mod event;
pub mod loop_guard;
pub mod mcp;
pub mod message;
pub mod network;
pub mod orchestrator;
pub mod path_resolver;
pub mod prompt;
pub mod prompt_provider;
pub mod provider;
pub mod skill;
pub mod tool;
pub mod voice;

pub mod app;
pub mod conversation;

pub use agent::{AgentConfig, AgentRuntime, TurnResult};
pub use agent_catalog::{AgentDescriptor, AgentRegistry};
pub use app::{SessionTree, ToolInventoryView};
pub use conversation::session::SessionSummary;
pub use event::AgentEvent;
pub use mcp::{McpClient, McpToolDef, ServerInfo};
pub use message::{ContentBlock, Message, Role, UserInput};
pub use prompt::{
    ActiveSkillState, SkillInvocationLevel, SkillRouteDecision, SkillSwitchResult, SystemPromptBuilder, TurnContext,
};
pub use prompt_provider::{AgentPromptProvider, PromptProviderRegistry};
pub use provider::{LlmClient, ModelConfig, ProviderStreamEvent, StreamReceiver};
pub use skill::{CapabilityPolicy, FileToolPriority, PolicySource, Skill, SkillPackage, SkillRegistry, ToolPolicy};
pub use tool::builtin::agent::NativeDeferredToolSeed;
pub use tool::builtin::orchestrate_hook::{OrchestrateTaskHookSlot, OrchestrateTaskPromptHook};
pub use tool::builtin::skill_system_hook::{SkillSystemPromptHook, SkillSystemPromptHookSlot};
pub use tool::builtin::BuiltinHookSlots;
pub use tool::{
    DeferredToolCategory, DeferredToolRepresentation, RegisteredToolDefinition, Tool, ToolContext, ToolRegistry,
};
