use crate::provider::sse::SseParser;
use crate::provider::types::{MessageRequest, ToolDefinition};
use crate::provider::{LlmClient, ModelConfig, ProviderStreamEvent, StreamReceiver};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::{header, Client};
use serde_json::json;

pub struct AnthropicClient {
    http: Client,
    api_key: String,
    base_url: String,
}

impl AnthropicClient {
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| anyhow!("ANTHROPIC_API_KEY not set"))?;
        let base_url = std::env::var("ANTHROPIC_BASE_URL").unwrap_or_else(|_| "https://api.anthropic.com".to_string());
        Ok(Self {
            http: Client::new(),
            api_key,
            base_url,
        })
    }

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
                    // Simplify: ignore other block types for now
                    _ => {}
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
        let resp = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        Ok(Box::new(AnthropicStreamReceiver {
            response: resp,
            parser: SseParser::new(),
        }))
    }
}

pub struct AnthropicStreamReceiver {
    response: reqwest::Response,
    parser: SseParser,
}

#[async_trait]
impl StreamReceiver for AnthropicStreamReceiver {
    async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>> {
        loop {
            // First, try to get an event from the already buffered data
            if let Some(event) = self.parser.next_event()? {
                // Convert StreamEvent to ProviderStreamEvent
                let provider_event = match event {
                    crate::provider::types::StreamEvent::MessageStart { .. } => {
                        continue;
                    }
                    crate::provider::types::StreamEvent::ContentBlockDelta { delta, .. } => {
                        if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                            ProviderStreamEvent::TextDelta(text.to_string())
                        } else {
                            continue;
                        }
                    }
                    crate::provider::types::StreamEvent::MessageDelta { delta, .. } => {
                        if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                            ProviderStreamEvent::TextDelta(text.to_string())
                        } else {
                            continue;
                        }
                    }
                    crate::provider::types::StreamEvent::MessageStop { usage } => {
                        ProviderStreamEvent::MessageComplete {
                            usage: usage.unwrap_or_default(),
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
            match self.response.chunk().await? {
                Some(chunk) => {
                    self.parser.feed(&chunk);
                }
                None => return Ok(None), // End of stream
            }
        }
    }
}
