pub mod agent_workspace_service;
pub mod application;
pub mod conversation_service;
pub mod inventory;
pub mod session_tree;
pub mod snapshot_assembler;
pub mod types;
pub mod voice_service;

pub use agent_workspace_service::AgentWorkspaceService;

pub use application::AgentApplicationImpl;
pub use conversation_service::ConversationService;
pub use inventory::ToolInventoryView;
pub use session_tree::SessionTree;
pub use types::{AppAgent, AppEvent, AppMessage, AppSession};
pub use voice_service::VoiceService;

// re-export: 保持 app 模块对外接口不变
pub use crate::conversation::SessionService;
pub use crate::event::AgentEvent;
pub use crate::message::ContentBlock;
