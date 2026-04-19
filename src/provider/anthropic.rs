use crate::provider::sse::SseParser;
use crate::provider::types::{MessageRequest, ToolDefinition};
use crate::provider::{LlmClient, ModelConfig, ProviderStreamEvent, StopReason, StreamReceiver};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::{header, Client};

/// Client for interacting with the Anthropic API.
pub struct AnthropicClient {
    http: Client,
    api_key: String,
    base_url: String,
}

impl AnthropicClient {
    /// Constructs an `AnthropicClient` using the provided configuration.
    pub fn from_config(config: &crate::config::LlmConfig) -> Self {
        Self {
            http: Client::new(),
            api_key: config.api_key.clone(),
            base_url: config.base_url.clone(),
        }
    }

    /// Constructs a new `AnthropicClient` with the provided API key and base URL.
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
            base_url,
        }
    }
}

#[allow(clippy::single_match)]
#[async_trait]
impl LlmClient for AnthropicClient {
    /// Streams responses from the Anthropic API based on the provided messages and configuration.
    async fn stream(
        &self,
        _messages: &[crate::message::Message],
        _system: &str,
        _tools: &[ToolDefinition],
        config: &ModelConfig,
    ) -> Result<Box<dyn StreamReceiver>> {
        // Build request body
        let mut input_messages = Vec::new();
        for msg in _messages {
            let role = match msg.role {
                crate::message::Role::User => "user",
                crate::message::Role::Assistant => "assistant",
            };
            let content_vec: Vec<crate::provider::types::InputContentBlock> = msg
                .content
                .iter()
                .map(|block| match block {
                    crate::message::ContentBlock::Text { text } => {
                        crate::provider::types::InputContentBlock::Text { text: text.clone() }
                    }
                    crate::message::ContentBlock::Thinking { thinking } => {
                        crate::provider::types::InputContentBlock::Thinking {
                            thinking: thinking.clone(),
                        }
                    }
                    crate::message::ContentBlock::ToolUse { id, name, input } => {
                        crate::provider::types::InputContentBlock::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input: input.clone(),
                        }
                    }
                    crate::message::ContentBlock::ToolResult {
                        tool_use_id,
                        output,
                        is_error,
                    } => crate::provider::types::InputContentBlock::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        output: output.clone(),
                        is_error: *is_error,
                    },
                })
                .collect();

            input_messages.push(crate::provider::types::InputMessage {
                role: role.to_string(),
                content: content_vec,
            });
        }
        let mut body = MessageRequest {
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            top_p: config.top_p,
            stream: true,
            messages: input_messages,
            system: Some(_system.to_string()),
            tools: if _tools.is_empty() { None } else { Some(_tools.to_vec()) },
            thinking: None,
        };

        // Constraints: if thinking is enabled, temperature must be 1.0
        if let Some(budget) = config.thinking_budget {
            body.thinking = Some(crate::provider::types::ThinkingConfig {
                kind: "enabled".to_string(),
                budget_tokens: budget,
            });
            body.temperature = Some(1.0);
            body.top_p = None;
        }

        let url = format!("{}/v1/messages", self.base_url);
        log::trace!("Sending POST request to: {}", url);
        let resp = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .inspect_err(|e| log::error!("HTTP request failed: {}", e))?
            .error_for_status()
            .inspect_err(|e| log::error!("HTTP response error status: {}", e))?;

        log::info!("HTTP response received: {}", resp.status());
        Ok(Box::new(AnthropicStreamReceiver {
            response: resp,
            parser: SseParser::new(),
            current_tool_id: None,
            current_tool_name: None,
            pending_stop_reason: None,
            current_block_type: None,
        }))
    }
}

#[derive(Debug, Clone, PartialEq)]
enum BlockType {
    Text,
    Thinking,
    ToolUse,
}

pub struct AnthropicStreamReceiver {
    response: reqwest::Response,
    parser: SseParser,
    current_tool_id: Option<String>,
    current_tool_name: Option<String>,
    pending_stop_reason: Option<StopReason>,
    current_block_type: Option<BlockType>,
}

#[async_trait]
impl StreamReceiver for AnthropicStreamReceiver {
    /// Retrieves the next streamed event from the Anthropic response.
    async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>> {
        loop {
            // First, try to get an event from the already buffered data
            if let Some(event) = self.parser.next_event()? {
                // Convert StreamEvent to ProviderStreamEvent
                let provider_event = match event {
                    crate::provider::types::StreamEvent::ContentBlockStart { content_block, .. } => {
                        let block_type = content_block.get("type").and_then(|t| t.as_str());
                        match block_type {
                            Some("tool_use") => {
                                let id = content_block
                                    .get("id")
                                    .and_then(|i| i.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                let name = content_block
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                self.current_block_type = Some(BlockType::ToolUse);
                                self.current_tool_id = Some(id.clone());
                                self.current_tool_name = Some(name.clone());
                                ProviderStreamEvent::ToolUseStart { id, name }
                            }
                            Some("thinking") => {
                                self.current_block_type = Some(BlockType::Thinking);
                                continue;
                            }
                            Some("text") => {
                                self.current_block_type = Some(BlockType::Text);
                                continue;
                            }
                            _ => continue,
                        }
                    }
                    crate::provider::types::StreamEvent::ContentBlockDelta { delta, .. } => {
                        match self.current_block_type {
                            Some(BlockType::Thinking) => {
                                if let Some(thinking) = delta.get("thinking").and_then(|t| t.as_str()) {
                                    ProviderStreamEvent::ThinkingDelta(thinking.to_string())
                                } else {
                                    continue;
                                }
                            }
                            Some(BlockType::ToolUse) => {
                                if let Some(partial_json) = delta.get("partial_json").and_then(|p| p.as_str()) {
                                    ProviderStreamEvent::ToolUseInputDelta(partial_json.to_string())
                                } else {
                                    continue;
                                }
                            }
                            _ => {
                                if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                    ProviderStreamEvent::TextDelta(text.to_string())
                                } else {
                                    continue;
                                }
                            }
                        }
                    }
                    crate::provider::types::StreamEvent::ContentBlockStop { .. } => {
                        let was_tool = self.current_block_type == Some(BlockType::ToolUse);
                        self.current_block_type = None;
                        if was_tool {
                            self.current_tool_id = None;
                            self.current_tool_name = None;
                            ProviderStreamEvent::ToolUseEnd
                        } else {
                            continue;
                        }
                    }
                    crate::provider::types::StreamEvent::MessageDelta { delta, .. } => {
                        if let Some(stop_reason_val) = delta.get("stop_reason") {
                            if !stop_reason_val.is_null() {
                                if let Ok(reason) = serde_json::from_value::<crate::provider::types::StopReason>(
                                    stop_reason_val.clone(),
                                ) {
                                    self.pending_stop_reason = Some(reason);
                                }
                            }
                        }

                        if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                            ProviderStreamEvent::TextDelta(text.to_string())
                        } else {
                            continue;
                        }
                    }
                    crate::provider::types::StreamEvent::MessageStop { usage } => {
                        ProviderStreamEvent::MessageComplete {
                            usage: usage.unwrap_or_default(),
                            stop_reason: self.pending_stop_reason.take(),
                        }
                    }
                    crate::provider::types::StreamEvent::Error { error } => {
                        return Err(anyhow!("Anthropic API Error: {}", error));
                    }
                    _ => continue,
                };
                return Ok(Some(provider_event));
            }

            // If no full event in buffer, read more from the response
            match self
                .response
                .chunk()
                .await
                .inspect_err(|e| log::error!("Failed to read chunk from response: {}", e))?
            {
                Some(chunk) => {
                    log::trace!("Received chunk: {} bytes", chunk.len());
                    self.parser.feed(&chunk);
                }
                None => {
                    log::trace!("Stream ended (no more chunks)");
                    return Ok(None);
                } // End of stream
            }
        }
    }
}
