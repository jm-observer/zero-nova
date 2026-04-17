use crate::provider::sse::SseParser;
use crate::provider::types::{MessageRequest, ToolDefinition};
use crate::provider::{LlmClient, ModelConfig, ProviderStreamEvent, StopReason, StreamReceiver};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::{header, Client};
use serde_json::json;

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
        messages: &[crate::message::Message],
        system: &str,
        tools: &[ToolDefinition],
        config: &ModelConfig,
    ) -> Result<Box<dyn StreamReceiver>> {
        // Build request body
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
                        content_vec.push(json!({"type": "text", "text": text}));
                    }
                    crate::message::ContentBlock::ToolUse { id, name, input } => {
                        content_vec.push(json!({
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": input
                        }));
                    }
                    crate::message::ContentBlock::ToolResult {
                        tool_use_id,
                        output,
                        is_error,
                    } => {
                        content_vec.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tool_use_id,
                            "content": output,
                            "is_error": is_error
                        }));
                    }
                }
            }
            input_messages.push(json!({"role": role, "content": content_vec}));
        }
        let body = MessageRequest {
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            top_p: config.top_p,
            stream: true,
            messages: input_messages
                .into_iter()
                .map(serde_json::from_value)
                .collect::<Result<Vec<_>, _>>()?,
            system: Some(system.to_string()),
            tools: if tools.is_empty() { None } else { Some(tools.to_vec()) },
        };
        let url = format!("{}/v1/messages", self.base_url);
        log::info!("Sending POST request to: {}", url);
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
        }))
    }
}

pub struct AnthropicStreamReceiver {
    response: reqwest::Response,
    parser: SseParser,
    current_tool_id: Option<String>,
    current_tool_name: Option<String>,
    pending_stop_reason: Option<StopReason>,
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
                        if content_block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
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
                            self.current_tool_id = Some(id.clone());
                            self.current_tool_name = Some(name.clone());
                            ProviderStreamEvent::ToolUseStart { id, name }
                        } else {
                            continue;
                        }
                    }
                    crate::provider::types::StreamEvent::ContentBlockDelta { delta, .. } => {
                        if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                            ProviderStreamEvent::TextDelta(text.to_string())
                        } else if let Some(partial_json) = delta.get("partial_json").and_then(|p| p.as_str()) {
                            ProviderStreamEvent::ToolUseInputDelta(partial_json.to_string())
                        } else {
                            continue;
                        }
                    }
                    crate::provider::types::StreamEvent::ContentBlockStop { .. } => {
                        if self.current_tool_id.is_some() {
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
                    log::debug!("Received chunk: {} bytes", chunk.len());
                    self.parser.feed(&chunk);
                }
                None => {
                    log::info!("Stream ended (no more chunks)");
                    return Ok(None);
                } // End of stream
            }
        }
    }
}
