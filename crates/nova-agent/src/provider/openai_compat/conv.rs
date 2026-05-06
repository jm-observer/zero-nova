//! 内部类型与 async-openai 类型之间的双向转换函数。

use async_openai::types::chat::{
    ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls, ChatCompletionRequestAssistantMessage,
    ChatCompletionRequestAssistantMessageContent, ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestSystemMessageContent, ChatCompletionRequestToolMessage,
    ChatCompletionRequestToolMessageContent, ChatCompletionRequestUserMessage, ChatCompletionRequestUserMessageContent,
    ChatCompletionStreamOptions, ChatCompletionTool, ChatCompletionTools, CompletionUsage, CreateChatCompletionRequest,
    FinishReason, FunctionCall, FunctionObject, ReasoningEffort,
};

use crate::message::{ContentBlock, Message, Role};
use crate::provider::types::{StopReason, ToolDefinition, Usage};
use crate::provider::ModelConfig;

// ============================================================================
// 请求侧转换
// ============================================================================

/// 将内部 Message 列表转换为 async-openai 的 ChatCompletionRequestMessage 列表
pub fn messages_to_openai(messages: &[Message]) -> Vec<ChatCompletionRequestMessage> {
    let mut result = Vec::new();

    for msg in messages {
        match msg.role {
            Role::System => {
                // System 消息：直接映射
                let text = msg
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    result.push(ChatCompletionRequestMessage::System(
                        ChatCompletionRequestSystemMessage {
                            content: ChatCompletionRequestSystemMessageContent::Text(text),
                            ..Default::default()
                        },
                    ));
                }
            }
            Role::User => {
                // User 消息：处理 Text 和 ToolResult
                let mut text_parts = Vec::new();
                let mut tool_results = Vec::new();

                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text } => {
                            text_parts.push(text.clone());
                        }
                        ContentBlock::ToolResult {
                            tool_use_id, output, ..
                        } => {
                            tool_results.push((tool_use_id.clone(), output.clone()));
                        }
                        _ => {} // 跳过 Thinking
                    }
                }

                // 如果有 ToolResult，每个生成一条独立的 Tool 消息
                if !tool_results.is_empty() {
                    for (tool_call_id, output) in tool_results {
                        result.push(ChatCompletionRequestMessage::Tool(ChatCompletionRequestToolMessage {
                            tool_call_id,
                            content: ChatCompletionRequestToolMessageContent::Text(output),
                        }));
                    }
                    // 如果有文本部分，单独作为 User 消息
                    if !text_parts.is_empty() {
                        result.push(ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                            content: ChatCompletionRequestUserMessageContent::Text(text_parts.join("\n")),
                            name: None,
                        }));
                    }
                } else if !text_parts.is_empty() {
                    // 纯文本 User 消息
                    result.push(ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                        content: ChatCompletionRequestUserMessageContent::Text(text_parts.join("\n")),
                        name: None,
                    }));
                }
            }
            Role::Assistant => {
                // Assistant 消息：处理 Text 和 ToolUse
                let mut text_parts = Vec::new();
                let mut tool_calls = Vec::new();

                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text } => {
                            text_parts.push(text.clone());
                        }
                        ContentBlock::ToolUse { id, name, input, .. } => {
                            tool_calls.push(ChatCompletionMessageToolCalls::Function(
                                ChatCompletionMessageToolCall {
                                    id: id.clone(),
                                    function: FunctionCall {
                                        name: name.clone(),
                                        arguments: input.to_string(),
                                    },
                                },
                            ));
                        }
                        _ => {} // 跳过 Thinking
                    }
                }

                // 如果有 ToolUse，构建带 tool_calls 的 Assistant 消息
                if !tool_calls.is_empty() {
                    let content = if text_parts.is_empty() {
                        None
                    } else {
                        Some(ChatCompletionRequestAssistantMessageContent::Text(
                            text_parts.join("\n"),
                        ))
                    };

                    result.push(ChatCompletionRequestMessage::Assistant(
                        ChatCompletionRequestAssistantMessage {
                            content,
                            refusal: None,
                            name: None,
                            audio: None,
                            tool_calls: Some(tool_calls),
                            ..Default::default()
                        },
                    ));
                } else if !text_parts.is_empty() {
                    // 纯文本 Assistant 消息
                    result.push(ChatCompletionRequestMessage::Assistant(
                        ChatCompletionRequestAssistantMessage {
                            content: Some(ChatCompletionRequestAssistantMessageContent::Text(
                                text_parts.join("\n"),
                            )),
                            refusal: None,
                            name: None,
                            audio: None,
                            tool_calls: None,
                            ..Default::default()
                        },
                    ));
                }
            }
        }
    }

    result
}

/// 将内部 ToolDefinition 列表转换为 async-openai 的 ChatCompletionTools 列表
pub fn tools_to_openai(tools: &[ToolDefinition]) -> Vec<ChatCompletionTools> {
    tools
        .iter()
        .map(|t| {
            ChatCompletionTools::Function(ChatCompletionTool {
                function: FunctionObject {
                    name: t.name.clone(),
                    description: if t.description.is_empty() {
                        None
                    } else {
                        Some(t.description.clone())
                    },
                    parameters: Some(t.input_schema.clone()),
                    strict: None,
                },
            })
        })
        .collect()
}

/// 根据 ModelConfig 构建 CreateChatCompletionRequest
pub fn build_request(
    messages: &[Message],
    tools: &[ToolDefinition],
    config: &ModelConfig,
) -> CreateChatCompletionRequest {
    let openai_messages = messages_to_openai(messages);
    let openai_tools = if tools.is_empty() {
        None
    } else {
        Some(tools_to_openai(tools))
    };

    let reasoning_effort = match config.reasoning_effort.as_deref() {
        Some("minimal") => Some(ReasoningEffort::Minimal),
        Some("low") => Some(ReasoningEffort::Low),
        Some("medium") => Some(ReasoningEffort::Medium),
        Some("high") => Some(ReasoningEffort::High),
        _ => None,
    };

    let mut request = CreateChatCompletionRequest {
        model: config.model.clone(),
        messages: openai_messages,
        tools: openai_tools,
        max_completion_tokens: Some(config.max_tokens),
        temperature: config.temperature.map(|t| t as f32),
        top_p: config.top_p.map(|p| p as f32),
        reasoning_effort,
        stream: Some(true),
        stream_options: Some(ChatCompletionStreamOptions {
            include_usage: Some(true),
            include_obfuscation: None,
        }),
        // thinking_budget 暂不映射（已确认去掉非标准字段）
        ..Default::default()
    };

    // 一些 OpenAI-compatible 服务端仍要求 `max_tokens`，这里保留兼容写法。
    #[allow(deprecated)]
    {
        request.max_tokens = Some(config.max_tokens);
    }

    request
}

// ============================================================================
// 响应侧转换
// ============================================================================

/// 将 async-openai 的 FinishReason 映射为内部 StopReason
pub fn map_finish_reason(reason: &FinishReason) -> StopReason {
    match reason {
        FinishReason::Stop => StopReason::EndTurn,
        FinishReason::Length => StopReason::MaxTokens,
        FinishReason::ToolCalls => StopReason::ToolUse,
        FinishReason::ContentFilter => StopReason::Unknown,
        _ => StopReason::Unknown,
    }
}

/// 将 async-openai 的 CompletionUsage 映射为内部 Usage
pub fn map_usage(usage: &CompletionUsage) -> Usage {
    let cache_read_tokens = usage
        .prompt_tokens_details
        .as_ref()
        .and_then(|details| details.cached_tokens);

    // 序列化原始 CompletionUsage 用于 raw_provider_usage
    let raw_provider_usage = serde_json::to_value(usage).ok();

    Usage {
        input_tokens: usage.prompt_tokens as u64,
        output_tokens: usage.completion_tokens as u64,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: cache_read_tokens.map(|t| t as u64),
        raw_provider_usage,
    }
}
