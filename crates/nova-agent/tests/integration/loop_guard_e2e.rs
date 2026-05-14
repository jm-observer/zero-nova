use anyhow::Result;
use async_trait::async_trait;
use nova_agent::agent::{AgentConfig, AgentRuntime, PromptDiagnosticsConfig, ToolResultCompactionConfig};
use nova_agent::event::AgentEvent;
use nova_agent::loop_guard::{DuplicateReadMode, LoopGuardConfig};
use nova_agent::message::Message;
use nova_agent::prompt::TrimmerConfig;
use nova_agent::provider::types::ProviderRequestContext;
use nova_agent::provider::{LlmClient, ModelConfig, ProviderStreamEvent, StreamReceiver};
use nova_agent::tool::ToolRegistry;
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
        _request_context: &ProviderRequestContext,
    ) -> Result<Box<dyn StreamReceiver>> {
        struct StalledReceiver {
            step: usize,
        }
        #[async_trait]
        impl StreamReceiver for StalledReceiver {
            async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>> {
                self.step += 1;
                match self.step {
                    1 => Ok(Some(ProviderStreamEvent::ToolUseStart {
                        id: "stalled_call".to_string(),
                        name: "Read".to_string(),
                    })),
                    2 => Ok(Some(ProviderStreamEvent::ToolUseInputDelta(
                        "{\"file_path\":\"a.txt\"}".to_string(),
                    ))),
                    3 => Ok(Some(ProviderStreamEvent::ToolUseEnd)),
                    4 => Ok(Some(ProviderStreamEvent::MessageComplete {
                        usage: Default::default(),
                        stop_reason: None,
                    })),
                    _ => Ok(None),
                }
            }
        }
        Ok(Box::new(StalledReceiver { step: 0 }))
    }
}

struct DuplicateToolClient {
    call_count: Arc<AtomicUsize>,
}

#[async_trait]
impl LlmClient for DuplicateToolClient {
    async fn stream(
        &self,
        _messages: &[Message],
        _tools: &[nova_agent::provider::types::ToolDefinition],
        _config: &ModelConfig,
        _request_context: &ProviderRequestContext,
    ) -> Result<Box<dyn StreamReceiver>> {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst);
        struct ToolReceiver {
            step: usize,
            id: String,
        }
        #[async_trait]
        impl StreamReceiver for ToolReceiver {
            async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>> {
                self.step += 1;
                match self.step {
                    1 => Ok(Some(ProviderStreamEvent::ToolUseStart {
                        id: self.id.clone(),
                        name: "Read".to_string(),
                    })),
                    2 => Ok(Some(ProviderStreamEvent::ToolUseInputDelta(
                        "{\"file_path\":\"a.txt\"}".to_string(),
                    ))),
                    3 => Ok(Some(ProviderStreamEvent::ToolUseEnd)),
                    4 => Ok(Some(ProviderStreamEvent::MessageComplete {
                        usage: Default::default(),
                        stop_reason: None,
                    })),
                    _ => Ok(None),
                }
            }
        }
        Ok(Box::new(ToolReceiver {
            step: 0,
            id: format!("call_{}", count),
        }))
    }
}

struct DuplicateThenRecoverClient {
    call_count: Arc<AtomicUsize>,
}

#[async_trait]
impl LlmClient for DuplicateThenRecoverClient {
    async fn stream(
        &self,
        _messages: &[Message],
        _tools: &[nova_agent::provider::types::ToolDefinition],
        _config: &ModelConfig,
        _request_context: &ProviderRequestContext,
    ) -> Result<Box<dyn StreamReceiver>> {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst);
        struct Receiver {
            step: usize,
            count: usize,
        }
        #[async_trait]
        impl StreamReceiver for Receiver {
            async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>> {
                self.step += 1;
                if self.count >= 3 {
                    return match self.step {
                        1 => Ok(Some(ProviderStreamEvent::TextDelta(
                            "Recovered after guard feedback".to_string(),
                        ))),
                        2 => Ok(Some(ProviderStreamEvent::MessageComplete {
                            usage: Default::default(),
                            stop_reason: None,
                        })),
                        _ => Ok(None),
                    };
                }

                match self.step {
                    1 => Ok(Some(ProviderStreamEvent::ToolUseStart {
                        id: format!("recover_call_{}", self.count),
                        name: "Read".to_string(),
                    })),
                    2 => Ok(Some(ProviderStreamEvent::ToolUseInputDelta(
                        "{\"file_path\":\"a.txt\"}".to_string(),
                    ))),
                    3 => Ok(Some(ProviderStreamEvent::ToolUseEnd)),
                    4 => Ok(Some(ProviderStreamEvent::MessageComplete {
                        usage: Default::default(),
                        stop_reason: None,
                    })),
                    _ => Ok(None),
                }
            }
        }
        Ok(Box::new(Receiver { step: 0, count }))
    }
}

fn build_runtime<C: LlmClient>(client: C, max_iterations: usize) -> AgentRuntime<C> {
    build_runtime_with_loop_guard(client, max_iterations, LoopGuardConfig::default())
}

fn build_runtime_with_loop_guard<C: LlmClient>(
    client: C,
    max_iterations: usize,
    loop_guard: LoopGuardConfig,
) -> AgentRuntime<C> {
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
        loop_guard,
        prompt_diagnostics: PromptDiagnosticsConfig {
            enabled: false,
            large_section_chars: 8_000,
            large_message_chars: 12_000,
            large_tool_result_chars: 8_000,
        },
        tool_result_compaction: ToolResultCompactionConfig {
            enabled: true,
            max_chars: 12_000,
            head_chars: 4_000,
            tail_chars: 4_000,
            disable_for_tools: std::collections::HashSet::new(),
        },
    };
    AgentRuntime::new(client, ToolRegistry::new(), config)
}

#[tokio::test]
async fn test_stalled_iteration_aborts_turn() {
    let tools = ToolRegistry::new();
    tools.register(Box::new(nova_agent::tool::builtin::read::ReadTool::new(None)));
    let mut runtime = build_runtime_with_loop_guard(
        StalledClient,
        10,
        LoopGuardConfig {
            max_consecutive_duplicate_tool_calls: 100,
            duplicate_read_mode: DuplicateReadMode::WarnOnly,
            ..LoopGuardConfig::default()
        },
    );
    runtime.set_tools(tools);
    let (tx, mut rx) = mpsc::channel(100);

    let _res = runtime
        .run_turn(&[], "hello", "session_1", None, None, tx, None)
        .await
        .unwrap();

    let mut hit_stall = false;
    while let Some(ev) = rx.recv().await {
        if let AgentEvent::SystemLog(log) = ev {
            if log.contains("reason_code=stalled_iteration_abort") {
                hit_stall = true;
            }
        }
    }
    assert!(hit_stall);
}

#[tokio::test]
async fn test_duplicate_tool_call_rejected() {
    let tools = ToolRegistry::new();
    tools.register(Box::new(nova_agent::tool::builtin::read::ReadTool::new(None)));

    let mut runtime = build_runtime(
        DuplicateToolClient {
            call_count: Arc::new(AtomicUsize::new(0)),
        },
        5,
    );
    runtime.set_tools(tools);
    let (tx, mut rx) = mpsc::channel(100);

    let _res = runtime
        .run_turn(&[], "hello", "session_1", None, None, tx, None)
        .await
        .unwrap();

    let mut hit_warning = false;
    let mut hit_reject = false;
    while let Some(ev) = rx.recv().await {
        if let AgentEvent::SystemLog(log) = ev {
            if log.contains("reason_code=duplicate_tool_call_warning") {
                hit_warning = true;
            } else if log.contains("reason_code=duplicate_tool_call_rejected") {
                hit_reject = true;
            }
        }
    }
    assert!(hit_warning);
    assert!(hit_reject);
}

#[tokio::test]
async fn duplicate_tool_rejection_is_fed_back_before_stall_abort() {
    let tools = ToolRegistry::new();
    tools.register(Box::new(nova_agent::tool::builtin::read::ReadTool::new(None)));

    let call_count = Arc::new(AtomicUsize::new(0));
    let mut runtime = build_runtime(
        DuplicateThenRecoverClient {
            call_count: call_count.clone(),
        },
        6,
    );
    runtime.set_tools(tools);
    let (tx, mut rx) = mpsc::channel(100);

    let res = runtime
        .run_turn(&[], "hello", "session_1", None, None, tx, None)
        .await
        .unwrap();

    let mut hit_reject = false;
    let mut hit_stall = false;
    while let Some(ev) = rx.recv().await {
        if let AgentEvent::SystemLog(log) = ev {
            if log.contains("reason_code=duplicate_tool_call_rejected") {
                hit_reject = true;
            } else if log.contains("reason_code=stalled_iteration_abort") {
                hit_stall = true;
            }
        }
    }

    let final_text = res
        .messages
        .iter()
        .flat_map(|message| message.content.iter())
        .find_map(|block| match block {
            nova_agent::message::ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        });

    assert!(hit_reject);
    assert!(!hit_stall);
    assert_eq!(final_text, Some("Recovered after guard feedback"));
    assert_eq!(call_count.load(Ordering::SeqCst), 4);
}
