use crate::provider::openai_compat::types::ChatCompletionChunk;
use crate::provider::sse::{RawSseEvent, SseParser};
use crate::provider::types::{StopReason, ToolDefinition};
use crate::provider::{LlmClient, ModelConfig, ProviderStreamEvent, StreamReceiver};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::{header, Client};
use serde_json::json;
use std::collections::VecDeque;

pub mod types;

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

        // 3.6.4 System Prompt Adaptation
        if !system.is_empty() {
            input_messages.push(json!({
                "role": "system",
                "content": system
            }));
        }

        for msg in messages {
            let role = match msg.role {
                crate::message::Role::User => "user",
                crate::message::Role::Assistant => "assistant",
            };

            let mut text_parts = Vec::new();
            let mut tool_calls = Vec::new();
            let mut tool_results = Vec::new();

            for block in &msg.content {
                match block {
                    crate::message::ContentBlock::Text { text } => {
                        text_parts.push(text.clone());
                    }
                    crate::message::ContentBlock::Thinking { .. } => {
                        // 3.6.2 OpenAI compatibility: Skip thinking for requests
                        continue;
                    }
                    crate::message::ContentBlock::ToolUse { id, name, input } => {
                        tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": input.to_string()
                            }
                        }));
                    }
                    crate::message::ContentBlock::ToolResult {
                        tool_use_id, output, ..
                    } => {
                        tool_results.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": output
                        }));
                    }
                }
            }

            // 3.6.3 OpenAI Tool Call Adaptation
            if !tool_results.is_empty() {
                // OpenAI requires each tool result to be a separate message with role "tool"
                for tr in tool_results {
                    input_messages.push(tr);
                }
                // If there's also text in this message (rare), add it as a user message
                if !text_parts.is_empty() {
                    input_messages.push(json!({
                        "role": "user",
                        "content": text_parts.join("\n")
                    }));
                }
            } else if role == "assistant" && !tool_calls.is_empty() {
                // Assistant message with tool calls
                let mut assistant_msg = json!({
                    "role": "assistant"
                });
                if !text_parts.is_empty() {
                    assistant_msg["content"] = json!(text_parts.join("\n"));
                } else {
                    assistant_msg["content"] = json!(null);
                }
                assistant_msg["tool_calls"] = json!(tool_calls);
                input_messages.push(assistant_msg);
            } else {
                // Regular user or assistant message
                let content = text_parts.join("\n");
                if !content.is_empty() || role == "user" {
                    input_messages.push(json!({
                        "role": role,
                        "content": if content.is_empty() { json!(null) } else { json!(content) }
                    }));
                }
            }
        }

        let mut body = json!({
            "model": config.model,
            "messages": input_messages,
            "stream": true,
        });

        if !tools.is_empty() {
            body["tools"] = json!(tools
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
                .collect::<Vec<_>>());
        }

        if let Some(temp) = config.temperature {
            body["temperature"] = json!(temp);
        }
        if let Some(top_p) = config.top_p {
            body["top_p"] = json!(top_p);
        }
        body["max_tokens"] = json!(config.max_tokens);

        // Phase 4a: stream_options to get usage
        body["stream_options"] = json!({ "include_usage": true });

        if let Some(effort) = &config.reasoning_effort {
            body["reasoning_effort"] = json!(effort);
        }

        // Phase 4b: Support generic reasoning toggle for models like Gemma 4 or DeepSeek R1
        if config.thinking_budget.is_some() {
            body["enable_thinking"] = json!(true);
            body["include_reasoning"] = json!(true);
        }

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
            pending_tool_calls: Vec::new(),
            pending_stop_reason: None,
            event_queue: VecDeque::new(),
        }))
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
}

#[async_trait]
impl StreamReceiver for OpenAiCompatStreamReceiver {
    async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>> {
        loop {
            // 1. 先消费缓冲队列
            if let Some(event) = self.event_queue.pop_front() {
                return Ok(Some(event));
            }

            // 2. 从 SSE 帧中取原始 JSON
            match self.parser.next_raw()? {
                Some(RawSseEvent::Done) => {
                    // [DONE] 信号：发射所有未关闭的 tool calls 的 End 事件，再发 MessageComplete
                    self.flush_pending_tool_calls();
                    return Ok(self.event_queue.pop_front());
                }
                Some(RawSseEvent::Data(json_str)) => {
                    // Check for error in JSON
                    if let Ok(err_obj) = serde_json::from_str::<serde_json::Value>(&json_str) {
                        if let Some(error) = err_obj.get("error") {
                            let msg = error.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error");
                            return Err(anyhow!("OpenAI API Error: {}", msg));
                        }
                    }

                    let chunk: ChatCompletionChunk = serde_json::from_str(&json_str)
                        .map_err(|e| anyhow!("Failed to parse OpenAI chunk: {}, content: {}", e, json_str))?;
                    self.process_chunk(chunk);
                    // 回到循环顶部消费 event_queue
                    continue;
                }
                None => {
                    // 缓冲区中没有完整帧，读取更多数据
                }
            }

            // 3. 从 HTTP response 读取更多数据
            match self.response.chunk().await? {
                Some(bytes) => self.parser.feed(&bytes),
                None => return Ok(None),
            }
        }
    }
}

impl OpenAiCompatStreamReceiver {
    fn process_chunk(&mut self, chunk: ChatCompletionChunk) {
        // --- Usage 处理 ---
        if let Some(usage) = chunk.usage {
            self.event_queue.push_back(ProviderStreamEvent::MessageComplete {
                usage: crate::provider::types::Usage {
                    input_tokens: usage.prompt_tokens,
                    output_tokens: usage.completion_tokens,
                    cache_creation_input_tokens: 0,
                    cache_read_input_tokens: 0,
                },
                stop_reason: self.pending_stop_reason.take(),
            });
            return;
        }

        let Some(choice) = chunk.choices.first() else { return };

        // --- finish_reason 处理 ---
        if let Some(reason) = &choice.finish_reason {
            self.pending_stop_reason = Some(match reason.as_str() {
                "stop" => StopReason::EndTurn,
                "length" => StopReason::MaxTokens,
                "tool_calls" => StopReason::ToolUse,
                _ => StopReason::Unknown,
            });
        }

        let delta = &choice.delta;

        // --- Reasoning content（优先于普通 text）---
        let reasoning = delta.reasoning_content.as_ref().or(delta.reasoning_alias.as_ref()); // fallback 到 alias
        if let Some(reasoning) = reasoning {
            if !reasoning.is_empty() {
                self.event_queue
                    .push_back(ProviderStreamEvent::ThinkingDelta(reasoning.clone()));
            }
        }

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
                let idx = tc.index;
                // 确保 pending_tool_calls 容量足够
                while self.pending_tool_calls.len() <= idx {
                    self.pending_tool_calls.push(None);
                }

                if let Some(id) = &tc.id {
                    // 新 tool call 的首个 chunk：发射 ToolUseStart
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
                usage: crate::provider::types::Usage::default(),
                stop_reason: Some(reason),
            });
        }
    }
}
