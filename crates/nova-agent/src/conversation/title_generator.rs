use anyhow::{anyhow, Error as AnyError};
use async_trait::async_trait;
use std::fmt;
use std::sync::Arc;

use crate::config::AppConfig;
use crate::conversation::SessionService;
use crate::message::{ContentBlock, Message, Role};
use crate::network::build_provider_client;
use crate::provider::openai_compat::OpenAiCompatClient;
use crate::provider::types::ProviderRequestContext;
use crate::provider::{LlmClient, ModelConfig, ProviderStreamEvent};

/// 标题生成器错误类型。
///
/// `Retryable` 走可重试路径（`set_failed` 前缀 `retryable:`），状态机下一次用户消息可再次触发。
/// `NonRetryable` 走不可重试路径（前缀 `non_retryable:`），仍计入 `attempt_count`。
#[derive(Debug)]
pub enum TitleGenerationError {
    Retryable(AnyError),
    NonRetryable(AnyError),
}

impl fmt::Display for TitleGenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retryable(err) => write!(f, "{err}"),
            Self::NonRetryable(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for TitleGenerationError {}

/// 标题生成器抽象。
///
/// 实现方按 `session_id` 与 `user_texts` 产出一行 ≤40 字符的标题候选。
/// 调用方对返回值再做 `normalize_generated_title`（取首行、剥引号、截 40 chars），
/// 实现方不必自己 normalize。
#[async_trait]
pub trait TitleGenerator: Send + Sync {
    async fn generate(&self, session_id: &str, user_texts: &[String]) -> Result<String, TitleGenerationError>;
}

/// 默认 fallback 生成器：取首条用户文本（trim 后单行），不调任何外部服务。
///
/// 使用场景：
/// - 单元测试与早期 bootstrap 路径
/// - 宿主未注入 `LlmTitleGenerator` 时的兜底
pub struct FallbackTitleGenerator;

#[async_trait]
impl TitleGenerator for FallbackTitleGenerator {
    async fn generate(&self, _session_id: &str, user_texts: &[String]) -> Result<String, TitleGenerationError> {
        let first = user_texts
            .iter()
            .map(|t| t.trim())
            .find(|t| !t.is_empty())
            .ok_or_else(|| TitleGenerationError::NonRetryable(anyhow::anyhow!("user texts are empty")))?;
        // 取单行，再交给调用方 normalize 截 40。
        let single_line = first.lines().next().unwrap_or("").trim().to_string();
        if single_line.is_empty() {
            return Err(TitleGenerationError::NonRetryable(anyhow::anyhow!(
                "first user text reduces to empty after single-line trim"
            )));
        }
        Ok(single_line)
    }
}

// ---------------------------------------------------------------------------
// LlmTitleGenerator: 复用当前 session active agent 的 binding 调一次 LLM
// ---------------------------------------------------------------------------

const TITLE_SYSTEM_PROMPT: &str = "你是一个会话标题生成器。读用户消息后输出一行不超过 40 个字符的中文短摘要，\
仅输出标题文本本身，禁止使用引号、Markdown、emoji、句末标点。";

const TITLE_USER_PROMPT_TEMPLATE: &str = "用户消息：\n{}\n\n请只输出一行短标题。";

const TITLE_MAX_TOKENS: u32 = 80;
const TITLE_TEMPERATURE: f32 = 0.2;

/// 使用 LLM 生成会话标题。每次调用按 `session_id` 反查当前 session 的 active agent，
/// 用该 agent 的 provider/model binding 即时构造 [`OpenAiCompatClient`]。
///
/// **不缓存 client**：保证 `AppConfig` 热更新场景下立即生效。
pub struct LlmTitleGenerator {
    config: Arc<AppConfig>,
    sessions: Arc<SessionService>,
}

impl LlmTitleGenerator {
    pub fn new(config: Arc<AppConfig>, sessions: Arc<SessionService>) -> Self {
        Self { config, sessions }
    }
}

#[async_trait]
impl TitleGenerator for LlmTitleGenerator {
    async fn generate(&self, session_id: &str, user_texts: &[String]) -> Result<String, TitleGenerationError> {
        if user_texts.iter().all(|t| t.trim().is_empty()) {
            return Err(TitleGenerationError::NonRetryable(anyhow!("user texts are empty")));
        }

        let session = self
            .sessions
            .get(session_id)
            .await
            .map_err(|e| TitleGenerationError::NonRetryable(e.context("lookup session for title generation")))?
            .ok_or_else(|| TitleGenerationError::NonRetryable(anyhow!("session not found: {session_id}")))?;
        let agent_id = session.get_active_agent().await;

        let binding = self
            .config
            .resolve_agent_binding_by_id(&agent_id)
            .map_err(|e| TitleGenerationError::NonRetryable(e.context("resolve agent binding")))?;
        let http_client =
            build_provider_client().map_err(|e| TitleGenerationError::Retryable(e.context("build http client")))?;
        let client = OpenAiCompatClient::from_registry_with_http_client(
            self.config.providers.clone(),
            binding.provider_id.clone(),
            http_client,
        );

        let mut model_config: ModelConfig = binding.model_config.clone().into();
        model_config.max_tokens = TITLE_MAX_TOKENS;
        model_config.temperature = Some(TITLE_TEMPERATURE);
        model_config.thinking_budget = None;
        model_config.reasoning_effort = None;

        let joined_user_text = user_texts
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n---\n");
        let user_prompt = TITLE_USER_PROMPT_TEMPLATE.replacen("{}", &joined_user_text, 1);
        let messages = vec![
            Message::new(
                Role::System,
                vec![ContentBlock::Text {
                    text: TITLE_SYSTEM_PROMPT.to_string(),
                }],
                chrono::Utc::now().timestamp_millis(),
            ),
            Message::new(
                Role::User,
                vec![ContentBlock::Text { text: user_prompt }],
                chrono::Utc::now().timestamp_millis(),
            ),
        ];
        let request_context = ProviderRequestContext {
            session_id: Some(session_id.to_string()),
            agent_id: format!("title-gen[{agent_id}]"),
            message_id: uuid::Uuid::new_v4().to_string(),
        };

        run_title_stream(&client, &messages, &model_config, &request_context).await
    }
}

/// 调用任意 `LlmClient` 的 stream 接口，累加 `TextDelta` 直到 `MessageComplete`。
///
/// 抽出来便于用 mock `LlmClient` 单测；`LlmTitleGenerator::generate` 仅负责
/// session lookup → binding 解析 → 构造 client + ModelConfig，剩下的交给本函数。
async fn run_title_stream(
    client: &dyn LlmClient,
    messages: &[Message],
    model_config: &ModelConfig,
    request_context: &ProviderRequestContext,
) -> Result<String, TitleGenerationError> {
    let mut stream = client
        .stream(messages, &[], model_config, request_context)
        .await
        .map_err(classify_error)?;

    let mut buffer = String::new();
    loop {
        let event = stream.next_event().await.map_err(classify_error)?;
        match event {
            Some(ProviderStreamEvent::TextDelta(delta)) => buffer.push_str(&delta),
            Some(ProviderStreamEvent::MessageComplete { .. }) => break,
            Some(_) => continue,
            None => break,
        }
    }
    if buffer.trim().is_empty() {
        return Err(TitleGenerationError::NonRetryable(anyhow!("llm returned empty title")));
    }
    Ok(buffer)
}

/// 把底层错误按字符串关键字判定是否 retryable。
///
/// 未识别的错误一律 `Retryable`，避免单次抖动卡死状态机；下一次用户消息可重试。
fn classify_error(err: AnyError) -> TitleGenerationError {
    let msg = err.to_string().to_ascii_lowercase();
    let non_retryable_keywords = ["unauthorized", "invalid api key", "forbidden", "not found", "404"];
    if non_retryable_keywords.iter().any(|kw| msg.contains(kw)) {
        TitleGenerationError::NonRetryable(err)
    } else {
        TitleGenerationError::Retryable(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fallback_returns_first_non_empty_user_message() {
        let gen = FallbackTitleGenerator;
        let texts = vec!["  ".to_string(), "我想做一个桌面端任务调度工具".to_string()];
        let out = gen.generate("sess-1", &texts).await.unwrap();
        assert_eq!(out, "我想做一个桌面端任务调度工具");
    }

    #[tokio::test]
    async fn fallback_takes_only_first_line() {
        let gen = FallbackTitleGenerator;
        let texts = vec!["第一行\n第二行".to_string()];
        let out = gen.generate("sess-1", &texts).await.unwrap();
        assert_eq!(out, "第一行");
    }

    #[tokio::test]
    async fn fallback_rejects_empty_input() {
        let gen = FallbackTitleGenerator;
        let err = gen.generate("sess-1", &[]).await.unwrap_err();
        assert!(matches!(err, TitleGenerationError::NonRetryable(_)));
    }

    #[tokio::test]
    async fn fallback_rejects_whitespace_only_input() {
        let gen = FallbackTitleGenerator;
        let texts = vec!["   \n   ".to_string()];
        let err = gen.generate("sess-1", &texts).await.unwrap_err();
        assert!(matches!(err, TitleGenerationError::NonRetryable(_)));
    }

    // -----------------------------------------------------------------------
    // run_title_stream + classify_error 单元测试
    // -----------------------------------------------------------------------

    use crate::provider::types::{StopReason, ToolDefinition, Usage};
    use crate::provider::StreamReceiver;

    struct MockLlmClient {
        events: std::sync::Mutex<Option<Vec<ProviderStreamEvent>>>,
        stream_error: std::sync::Mutex<Option<anyhow::Error>>,
    }

    impl MockLlmClient {
        fn with_events(events: Vec<ProviderStreamEvent>) -> Self {
            Self {
                events: std::sync::Mutex::new(Some(events)),
                stream_error: std::sync::Mutex::new(None),
            }
        }
        fn with_stream_error(err: anyhow::Error) -> Self {
            Self {
                events: std::sync::Mutex::new(Some(Vec::new())),
                stream_error: std::sync::Mutex::new(Some(err)),
            }
        }
    }

    struct MockStream {
        events: std::collections::VecDeque<ProviderStreamEvent>,
    }

    #[async_trait]
    impl StreamReceiver for MockStream {
        async fn next_event(&mut self) -> anyhow::Result<Option<ProviderStreamEvent>> {
            Ok(self.events.pop_front())
        }
    }

    #[async_trait]
    impl LlmClient for MockLlmClient {
        async fn stream(
            &self,
            _messages: &[Message],
            _tools: &[ToolDefinition],
            _config: &ModelConfig,
            _request_context: &ProviderRequestContext,
        ) -> anyhow::Result<Box<dyn StreamReceiver>> {
            if let Some(err) = self.stream_error.lock().unwrap().take() {
                return Err(err);
            }
            let events = self.events.lock().unwrap().take().unwrap_or_default();
            Ok(Box::new(MockStream { events: events.into() }))
        }
    }

    fn default_model_config() -> ModelConfig {
        ModelConfig {
            provider: Some("p".into()),
            model: "m".into(),
            max_tokens: 80,
            temperature: Some(0.2),
            top_p: None,
            thinking_budget: None,
            reasoning_effort: None,
            max_tokens_field: "both".into(),
            extra_body: None,
        }
    }

    fn ctx() -> ProviderRequestContext {
        ProviderRequestContext {
            session_id: Some("sess-1".into()),
            agent_id: "title-gen[zero]".into(),
            message_id: "msg-test".into(),
        }
    }

    #[tokio::test]
    async fn run_title_stream_accumulates_text_deltas() {
        let client = MockLlmClient::with_events(vec![
            ProviderStreamEvent::TextDelta("路由".into()),
            ProviderStreamEvent::TextDelta("设计".into()),
            ProviderStreamEvent::MessageComplete {
                usage: Usage::default(),
                stop_reason: Some(StopReason::EndTurn),
            },
        ]);
        let out = run_title_stream(&client, &[], &default_model_config(), &ctx())
            .await
            .unwrap();
        assert_eq!(out, "路由设计");
    }

    #[tokio::test]
    async fn run_title_stream_returns_non_retryable_when_empty() {
        let client = MockLlmClient::with_events(vec![ProviderStreamEvent::MessageComplete {
            usage: Usage::default(),
            stop_reason: Some(StopReason::EndTurn),
        }]);
        let err = run_title_stream(&client, &[], &default_model_config(), &ctx())
            .await
            .unwrap_err();
        assert!(matches!(err, TitleGenerationError::NonRetryable(_)));
    }

    #[tokio::test]
    async fn run_title_stream_classifies_stream_error_as_retryable() {
        let client = MockLlmClient::with_stream_error(anyhow!("connection reset"));
        let err = run_title_stream(&client, &[], &default_model_config(), &ctx())
            .await
            .unwrap_err();
        assert!(matches!(err, TitleGenerationError::Retryable(_)));
    }

    #[tokio::test]
    async fn classify_error_recognizes_unauthorized_as_non_retryable() {
        let err = classify_error(anyhow!("HTTP 401 Unauthorized"));
        assert!(matches!(err, TitleGenerationError::NonRetryable(_)));
    }

    #[tokio::test]
    async fn classify_error_recognizes_404_as_non_retryable() {
        let err = classify_error(anyhow!("model not found (404)"));
        assert!(matches!(err, TitleGenerationError::NonRetryable(_)));
    }

    #[tokio::test]
    async fn classify_error_defaults_to_retryable() {
        let err = classify_error(anyhow!("some weird i/o glitch"));
        assert!(matches!(err, TitleGenerationError::Retryable(_)));
    }
}
