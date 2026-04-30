use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Role of a message sender (User or Assistant).
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
/// Different blocks that can appear in a message content.
pub enum ContentBlock {
    /// Text block.
    Text { text: String },
    /// Thinking block, containing the reasoning process.
    Thinking { thinking: String },
    /// Tool usage block, containing tool ID, name, and input.
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Tool result block, containing the result output and error flag.
    ToolResult {
        tool_use_id: String,
        output: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHttpTrace {
    pub request_body: Value,
    pub response_body: Value,
    pub format: String,
    pub bound_message_id: String,
    pub captured_at: i64,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MessageMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_http_trace: Option<ProviderHttpTrace>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Represents a chat message with a role and content blocks.
pub struct Message {
    pub id: String,
    pub role: Role,
    pub content: Vec<ContentBlock>,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MessageMetadata>,
}

impl Message {
    pub fn new(role: Role, content: Vec<ContentBlock>, created_at: i64) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role,
            content,
            created_at,
            metadata: None,
        }
    }
}
