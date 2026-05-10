use anyhow::Result;
use nova_agent::provider::openai_compat::OpenAiCompatClient;
use nova_agent::provider::types::ProviderRequestContext;
use nova_agent::provider::{LlmClient, ModelConfig};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Clone, Debug, Default)]
struct RequestContextCapture {
    session_id: Option<String>,
    agent_id: Option<String>,
}

async fn create_mock_server_with_capture(
) -> (MockServer, Arc<Mutex<Vec<RequestContextCapture>>>) {
    let mock_server = MockServer::start().await;
    let captured = Arc::new(Mutex::new(Vec::new()));

    let captured_clone = Arc::clone(&captured);
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |req: &wiremock::Request| {
            let mut record = RequestContextCapture::default();
            if let Some(values) = req.headers.get("x-session-id") {
                record.session_id = values.first().map(ToString::to_string);
            }
            if let Some(values) = req.headers.get("x-agent-id") {
                record.agent_id = values.first().map(ToString::to_string);
            }

            if let Ok(mut guard) = captured_clone.lock() {
                guard.push(record);
            }

            ResponseTemplate::new(200).set_body_string(
                r#"{
                    "id":"chatcmpl-test",
                    "object":"chat.completion.chunk",
                    "created":1234567890,
                    "model":"gpt-4o-mini",
                    "choices":[
                        {
                            "index":0,
                            "delta":{"content":"hello"},
                            "finish_reason":null
                        }
                    ]
                }"#,
            )
        })
        .mount(&mock_server)
        .await;

    (mock_server, captured)
}

fn make_context(session_id: Option<String>, agent_id: Option<String>) -> ProviderRequestContext {
    ProviderRequestContext { session_id, agent_id }
}

#[tokio::test]
async fn header_透传开启且字段齐全() -> Result<()> {
    let (mock_server, captured) = create_mock_server_with_capture().await;
    let client =
        OpenAiCompatClient::new_with_context_headers_enabled("sk-test".to_string(), mock_server.uri(), true);
    let ctx = make_context(Some("sess-123".to_string()), Some("agent-456".to_string()));

    let _receiver = client.stream(&[], &[], &ModelConfig::default(), &ctx).await?;

    let records = captured.lock().map(|g| g.clone()).unwrap_or_default();
    let record = records.first().cloned().unwrap_or_default();
    assert_eq!(record.session_id.as_deref(), Some("sess-123"));
    assert_eq!(record.agent_id.as_deref(), Some("agent-456"));
    Ok(())
}

#[tokio::test]
async fn header_透传开启且单字段缺失() -> Result<()> {
    let (mock_server, captured) = create_mock_server_with_capture().await;
    let client =
        OpenAiCompatClient::new_with_context_headers_enabled("sk-test".to_string(), mock_server.uri(), true);
    let ctx = make_context(Some("sess-123".to_string()), None);

    let _receiver = client.stream(&[], &[], &ModelConfig::default(), &ctx).await?;

    let records = captured.lock().map(|g| g.clone()).unwrap_or_default();
    let record = records.first().cloned().unwrap_or_default();
    assert_eq!(record.session_id.as_deref(), Some("sess-123"));
    assert_eq!(record.agent_id, None);
    Ok(())
}

#[tokio::test]
async fn header_透传关闭且字段齐全() -> Result<()> {
    let (mock_server, captured) = create_mock_server_with_capture().await;
    let client =
        OpenAiCompatClient::new_with_context_headers_enabled("sk-test".to_string(), mock_server.uri(), false);
    let ctx = make_context(Some("sess-123".to_string()), Some("agent-456".to_string()));

    let _receiver = client.stream(&[], &[], &ModelConfig::default(), &ctx).await?;

    let records = captured.lock().map(|g| g.clone()).unwrap_or_default();
    let record = records.first().cloned().unwrap_or_default();
    assert_eq!(record.session_id, None);
    assert_eq!(record.agent_id, None);
    Ok(())
}

#[tokio::test]
async fn 并发请求中header与session一一对应() -> Result<()> {
    let (mock_server, captured) = create_mock_server_with_capture().await;
    let mut handles = Vec::new();
    let expected_total = 10usize;

    for i in 0..expected_total {
        let base_url = mock_server.uri();
        handles.push(tokio::spawn(async move {
            let client = OpenAiCompatClient::new_with_context_headers_enabled("sk-test".to_string(), base_url, true);
            let session = format!("sess-{i}");
            let ctx = make_context(Some(session.clone()), Some("agent-001".to_string()));
            let _receiver = client.stream(&[], &[], &ModelConfig::default(), &ctx).await?;
            Ok::<String, anyhow::Error>(session)
        }));
    }

    let mut expected_sessions = Vec::new();
    for handle in handles {
        expected_sessions.push(handle.await??);
    }

    let records = captured.lock().map(|g| g.clone()).unwrap_or_default();
    let mut seen = HashMap::new();
    for record in records.iter() {
        if let Some(session_id) = &record.session_id {
            seen.insert(session_id.clone(), true);
        }
    }

    for session in expected_sessions {
        assert!(seen.get(&session).copied().unwrap_or(false), "missing session header: {session}");
    }
    Ok(())
}

#[tokio::test]
async fn 上游返回4xx时错误链路可观测() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(400).set_body_string(r#"{"error":"bad request"}"#))
        .mount(&mock_server)
        .await;

    let client = OpenAiCompatClient::new_with_context_headers_enabled(
        "sk-test".to_string(),
        mock_server.uri(),
        true,
    );
    let ctx = make_context(Some("sess-123".to_string()), Some("agent-456".to_string()));
    let result = client.stream(&[], &[], &ModelConfig::default(), &ctx).await;

    assert!(result.is_err());
    let message = result.err().map(|err| err.to_string()).unwrap_or_default();
    assert!(message.contains("response error status") || message.contains("OpenAI-compatible response error status"));
}
