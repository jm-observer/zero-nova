//! 内部类型与 async-openai 类型之间的双向转换函数。

use async_openai::types::chat::{
    ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls, ChatCompletionRequestAssistantMessage,
    ChatCompletionRequestAssistantMessageContent, ChatCompletionRequestMessage,
    ChatCompletionRequestMessageContentPartImage, ChatCompletionRequestMessageContentPartText,
    ChatCompletionRequestSystemMessage, ChatCompletionRequestSystemMessageContent, ChatCompletionRequestToolMessage,
    ChatCompletionRequestToolMessageContent, ChatCompletionRequestUserMessage, ChatCompletionRequestUserMessageContent,
    ChatCompletionRequestUserMessageContentPart, ChatCompletionStreamOptions, ChatCompletionTool, ChatCompletionTools,
    CompletionUsage, CreateChatCompletionRequest, FinishReason, FunctionCall, FunctionObject, ImageDetail, ImageUrl,
    ReasoningEffort,
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
                // User 消息：处理 Text / Image / ToolResult。
                // Image 出现时使用 Array content（vision part）；纯文本仍走 Text content。
                let mut text_parts = Vec::new();
                let mut image_parts: Vec<ChatCompletionRequestMessageContentPartImage> = Vec::new();
                let mut tool_results = Vec::new();

                // 工具回传的图片(ToolResult.images)需要作为**紧跟 tool 消息后**
                // 的合成 user 消息承载——OpenAI 协议下 tool 消息 content part 仅
                // 支持 text(spec 限制 + vLLM/Gemma chat template 也仅支持此形态;
                // 详见 docs/2026-05-29-image-handle-injection/vision-tool-result-design.md
                // §2 的真机验证)。与原 user 消息自带的 inline image(ContentBlock::
                // Image)分开收集,避免顺序错乱。
                let mut tool_images: Vec<ChatCompletionRequestMessageContentPartImage> = Vec::new();
                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text } => {
                            text_parts.push(text.clone());
                        }
                        ContentBlock::Image { mime, data_base64 } => {
                            image_parts.push(ChatCompletionRequestMessageContentPartImage {
                                image_url: ImageUrl {
                                    url: format!("data:{};base64,{}", mime, data_base64),
                                    // detail 必须显式给值：async-openai 的 ImageUrl.detail
                                    // 无 skip_serializing_if，None 会序列化成 "detail": null，
                                    // 严格校验的 OpenAI 兼容服务（pydantic）只接受 auto/low/high。
                                    detail: Some(ImageDetail::Auto),
                                },
                            });
                        }
                        ContentBlock::ToolResult {
                            tool_use_id,
                            output,
                            images,
                            ..
                        } => {
                            tool_results.push((tool_use_id.clone(), output.clone()));
                            for img in images {
                                tool_images.push(ChatCompletionRequestMessageContentPartImage {
                                    image_url: ImageUrl {
                                        url: format!("data:{};base64,{}", img.mime, img.data_base64),
                                        detail: Some(ImageDetail::Auto),
                                    },
                                });
                            }
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
                }

                // 紧跟 tool 消息后:若工具回传了图片,合成一条 user 消息只含 image_url
                // parts 把图喂给模型(spec 限制 tool 消息无法直接承载图;真机实测确认
                // 这是 vLLM/Gemma 接受的唯一可行线格式)。这条合成消息**先于**原 user
                // 消息(text + inline image)发出,保证图与 tool call 在序列上紧邻。
                if !tool_images.is_empty() {
                    let parts: Vec<ChatCompletionRequestUserMessageContentPart> = tool_images
                        .into_iter()
                        .map(ChatCompletionRequestUserMessageContentPart::ImageUrl)
                        .collect();
                    result.push(ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                        content: ChatCompletionRequestUserMessageContent::Array(parts),
                        name: None,
                    }));
                }

                let user_content: Option<ChatCompletionRequestUserMessageContent> = if !image_parts.is_empty() {
                    let mut parts: Vec<ChatCompletionRequestUserMessageContentPart> = Vec::new();
                    if !text_parts.is_empty() {
                        parts.push(ChatCompletionRequestUserMessageContentPart::Text(
                            ChatCompletionRequestMessageContentPartText {
                                text: text_parts.join("\n"),
                            },
                        ));
                    }
                    for img in image_parts {
                        parts.push(ChatCompletionRequestUserMessageContentPart::ImageUrl(img));
                    }
                    Some(ChatCompletionRequestUserMessageContent::Array(parts))
                } else if !text_parts.is_empty() {
                    Some(ChatCompletionRequestUserMessageContent::Text(text_parts.join("\n")))
                } else {
                    None
                };

                if let Some(content) = user_content {
                    result.push(ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                        content,
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
        max_completion_tokens: None,
        temperature: config.temperature,
        top_p: config.top_p,
        reasoning_effort,
        stream: Some(true),
        stream_options: Some(ChatCompletionStreamOptions {
            include_usage: Some(true),
            include_obfuscation: None,
        }),
        // thinking_budget 暂不映射（已确认去掉非标准字段）
        ..Default::default()
    };

    match config.max_tokens_field.as_str() {
        "completion" => {
            request.max_completion_tokens = Some(config.max_tokens);
        }
        "legacy" => {
            #[allow(deprecated)]
            {
                request.max_tokens = Some(config.max_tokens);
            }
        }
        "both" => {
            // 显式配置为 "both" 时同时设置两个字段（兼容模式）
            request.max_completion_tokens = Some(config.max_tokens);
            #[allow(deprecated)]
            {
                request.max_tokens = Some(config.max_tokens);
            }
        }
        _ => {
            // 默认使用 max_completion_tokens（现代 OpenAI-compatible 服务的最佳实践）
            request.max_completion_tokens = Some(config.max_tokens);
        }
    }

    request
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::build_request;
    use crate::provider::ModelConfig;

    fn base_config(max_tokens_field: &str) -> ModelConfig {
        ModelConfig {
            provider: None,
            model: "gpt-oss-120b".to_string(),
            max_tokens: 1024,
            temperature: None,
            top_p: None,
            thinking_budget: None,
            reasoning_effort: None,
            max_tokens_field: max_tokens_field.to_string(),
            extra_body: None,
        }
    }

    #[test]
    fn max_tokens_field_completion_only_sets_completion_tokens() {
        let request = build_request(&[], &[], &base_config("completion"));
        assert_eq!(request.max_completion_tokens, Some(1024));
        #[allow(deprecated)]
        {
            assert!(request.max_tokens.is_none());
        }
    }

    #[test]
    fn max_tokens_field_legacy_only_sets_legacy_tokens() {
        let request = build_request(&[], &[], &base_config("legacy"));
        assert!(request.max_completion_tokens.is_none());
        #[allow(deprecated)]
        {
            assert_eq!(request.max_tokens, Some(1024));
        }
    }

    #[test]
    fn max_tokens_field_both_sets_both_fields() {
        let request = build_request(&[], &[], &base_config("both"));
        assert_eq!(request.max_completion_tokens, Some(1024));
        #[allow(deprecated)]
        {
            assert_eq!(request.max_tokens, Some(1024));
        }
    }

    // ====================================================================
    // P3a-6: ToolResult.images → 合成 user 消息带 image_url 测试
    // 详见 docs/2026-05-29-image-handle-injection/vision-tool-result-design.md
    // ====================================================================
    use super::messages_to_openai;
    use crate::message::{ContentBlock, Message, Role, ToolImage};
    use async_openai::types::chat::{
        ChatCompletionRequestMessage, ChatCompletionRequestToolMessageContent, ChatCompletionRequestUserMessageContent,
        ChatCompletionRequestUserMessageContentPart, ImageDetail,
    };

    /// 带 images 的 ToolResult → 紧跟一条合成 user 消息承载 image_url parts。
    /// OpenAI 协议下 tool 消息 content part 仅支持 text，故图片必须经合成 user
    /// 消息送达。
    #[test]
    fn tool_result_with_image_synthesizes_user_message() {
        let messages = vec![Message::new(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                output: "[图片已读取]".to_string(),
                is_error: false,
                images: vec![ToolImage {
                    mime: "image/jpeg".to_string(),
                    data_base64: "FAKE_B64".to_string(),
                }],
            }],
            0,
        )];
        let out = messages_to_openai(&messages);
        // 期望:1 条 Tool 消息 + 1 条 User 消息(只含 image_url part)
        assert_eq!(out.len(), 2, "应产出 [Tool, User] 两条消息,得到 {out:#?}");
        assert!(
            matches!(out[0], ChatCompletionRequestMessage::Tool(_)),
            "第 1 条应是 Tool 消息"
        );
        match &out[1] {
            ChatCompletionRequestMessage::User(u) => match &u.content {
                ChatCompletionRequestUserMessageContent::Array(parts) => {
                    assert_eq!(parts.len(), 1);
                    match &parts[0] {
                        ChatCompletionRequestUserMessageContentPart::ImageUrl(img) => {
                            assert_eq!(img.image_url.url, "data:image/jpeg;base64,FAKE_B64");
                            assert!(matches!(img.image_url.detail, Some(ImageDetail::Auto)));
                        }
                        other => panic!("应是 ImageUrl part,得到 {other:?}"),
                    }
                }
                other => panic!("应是 Array content,得到 {other:?}"),
            },
            other => panic!("第 2 条应是 User 消息,得到 {other:?}"),
        }
    }

    /// 无 images 的 ToolResult 应保持原行为:只产出 Tool 消息,不合成 user 消息。
    /// 回归保护——避免本次改动意外影响零图场景。
    #[test]
    fn tool_result_without_image_unchanged() {
        let messages = vec![Message::new(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                output: "plain text result".to_string(),
                is_error: false,
                images: Vec::new(),
            }],
            0,
        )];
        let out = messages_to_openai(&messages);
        assert_eq!(out.len(), 1, "无图应只产 1 条 Tool 消息,得到 {out:#?}");
        match &out[0] {
            ChatCompletionRequestMessage::Tool(t) => match &t.content {
                ChatCompletionRequestToolMessageContent::Text(s) => {
                    assert_eq!(s, "plain text result");
                }
                other => panic!("应是 Text content,得到 {other:?}"),
            },
            other => panic!("应是 Tool 消息,得到 {other:?}"),
        }
    }

    /// 多张图(同一 ToolResult)→ 合成 user 消息含多个 image_url part。
    #[test]
    fn tool_result_multiple_images_all_in_synthetic_user() {
        let messages = vec![Message::new(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                output: "[3 images]".to_string(),
                is_error: false,
                images: vec![
                    ToolImage {
                        mime: "image/png".to_string(),
                        data_base64: "AAA".to_string(),
                    },
                    ToolImage {
                        mime: "image/jpeg".to_string(),
                        data_base64: "BBB".to_string(),
                    },
                    ToolImage {
                        mime: "image/webp".to_string(),
                        data_base64: "CCC".to_string(),
                    },
                ],
            }],
            0,
        )];
        let out = messages_to_openai(&messages);
        assert_eq!(out.len(), 2);
        match &out[1] {
            ChatCompletionRequestMessage::User(u) => match &u.content {
                ChatCompletionRequestUserMessageContent::Array(parts) => {
                    assert_eq!(parts.len(), 3, "应有 3 个 image_url parts");
                    for (i, part) in parts.iter().enumerate() {
                        match part {
                            ChatCompletionRequestUserMessageContentPart::ImageUrl(img) => {
                                assert!(img.image_url.url.starts_with("data:image/"));
                                let expected_b64 = ["AAA", "BBB", "CCC"][i];
                                assert!(
                                    img.image_url.url.ends_with(expected_b64),
                                    "第 {i} 张图 URL 应以 {expected_b64} 结尾"
                                );
                            }
                            other => panic!("part {i} 应是 ImageUrl,得到 {other:?}"),
                        }
                    }
                }
                _ => panic!("应是 Array"),
            },
            _ => panic!("第 2 条应是 User"),
        }
    }
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
