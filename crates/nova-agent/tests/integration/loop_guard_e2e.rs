use nova_agent::agent::{AgentConfig, AgentRuntime};
use nova_agent::event::AgentEvent;
use nova_agent::loop_guard::LoopGuardConfig;
use nova_agent::message::Message;
use nova_agent::prompt::TrimmerConfig;
use nova_agent::provider::{LlmClient, ModelConfig, ProviderStreamEvent, StreamReceiver};
use nova_agent::tool::ToolRegistry;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

struct StalledClient;

#[async_trait]
impl LlmClient for StalledClient {
    async fn stream(
        &self,
        _messages: &[Message],
        _tools: &[nova_agent::provider::types::ToolDefinition],
        _config: &ModelConfig,
    ) -> Result<Box<dyn StreamReceiver>> {
        struct StalledReceiver { step: usize }
        #[async_trait]
        impl StreamReceiver for StalledReceiver {
            async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>> {
                self.step += 1;
                match self.step {
                    1 => Ok(Some(ProviderStreamEvent::TextDelta("I am stuck in a loop".to_string()))),
                    2 => Ok(Some(ProviderStreamEvent::MessageComplete {
                        usage: Default::default(),
                        stop_reason: None,
                    })),
                    _ => Ok(None)
                }
            }
        }
        Ok(Box::new(StalledReceiver { step: 0 }))
    }
}

struct DuplicateToolClient {
    call_count: Arc<AtomicUsize>
}

#[async_trait]
impl LlmClient for DuplicateToolClient {
    async fn stream(
        &self,
        _messages: &[Message],
        _tools: &[nova_agent::provider::types::ToolDefinition],
        _config: &ModelConfig,
    ) -> Result<Box<dyn StreamReceiver>> {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst);
        struct ToolReceiver { step: usize, id: String }
        #[async_trait]
        impl StreamReceiver for ToolReceiver {
            async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>> {
                self.step += 1;
                match self.step {
                    1 => Ok(Some(ProviderStreamEvent::ToolUseStart {
                        id: self.id.clone(),
                        name: "Read".to_string()
                    })),
                    2 => Ok(Some(ProviderStreamEvent::ToolUseInputDelta("{\"file_path\":\"a.txt\"}".to_string()))),
                    3 => Ok(Some(ProviderStreamEvent::ToolUseEnd)),
                    4 => Ok(Some(ProviderStreamEvent::MessageComplete {
                        usage: Default::default(),
                        stop_reason: None,
                    })),
                    _ => Ok(None)
                }
            }
        }
        Ok(Box::new(ToolReceiver { step: 0, id: format!("call_{}", count) }))
    }
}

fn build_runtime<C: LlmClient>(client: C, max_iterations: usize) -> AgentRuntime<C> {
    let config = AgentConfig {
        max_iterations,
        model_config: ModelConfig {
            provider: None,
            model: "test".to_string(),
            max_tokens: 1024,
            temperature: None,
            top_p: None,
            thinking_budget: None,
            reasoning_effort: None,
        },
        tool_timeout: std::time::Duration::from_secs(10),
        max_tokens: 1000,
        use_turn_context: true,
        trimmer: TrimmerConfig::default(),
        config_dir: std::path::PathBuf::from(""),
        prompts_dir: std::path::PathBuf::from(""),
        project_context_file: None,
        initial_env_snapshot: None,
        loop_guard: LoopGuardConfig::default(),
    };
    AgentRuntime::new(client, ToolRegistry::new(), config)
}

#[tokio::test]
async fn test_stalled_iteration_aborts_turn() {
    let runtime = build_runtime(StalledClient, 10);
    let (tx, mut rx) = mpsc::channel(100);
    
    let _res = runtime.run_turn(
        &[],
        "hello",
        "session_1",
        None,
        tx,
        None
    ).await.unwrap();

    let mut hit_stall = false;
    while let Some(ev) = rx.recv().await {
        if let AgentEvent::LoopGuardTriggered { reason_code, .. } = ev {
            if reason_code == "stalled_iteration_abort" {
                hit_stall = true;
            }
        }
    }
    assert!(hit_stall);
}

#[tokio::test]
async fn test_duplicate_tool_call_rejected() {
    let mut tools = ToolRegistry::new();
    tools.register(Box::new(nova_agent::tool::builtin::read::ReadTool::new(None)));
    
    let mut runtime = build_runtime(DuplicateToolClient { call_count: Arc::new(AtomicUsize::new(0)) }, 5);
    runtime.set_tools(tools);
    let (tx, mut rx) = mpsc::channel(100);
    
    let _res = runtime.run_turn(
        &[],
        "hello",
        "session_1",
        None,
        tx,
        None
    ).await.unwrap();

    let mut hit_warning = false;
    let mut hit_reject = false;
    while let Some(ev) = rx.recv().await {
        if let AgentEvent::LoopGuardTriggered { reason_code, .. } = ev {
            if reason_code == "duplicate_tool_call_warning" {
                hit_warning = true;
            } else if reason_code == "duplicate_tool_call_rejected" {
                hit_reject = true;
            }
        }
    }
    assert!(hit_warning);
    assert!(hit_reject);
}
