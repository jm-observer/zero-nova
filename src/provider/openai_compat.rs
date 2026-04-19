use crate::provider::sse::SseParser;
use crate::provider::types::{StopReason, ToolDefinition};
use crate::provider::{LlmClient, ModelConfig, ProviderStreamEvent, StreamReceiver};
use anyhow::Result;
use async_trait::async_trait;
use reqwest::{Client, header};
use serde_json::json;

/// Client for interacting with OpenAI-compatible APIs.
pub struct OpenAiCompatClient {
    http: Client,
    api_key: String,
    base_url: String,
}

impl OpenAiCompatClient {
    /// Constructs a new `OpenAiCompatClient` with the provided API key and base URL.
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
            base_url,
        }
    }
}

#[async_trait]
impl LlmClient for OpenAiCompatClient {
    async fn stream(
        &self,
        messages: &[crate::message::Message],
        system: &str,
        tools: &[ToolDefinition],
        config: &ModelConfig,
    ) -> Result<Box<dyn StreamReceiver>> {
        let mut input_messages = Vec::new();
        for msg in messages {
            let role = match msg.role {
                crate::message::Role::User => "user",
                crate::message::Role::Assistant => "assistant",
            };

            let mut content_vec = Vec::new();
            for block in &msg.content {
                match block {
                    crate::message::ContentBlock::Text { text } => {
                        content_vec.push(json!({ "type": "text", "text": text }));
                    }
                    crate::message::ContentBlock::Thinking { thinking } => {
                        content_vec.push(json!({ "type": "text", "text": thinking }));
                    }
                    crate::message::ContentBlock::ToolUse { id, name, input } => {
                        content_vec.push(json!({
                            "type": "tool_call",
                            "id": id,
                            "function": { "name": name, "arguments": input }
                        }));
                    }
                    crate::message::ContentBlock::ToolResult {
                        tool_use_id,
                        output,
                        is_error,
                    } => {
                        content_vec.push(json!({
                            "type": "tool",
                            "tool_call_id": tool_use_id,
                            "content": output,
                            "is_error": *is_error
                        }));
                    }
                }
            }

            input_messages.push(json!({
                "role": role,
                "content": content_vec
            }));
        }

        let mut body = json!({
            "model": config.model,
            "messages": input_messages,
            "stream": true,
        });

        if !system.is_empty() {
            body["system"] = json!(system);
        }

        if !tools.is_empty() {
            body["tools"] = json!(
                tools
                    .iter()
                    .map(|t| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.input_schema
                            }
                        })
                    })
                    .collect::<Vec<_>>()
            );
        }

        if let Some(temp) = config.temperature {
            body["temperature"] = json!(temp);
        }
        if let Some(top_p) = config.top_p {
            body["top_p"] = json!(top_p);
        }
        body["max_tokens"] = json!(config.max_tokens);

        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        Ok(Box::new(OpenAiCompatStreamReceiver {
            response: resp,
            parser: SseParser::new(),
            current_tool_id: None,
            current_tool_name: None,
            pending_stop_reason: None,
            current_block_type: None,
        }))
    }
}

pub struct OpenAiCompatStreamReceiver {
    response: reqwest::Response,
    parser: SseParser,
    current_tool_id: Option<String>,
    current_tool_name: Option<String>,
    pending_stop_reason: Option<StopReason>,
    current_block_type: Option<BlockType>,
}

#[derive(Debug, Clone, PartialEq)]
enum BlockType {
    Text,
    Thinking,
    ToolUse,
}

#[async_trait]
impl StreamReceiver for OpenAiCompatStreamReceiver {
    async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>> {
        loop {
            if let Some(_event) = self.parser.next_event()? {
                // Implementation detail for OpenAI delta parsing
                return Ok(None);
            }

            match self.response.chunk().await? {
                Some(chunk) => self.parser.feed(&chunk),
                None => return Ok(None),
            }
        }
    }
}
