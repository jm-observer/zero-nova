/// 验证 agent_id 与 session_id 从 runtime 层正确透传至 LLM client.stream()
use anyhow::Result;
use async_trait::async_trait;
use nova_agent::agent::{AgentConfig, AgentRuntime, PromptDiagnosticsConfig, ToolResultCompactionConfig};
use nova_agent::loop_guard::LoopGuardConfig;
use nova_agent::message::Message;
use nova_agent::prompt::TrimmerConfig;
use nova_agent::provider::types::{ProviderRequestContext, ToolDefinition};
use nova_agent::provider::{LlmClient, ModelConfig, ProviderStreamEvent, StreamReceiver};
use nova_agent::tool::ToolRegistry;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

struct CapturingClient {
    captured: Arc<Mutex<Vec<ProviderRequestContext>>>,
}

#[async_trait]
impl LlmClient for CapturingClient {
    async fn stream(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _config: &ModelConfig,
        request_context: &ProviderRequestContext,
    ) -> Result<Box<dyn StreamReceiver>> {
        if let Ok(mut guard) = self.captured.lock() {
            guard.push(request_context.clone());
        }

        struct DoneReceiver {
            done: bool,
        }
        #[async_trait]
        impl StreamReceiver for DoneReceiver {
            async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>> {
                if self.done {
                    return Ok(None);
                }
                self.done = true;
                Ok(Some(ProviderStreamEvent::MessageComplete {
                    usage: Default::default(),
                    stop_reason: None,
                }))
            }
        }
        Ok(Box::new(DoneReceiver { done: false }))
    }
}

fn build_runtime(client: impl LlmClient + 'static) -> AgentRuntime {
    let config = AgentConfig {
        max_iterations: 2,
        model_config: ModelConfig {
            provider: None,
            model: "test".to_string(),
            max_tokens: 1024,
            temperature: None,
            top_p: None,
            thinking_budget: None,
            reasoning_effort: None,
            max_tokens_field: "both".to_string(),
            extra_body: None,
        },
        tool_timeout: std::time::Duration::from_secs(10),
        max_tokens: 1000,
        trimmer: TrimmerConfig::default(),
        config_dir: std::path::PathBuf::from(""),
        prompts_dir: std::path::PathBuf::from(""),
        project_context_file: None,
        initial_env_snapshot: None,
        loop_guard: LoopGuardConfig::default(),
        prompt_diagnostics: PromptDiagnosticsConfig {
            enabled: false,
            large_section_chars: 8_000,
            large_message_chars: 12_000,
            large_tool_result_chars: 8_000,
        },
        tool_result_compaction: ToolResultCompactionConfig {
            enabled: false,
            max_chars: 12_000,
            head_chars: 4_000,
            tail_chars: 4_000,
            disable_for_tools: std::collections::HashSet::new(),
        },
    };
    AgentRuntime::new(client, ToolRegistry::new(), config)
}

#[tokio::test]
async fn 主agent的agent_id透传至stream() -> Result<()> {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let client = CapturingClient {
        captured: Arc::clone(&captured),
    };
    let runtime = build_runtime(client);
    let (tx, _rx) = mpsc::channel(64);

    runtime
        .run_turn(&[], "hello", "test-session", "nova", None, tx, None)
        .await?;

    let records = captured.lock().map(|g| g.clone()).unwrap_or_default();
    let ctx = records.first().expect("stream() 至少被调用一次");
    assert_eq!(ctx.agent_id, "nova", "agent_id 未透传");
    assert_eq!(ctx.session_id.as_deref(), Some("test-session"), "session_id 未透传");
    Ok(())
}

#[tokio::test]
async fn 不同agent_id的turn互不干扰() -> Result<()> {
    let captured_a = Arc::new(Mutex::new(Vec::new()));
    let captured_b = Arc::new(Mutex::new(Vec::new()));

    let client_a = CapturingClient {
        captured: Arc::clone(&captured_a),
    };
    let client_b = CapturingClient {
        captured: Arc::clone(&captured_b),
    };

    let runtime_a = build_runtime(client_a);
    let runtime_b = build_runtime(client_b);

    let (tx_a, _rx_a) = mpsc::channel(64);
    let (tx_b, _rx_b) = mpsc::channel(64);

    runtime_a
        .run_turn(&[], "hello", "sess-a", "agent-alpha", None, tx_a, None)
        .await?;
    runtime_b
        .run_turn(&[], "hello", "sess-b", "agent-beta", None, tx_b, None)
        .await?;

    let recs_a = captured_a.lock().map(|g| g.clone()).unwrap_or_default();
    let recs_b = captured_b.lock().map(|g| g.clone()).unwrap_or_default();

    let ctx_a = recs_a.first().expect("runtime_a stream() 至少被调用一次");
    let ctx_b = recs_b.first().expect("runtime_b stream() 至少被调用一次");

    assert_eq!(ctx_a.agent_id, "agent-alpha");
    assert_eq!(ctx_a.session_id.as_deref(), Some("sess-a"));
    assert_eq!(ctx_b.agent_id, "agent-beta");
    assert_eq!(ctx_b.session_id.as_deref(), Some("sess-b"));
    Ok(())
}
