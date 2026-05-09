pub mod conv;

use crate::config::ProviderConfig;
use crate::message::Message;
use crate::provider::openai_compat::conv::{build_request, map_finish_reason, map_usage};
use crate::provider::sse::{RawSseEvent, SseParser};
use crate::provider::types::{StopReason, ToolDefinition, Usage};
use crate::provider::{LlmClient, ModelConfig, ProviderStreamEvent, StreamReceiver};
use anyhow::{anyhow, Result};
use async_openai::types::chat::CreateChatCompletionStreamResponse;
use async_trait::async_trait;
use log::{debug, trace};
use reqwest::{header, Client};
use std::collections::{HashMap, VecDeque};

/// Client for interacting with OpenAI-compatible APIs using async-openai SDK.
pub struct OpenAiCompatClient {
    endpoint: OpenAiCompatEndpoint,
    http: Client,
}

enum OpenAiCompatEndpoint {
    Fixed {
        api_key: String,
        base_url: String,
    },
    Registry {
        providers: HashMap<String, ProviderConfig>,
        default_provider: String,
    },
}

impl OpenAiCompatClient {
    /// Constructs a new `OpenAiCompatClient` with the provided API key and base URL.
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            endpoint: OpenAiCompatEndpoint::Fixed { api_key, base_url },
            http: Client::new(),
        }
    }

    pub fn from_registry(providers: HashMap<String, ProviderConfig>, default_provider: String) -> Self {
        Self {
            endpoint: OpenAiCompatEndpoint::Registry {
                providers,
                default_provider,
            },
            http: Client::new(),
        }
    }

    fn resolve_endpoint(&self, config: &ModelConfig) -> Result<(String, String)> {
        match &self.endpoint {
            OpenAiCompatEndpoint::Fixed { api_key, base_url } => Ok((api_key.clone(), base_url.clone())),
            OpenAiCompatEndpoint::Registry {
                providers,
                default_provider,
            } => {
                let provider_id = config.provider.as_deref().unwrap_or(default_provider.as_str());
                let provider = providers
                    .get(provider_id)
                    .ok_or_else(|| anyhow!("Unknown provider '{}' for model '{}'", provider_id, config.model))?;
                Ok((provider.api_key.clone(), provider.base_url.clone()))
            }
        }
    }
}

#[async_trait]
impl LlmClient for OpenAiCompatClient {
    async fn stream(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        config: &ModelConfig,
    ) -> Result<Box<dyn StreamReceiver>> {
        let request = build_request(messages, tools, config);
        let (api_key, base_url) = self.resolve_endpoint(config)?;
        let base = base_url.trim_end_matches('/').to_string();

        debug!(
            "[OUTBOUND] LLM HTTP request via reqwest: provider={:?}, model={}, msg_count={}",
            config.provider,
            config.model,
            messages.len()
        );

        // 序列化请求体并注入 openai-compatible 扩展参数用于日志/诊断
        let mut request_body = serde_json::to_value(&request).unwrap_or(serde_json::Value::Null);
        if let Some(obj) = request_body.as_object_mut() {
            obj.insert(
                "extra_body".to_string(),
                serde_json::json!({
                    "top_k": 20,
                    "chat_template_kwargs": {
                        "enable_thinking": true,
                        "preserve_thinking": true
                    }
                }),
            );
        }

        let url = format!("{}/chat/completions", base);
        let response = self
            .http
            .post(url)
            .header(header::CONTENT_TYPE, "application/json")
            .bearer_auth(api_key)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to create chat stream: {}", e))?
            .error_for_status()
            .map_err(|e| anyhow!("OpenAI-compatible response error status: {}", e))?;

        Ok(Box::new(OpenAiCompatStreamReceiver::new(response, request_body)))
    }
}

#[derive(Debug, Clone)]
struct PendingToolCall {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    name: String,
    arguments_buffer: String,
}

pub struct OpenAiCompatStreamReceiver {
    response: reqwest::Response,
    parser: SseParser,
    /// 按 index 存储正在组装的 tool calls
    pending_tool_calls: Vec<Option<PendingToolCall>>,
    pending_stop_reason: Option<StopReason>,
    /// 缓存待发射的事件（单个 chunk 可能产生多个 ProviderStreamEvent）
    event_queue: VecDeque<ProviderStreamEvent>,
    request_body: serde_json::Value,
    response_chunks: Vec<serde_json::Value>,
}

impl OpenAiCompatStreamReceiver {
    fn new(response: reqwest::Response, request_body: serde_json::Value) -> Self {
        Self {
            response,
            parser: SseParser::new(),
            pending_tool_calls: Vec::new(),
            pending_stop_reason: None,
            event_queue: VecDeque::new(),
            request_body,
            response_chunks: Vec::new(),
        }
    }
}

#[async_trait]
impl StreamReceiver for OpenAiCompatStreamReceiver {
    async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>> {
        loop {
            // 1. 先消费缓冲队列
            if let Some(event) = self.event_queue.pop_front() {
                trace!(
                    "[INBOUND] Stream: event from buffer, event_type={}",
                    std::any::type_name_of_val(&event)
                );
                return Ok(Some(event));
            }

            // 2. 尝试先从 parser 消费一个 SSE 帧
            if let Some(raw_event) = self.parser.next_raw()? {
                match raw_event {
                    RawSseEvent::Done => {
                        debug!("[INBOUND] Stream: [DONE] received from LLM");
                        self.flush_pending_tool_calls();
                        if let Some(event) = self.event_queue.pop_front() {
                            return Ok(Some(event));
                        }
                        return Ok(None);
                    }
                    RawSseEvent::Data(json_str) => {
                        let response: CreateChatCompletionStreamResponse = serde_json::from_str(&json_str)
                            .map_err(|e| anyhow!("Failed to parse openai-compatible SSE JSON: {}", e))?;
                        if let Ok(json) = serde_json::to_value(&response) {
                            self.response_chunks.push(json);
                        }
                        self.process_response(response);
                        continue;
                    }
                }
            }

            // 3. 拉取新的字节块喂给 parser
            match self.response.chunk().await {
                Ok(Some(chunk)) => self.parser.feed(&chunk),
                Ok(None) => {
                    debug!("[INBOUND] Stream: upstream closed");
                    self.flush_pending_tool_calls();
                    if let Some(event) = self.event_queue.pop_front() {
                        return Ok(Some(event));
                    }
                    return Ok(None);
                }
                Err(e) => return Err(anyhow!("OpenAI stream error: {}", e)),
            }
        }
    }

    fn response_body(&self) -> Option<serde_json::Value> {
        Some(serde_json::Value::Array(self.response_chunks.clone()))
    }

    fn request_body(&self) -> Option<serde_json::Value> {
        Some(self.request_body.clone())
    }
}

impl OpenAiCompatStreamReceiver {
    fn process_response(&mut self, response: CreateChatCompletionStreamResponse) {
        // --- Usage 处理 ---
        if let Some(usage) = response.usage {
            self.event_queue.push_back(ProviderStreamEvent::MessageComplete {
                usage: map_usage(&usage),
                stop_reason: self.pending_stop_reason.take(),
            });
            return;
        }

        let Some(choice) = response.choices.first() else { return };

        // --- finish_reason 处理 ---
        if let Some(reason) = &choice.finish_reason {
            self.pending_stop_reason = Some(map_finish_reason(reason));
        }

        let delta = &choice.delta;

        // --- Text content ---
        if let Some(content) = &delta.content {
            if !content.is_empty() {
                self.event_queue
                    .push_back(ProviderStreamEvent::TextDelta(content.clone()));
            }
        }

        // --- Tool calls 增量组装 ---
        if let Some(tool_calls) = &delta.tool_calls {
            for tc in tool_calls {
                let idx = tc.index as usize;
                while self.pending_tool_calls.len() <= idx {
                    self.pending_tool_calls.push(None);
                }

                if let Some(id) = &tc.id {
                    let name = tc
                        .function
                        .as_ref()
                        .and_then(|f| f.name.as_ref())
                        .cloned()
                        .unwrap_or_default();
                    self.pending_tool_calls[idx] = Some(PendingToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments_buffer: String::new(),
                    });
                    self.event_queue
                        .push_back(ProviderStreamEvent::ToolUseStart { id: id.clone(), name });
                }

                // 追加 arguments 增量
                if let Some(func) = &tc.function {
                    if let Some(args) = &func.arguments {
                        if !args.is_empty() {
                            if let Some(Some(pending)) = self.pending_tool_calls.get_mut(idx) {
                                pending.arguments_buffer.push_str(args);
                            }
                            self.event_queue
                                .push_back(ProviderStreamEvent::ToolUseInputDelta(args.clone()));
                        }
                    }
                }
            }
        }
    }

    /// 在流结束时（[DONE] 或 finish_reason=tool_calls），关闭所有未完成的 tool calls
    fn flush_pending_tool_calls(&mut self) {
        let count = self.pending_tool_calls.iter().filter(|p| p.is_some()).count();
        for _ in 0..count {
            self.event_queue.push_back(ProviderStreamEvent::ToolUseEnd);
        }
        self.pending_tool_calls.clear();

        // 如果还有未发射的 MessageComplete
        if let Some(reason) = self.pending_stop_reason.take() {
            self.event_queue.push_back(ProviderStreamEvent::MessageComplete {
                usage: Usage::default(),
                stop_reason: Some(reason),
            });
        }
    }
}
