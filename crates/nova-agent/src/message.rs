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
    /// Image block carrying base64-encoded bytes plus MIME type. Currently
    /// only used for inbound user content (vision); providers serialize this
    /// to their vendor-specific image part on the wire.
    Image { mime: String, data_base64: String },
}

/// Turn input accepted by `AgentApplicationImpl::start_turn`. The unit `Text`
/// variant preserves the v0.3.x string-only call-shape; `Multimodal` carries
/// arbitrary `ContentBlock`s (notably `Image`).
#[derive(Debug, Clone)]
pub enum UserInput {
    Text(String),
    Multimodal(Vec<ContentBlock>),
}

impl UserInput {
    /// Normalize to a non-empty `Vec<ContentBlock>` (`Text("")` collapses to empty).
    pub fn into_blocks(self) -> Vec<ContentBlock> {
        match self {
            UserInput::Text(text) => {
                if text.is_empty() {
                    Vec::new()
                } else {
                    vec![ContentBlock::Text { text }]
                }
            }
            UserInput::Multimodal(blocks) => blocks,
        }
    }

    /// Flatten to a plain string preview (concatenated text blocks). Used for
    /// snapshots / logs where structured content is not preserved.
    pub fn as_text_preview(&self) -> String {
        match self {
            UserInput::Text(text) => text.clone(),
            UserInput::Multimodal(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

impl From<&str> for UserInput {
    fn from(value: &str) -> Self {
        UserInput::Text(value.to_string())
    }
}

impl From<String> for UserInput {
    fn from(value: String) -> Self {
        UserInput::Text(value)
    }
}

impl From<&String> for UserInput {
    fn from(value: &String) -> Self {
        UserInput::Text(value.clone())
    }
}

impl From<Vec<ContentBlock>> for UserInput {
    fn from(value: Vec<ContentBlock>) -> Self {
        UserInput::Multimodal(value)
    }
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
