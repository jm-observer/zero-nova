use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Role of a message sender (User or Assistant).
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
/// Different blocks that can appear in a message content.
pub enum ContentBlock {
    /// Text block.
    Text { text: String },
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
/// Represents a chat message with a role and content blocks.
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}
