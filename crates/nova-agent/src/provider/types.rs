use serde::{Deserialize, Serialize};

/// 轻量级请求上下文，用于 Provider 出站 HTTP 请求时透传 session_id / agent_id / message_id。
/// `message_id` 为单次 LLM HTTP 请求的唯一标识，调用方应在每次发起 stream 前生成新值
/// （通常用 `uuid::Uuid::new_v4().to_string()`）；空串视为未设置，不会注入到 Header。
#[derive(Debug, Clone, Default)]
pub struct ProviderRequestContext {
    pub session_id: Option<String>,
    pub agent_id: String,
    pub message_id: String,
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ThinkingConfig {
    #[serde(rename = "type")]
    pub kind: ThinkingMode,
    pub budget_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingMode {
    Enabled,
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
    Thinking {
        thinking: String,
    },
    /// Tool usage block, containing tool ID, name, and input.
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Tool result block, containing the result output and error flag.
    ///
    /// `output` 字段映射到 Anthropic wire 协议的 `content`，可以是:
    /// - 纯文本字符串（无图场景，向后兼容）
    /// - 混合 content blocks 数组（含图片场景；Anthropic 原生支持 `content: [text, image, ...]`）
    ///
    /// 由 provider 序列化层根据 `ContentBlock::ToolResult.images` 是否为空决定走哪种形态。
    /// 详见 docs/2026-05-29-image-handle-injection/vision-tool-result-design.md。
    ToolResult {
        tool_use_id: String,
        #[serde(rename = "content")]
        output: ToolResultBody,
        #[serde(default)]
        is_error: bool,
    },
    /// Image block carrying base64 image data; serializes to the Anthropic
    /// vision content format (`{type:"image", source:{type:"base64", ...}}`).
    Image {
        source: AnthropicImageSource,
    },
}

/// Anthropic `tool_result.content` 的两种形态：纯文本或 blocks 数组。
/// `untagged` serde:序列化时按当前 variant 自动选 String 或 Array,
/// 反序列化时尝试匹配(优先 String)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultBody {
    Plain(String),
    Blocks(Vec<ToolResultContentBlock>),
}

impl From<String> for ToolResultBody {
    fn from(s: String) -> Self {
        ToolResultBody::Plain(s)
    }
}

/// Anthropic `tool_result.content` blocks 数组里允许的元素类型(text / image)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResultContentBlock {
    Text { text: String },
    Image { source: AnthropicImageSource },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicImageSource {
    #[serde(rename = "type")]
    pub kind: String,
    pub media_type: String,
    pub data: String,
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
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_provider_usage: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Specifies tool selection behavior for the LLM.
pub enum ToolChoice {
    Auto,
    Any,
    Tool { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Specifies the reason why the model stopped generating.
pub enum StopReason {
    /// Model thinks it has finished the response.
    EndTurn,
    /// Output reached max_tokens limit.
    MaxTokens,
    /// Encountered a custom stop sequence.
    StopSequence,
    /// Model requested a tool call.
    ToolUse,
    /// Unknown reason (forward compatibility).
    #[serde(other)]
    Unknown,
}

// ============================================================================
// P3a-6: Anthropic wire 协议 tool_result 形态测试
// 详见 docs/2026-05-29-image-handle-injection/vision-tool-result-design.md
// ============================================================================
#[cfg(test)]
mod tool_result_body_tests {
    use super::{AnthropicImageSource, InputContentBlock, ToolResultBody, ToolResultContentBlock};
    use serde_json::{json, Value};

    /// 无图 → `content` 字段序列化成 JSON 字符串(向后兼容既有形态)。
    #[test]
    fn tool_result_without_image_serializes_to_plain_string() {
        let block = InputContentBlock::ToolResult {
            tool_use_id: "call_1".to_string(),
            output: ToolResultBody::Plain("plain text".to_string()),
            is_error: false,
        };
        let v: Value = serde_json::to_value(&block).unwrap();
        assert_eq!(v["type"], "tool_result");
        assert_eq!(v["tool_use_id"], "call_1");
        assert_eq!(v["content"], json!("plain text"));
        assert_eq!(v["is_error"], false);
    }

    /// 有图 → `content` 字段序列化成 blocks 数组(Anthropic 原生格式)。
    /// 每个 block 自带 `type` 区分 text/image。
    #[test]
    fn tool_result_with_image_serializes_to_blocks_array() {
        let block = InputContentBlock::ToolResult {
            tool_use_id: "call_1".to_string(),
            output: ToolResultBody::Blocks(vec![
                ToolResultContentBlock::Text {
                    text: "图片已读取".to_string(),
                },
                ToolResultContentBlock::Image {
                    source: AnthropicImageSource {
                        kind: "base64".to_string(),
                        media_type: "image/jpeg".to_string(),
                        data: "FAKE_B64".to_string(),
                    },
                },
            ]),
            is_error: false,
        };
        let v: Value = serde_json::to_value(&block).unwrap();
        assert_eq!(v["type"], "tool_result");
        let content = v["content"].as_array().expect("content 应是数组");
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "图片已读取");
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["source"]["type"], "base64");
        assert_eq!(content[1]["source"]["media_type"], "image/jpeg");
        assert_eq!(content[1]["source"]["data"], "FAKE_B64");
    }

    /// 反序列化兼容性:既能接受字符串形态又能接受数组形态。
    #[test]
    fn tool_result_body_deserializes_both_forms() {
        let from_string: ToolResultBody = serde_json::from_value(json!("hi")).expect("Plain 反序列化");
        assert!(matches!(from_string, ToolResultBody::Plain(s) if s == "hi"));

        let from_array: ToolResultBody = serde_json::from_value(json!([
            {"type": "text", "text": "x"},
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAA"}}
        ]))
        .expect("Blocks 反序列化");
        match from_array {
            ToolResultBody::Blocks(blocks) => assert_eq!(blocks.len(), 2),
            other => panic!("应是 Blocks,得到 {other:?}"),
        }
    }
}
