/// 模拟 LLM 客户端用于集成测试
use nova_core::provider::types::ToolDefinition;
use nova_core::provider::{LlmClient, ModelConfig, ProviderStreamEvent, StreamReceiver};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// 模拟接收器 — 返回预定义的流事件序列
struct MockStreamReceiver {
    events: Arc<Vec<ProviderStreamEvent>>,
    position: Arc<AtomicUsize>,
    done: Arc<AtomicBool>,
}

impl StreamReceiver for MockStreamReceiver {
    async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>> {
        let pos = self.position.fetch_add(1, Ordering::SeqCst);
        if pos >= self.events.len() {
            return Ok(None);
        }
        Ok(Some(self.events[pos].clone()))
    }
}

/// 模拟 LLM 客户端
pub struct MockClient {
    response_text: String,
    has_tool_use: bool,
}

impl MockClient {
    pub fn new(text: &str, has_tool_use: bool) -> Self {
        Self {
            response_text: text.to_string(),
            has_tool_use,
        }
    }
}

#[async_trait]
impl LlmClient for MockClient {
    async fn stream(
        &self,
        _messages: &[nova_core::message::Message],
        _tools: &[ToolDefinition],
        _config: &ModelConfig,
    ) -> Result<Box<dyn StreamReceiver>> {
        let mut events = Vec::new();

        // 添加文本增量
        if !self.response_text.is_empty() {
            events.push(ProviderStreamEvent::TextDelta(self.response_text.clone()));
        }

        // 可选：添加工具使用
        if self.has_tool_use {
            events.push(ProviderStreamEvent::ToolUseStart {
                id: "tool-123".to_string(),
                name: "test_tool".to_string(),
            });
            events.push(ProviderStreamEvent::ToolUseInputDelta("{}".to_string()));
            events.push(ProviderStreamEvent::ToolUseEnd);
        }

        Ok(Box::new(MockStreamReceiver {
            events: Arc::new(events),
            position: Arc::new(AtomicUsize::new(0)),
            done: Arc::new(AtomicBool::new(false)),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::types::StopReason;

    #[tokio::test]
    async fn mock_stream_returns_text_delta() {
        let client = MockClient::new("Hello world", false);
        let stream = client.stream(&[], &[], &ModelConfig::default()).await.unwrap();

        let mut stream = stream;
        if let Some(event) = stream.next_event().await.unwrap() {
            match event {
                ProviderStreamEvent::TextDelta(text) => {
                    assert_eq!(text, "Hello world");
                }
                other => panic!("Expected TextDelta, got {:?}", other),
            }
        }
    }

    #[tokio::test]
    async fn mock_stream_returns_tool_use_start() {
        let client = MockClient::new("", true);
        let stream = client.stream(&[], &[], &ModelConfig::default()).await.unwrap();

        let mut stream = stream;
        // Skip first TextDelta (empty string)
        let _ = stream.next_event().await.unwrap();

        if let Some(event) = stream.next_event().await.unwrap() {
            match event {
                ProviderStreamEvent::ToolUseStart { id, name } => {
                    assert_eq!(id, "tool-123");
                    assert_eq!(name, "test_tool");
                }
                other => panic!("Expected ToolUseStart, got {:?}", other),
            }
        }
    }
}

impl ModelConfig {
    /// 用于测试的默认配置
    pub fn default() -> Self {
        Self {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 1024,
            temperature: Some(0.7),
            top_p: Some(0.9),
            thinking_budget: None,
            reasoning_effort: None,
        }
    }
}
