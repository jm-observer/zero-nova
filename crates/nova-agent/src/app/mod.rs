pub mod agent_workspace_service;
pub mod application;
pub mod bootstrap;
pub mod conversation_service;
pub mod snapshot_assembler;
pub mod types;

pub use agent_workspace_service::AgentWorkspaceService;

pub use application::{AgentApplication, AgentApplicationImpl};
pub use bootstrap::{build_application, BootstrapOptions};
pub use conversation_service::ConversationService;
pub use types::{AppAgent, AppEvent, AppMessage, AppSession};

// re-export: 保持 app 模块对外接口不变
pub use crate::conversation::SessionService;
pub use crate::event::AgentEvent;
pub use crate::message::ContentBlock;
pub use crate::provider::LlmClient;
