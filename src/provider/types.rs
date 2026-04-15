use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageRequest {
    pub model: String,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    pub stream: bool,
    pub messages: Vec<InputMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InputMessage {
    pub role: String,
    pub content: Vec<InputContentBlock>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputContentBlock {
    Text {
        text: String,
    },
    /// Tool usage block, containing tool ID, name, and input.
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Tool result block, containing the result output and error flag.
    ToolResult {
        tool_use_id: String,
        #[serde(alias = "content", default)]
        output: String,
        #[serde(default)]
        is_error: bool,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    MessageStart {
        message: serde_json::Value,
    },
    ContentBlockStart {
        index: usize,
        content_block: serde_json::Value,
    },
    ContentBlockDelta {
        index: usize,
        delta: serde_json::Value,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: serde_json::Value,
        #[serde(default)]
        usage: Option<Usage>,
    },
    MessageStop {
        #[serde(default)]
        usage: Option<Usage>,
    },
    Ping {},
    Error {
        error: serde_json::Value,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
/// Definition of a tool, including its name, description, and input schema.
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
/// Token usage statistics for a request.
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Specifies tool selection behavior for the LLM.
pub enum ToolChoice {
    Auto,
    Any,
    Tool { name: String },
}
