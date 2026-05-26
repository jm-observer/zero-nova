use nova_agent::config::{
    AgentSpec, AppConfig, ConfiguredAgentModel, ConfiguredModel, ProviderConfig, RegisteredLlmConfig,
};
use nova_agent::conversation::cache::SessionCache;
use nova_agent::conversation::repository::SqliteSessionRepository;
use nova_agent::conversation::sqlite_manager::SqliteManager;
use nova_agent::conversation::{LlmTitleGenerator, SessionService};
use nova_agent::message::{ContentBlock, Role};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn build_app_config(provider_base_url: String) -> Arc<AppConfig> {
    let mut config = AppConfig::default();

    // 覆盖默认 provider 指向 wiremock
    config.providers.insert(
        "default".to_string(),
        ProviderConfig {
            api_key: "sk-test".to_string(),
            base_url: provider_base_url,
        },
    );

    // 覆盖默认 llm 配置
    config.llms.insert(
        "default".to_string(),
        RegisteredLlmConfig {
            provider: "default".to_string(),
            model_config: ConfiguredModel {
                provider: Some("default".to_string()),
                model: "test-model".to_string(),
                max_tokens: 256,
                temperature: Some(0.0),
                top_p: None,
                thinking_budget: None,
                reasoning_effort: None,
                max_tokens_field: "both".to_string(),
                extra_body: None,
            },
        },
    );

    // 注册一个 agent
    config.gateway.agents.push(AgentSpec {
        id: "zero".to_string(),
        display_name: "Zero".to_string(),
        description: "test agent".to_string(),
        aliases: Vec::new(),
        provider: "default".to_string(),
        llm: "default".to_string(),
        prompt_file: None,
        prompt_inline: None,
        system_prompt_template: None,
        model_config: ConfiguredAgentModel {
            model: "test-model".to_string(),
            temperature: 0.0,
            max_tokens: Some(256),
            top_p: 1.0,
        },
        enable_project_developer_prompt: false,
    });

    Arc::new(config)
}

fn mock_chat_completion_body(content: &str) -> String {
    // OpenAI-compatible SSE 格式：两个 chunk —— 一个含 delta.content，一个 finish_reason，然后 [DONE]。
    let chunk_content = format!(
        r#"{{"id":"chatcmpl-test","object":"chat.completion.chunk","created":1,"model":"test-model","choices":[{{"index":0,"delta":{{"content":"{content}"}},"finish_reason":null}}]}}"#,
    );
    let chunk_stop = r#"{"id":"chatcmpl-test","object":"chat.completion.chunk","created":1,"model":"test-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;
    format!("data: {chunk_content}\n\ndata: {chunk_stop}\n\ndata: [DONE]\n\n")
}

#[tokio::test]
async fn llm_title_generator_e2e_writes_title_from_mock_provider() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_string(mock_chat_completion_body("路由设计")))
        .mount(&mock_server)
        .await;

    let dir = tempdir().unwrap();
    let manager = SqliteManager::new(dir.path()).await.unwrap();
    let repository = SqliteSessionRepository::new(manager.pool.clone());
    let mut sessions = SessionService::new(Arc::new(SessionCache::new()), repository);

    let config = build_app_config(mock_server.uri());
    let sessions_arc = Arc::new(sessions.clone());
    sessions.set_title_generator(Arc::new(LlmTitleGenerator::new(config, sessions_arc)));

    let session = sessions
        .create_for_agent(None, "zero".to_string(), String::new(), None)
        .await
        .unwrap();

    sessions
        .append_message(
            &session.id,
            Role::User,
            vec![ContentBlock::Text {
                text: "我想做一个桌面端任务调度工具".to_string(),
            }],
            None,
        )
        .await
        .unwrap();
    sessions
        .append_message(
            &session.id,
            Role::User,
            vec![ContentBlock::Text {
                text: "要支持重试队列并且按项目分类展示".to_string(),
            }],
            None,
        )
        .await
        .unwrap();

    // title generation 是 tokio::spawn，等异步任务跑完
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let state = session.title_state.read().await;
        if !matches!(
            state.status,
            nova_agent::conversation::control::TitleStatus::Pending
                | nova_agent::conversation::control::TitleStatus::Idle
        ) {
            break;
        }
    }

    let final_state = session.title_state.read().await;
    assert_eq!(
        final_state.status,
        nova_agent::conversation::control::TitleStatus::Succeeded,
        "expected Succeeded, got {:?}, last_error={:?}",
        final_state.status,
        final_state.last_error
    );
    drop(final_state);
    assert_eq!(session.get_name().await, "路由设计");
}

#[tokio::test]
async fn llm_title_generator_e2e_marks_failed_when_provider_5xx() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("upstream error"))
        .mount(&mock_server)
        .await;

    let dir = tempdir().unwrap();
    let manager = SqliteManager::new(dir.path()).await.unwrap();
    let repository = SqliteSessionRepository::new(manager.pool.clone());
    let mut sessions = SessionService::new(Arc::new(SessionCache::new()), repository);

    let config = build_app_config(mock_server.uri());
    let sessions_arc = Arc::new(sessions.clone());
    sessions.set_title_generator(Arc::new(LlmTitleGenerator::new(config, sessions_arc)));

    let session = sessions
        .create_for_agent(None, "zero".to_string(), String::new(), None)
        .await
        .unwrap();

    sessions
        .append_message(
            &session.id,
            Role::User,
            vec![ContentBlock::Text {
                text: "我想做一个桌面端任务调度工具".to_string(),
            }],
            None,
        )
        .await
        .unwrap();
    sessions
        .append_message(
            &session.id,
            Role::User,
            vec![ContentBlock::Text {
                text: "要支持重试队列并且按项目分类展示".to_string(),
            }],
            None,
        )
        .await
        .unwrap();

    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let state = session.title_state.read().await;
        if !matches!(
            state.status,
            nova_agent::conversation::control::TitleStatus::Pending
                | nova_agent::conversation::control::TitleStatus::Idle
        ) {
            break;
        }
    }

    let final_state = session.title_state.read().await;
    assert_eq!(
        final_state.status,
        nova_agent::conversation::control::TitleStatus::Failed
    );
    assert!(final_state
        .last_error
        .as_deref()
        .map(|e| e.starts_with("retryable:"))
        .unwrap_or(false));
}
