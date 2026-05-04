use anyhow::Result;
use async_trait::async_trait;
use nova_agent::app::ConversationService;
use nova_agent::conversation::{SessionCache, SessionService, SqliteManager, SqliteSessionRepository};
use nova_agent::message::ContentBlock;
use nova_agent::prompt::TrimmerConfig;
use nova_agent::provider::types::{StopReason, ToolDefinition, Usage};
use nova_agent::provider::{LlmClient, ModelConfig, ProviderStreamEvent, StreamReceiver};
use nova_agent::tool::builtin::edit::EditTool;
use nova_agent::tool::builtin::project_manager::ProjectManagerTool;
use nova_agent::tool::builtin::read::ReadTool;
use nova_agent::tool::builtin::write::WriteTool;
use nova_agent::{
    AgentConfig, AgentDescriptor, AgentEvent, AgentRegistry, AgentRuntime, EnvironmentSnapshot, ModelConfig as _,
    ToolContext, ToolRegistry,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::{mpsc, Mutex};

struct SequenceReceiver {
    events: Vec<ProviderStreamEvent>,
    index: usize,
}

#[async_trait]
impl StreamReceiver for SequenceReceiver {
    async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>> {
        if self.index >= self.events.len() {
            return Ok(None);
        }
        let event = self.events[self.index].clone();
        self.index += 1;
        Ok(Some(event))
    }
}

struct RelativeReadClient {
    call_count: AtomicUsize,
}

impl RelativeReadClient {
    fn new() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LlmClient for RelativeReadClient {
    async fn stream(
        &self,
        _messages: &[nova_agent::message::Message],
        _tools: &[ToolDefinition],
        _config: &ModelConfig,
    ) -> Result<Box<dyn StreamReceiver>> {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst);
        let events = if count == 0 {
            vec![
                ProviderStreamEvent::ToolUseStart {
                    id: "tool-read-1".to_string(),
                    name: "Read".to_string(),
                },
                ProviderStreamEvent::ToolUseInputDelta("{\"file_path\":\"note.txt\"}".to_string()),
                ProviderStreamEvent::ToolUseEnd,
                ProviderStreamEvent::MessageComplete {
                    usage: Usage::default(),
                    stop_reason: Some(StopReason::ToolUse),
                },
            ]
        } else {
            vec![
                ProviderStreamEvent::TextDelta("done".to_string()),
                ProviderStreamEvent::MessageComplete {
                    usage: Usage::default(),
                    stop_reason: Some(StopReason::EndTurn),
                },
            ]
        };

        Ok(Box::new(SequenceReceiver { events, index: 0 }))
    }
}

#[tokio::test]
async fn turn_prompt_shows_project_not_set_and_skips_project_context() {
    let data_dir = tempdir().expect("create data tempdir");
    let prompts_dir = tempdir().expect("create prompts tempdir");
    tokio::fs::write(data_dir.path().join("PROJECT.md"), "should not be loaded")
        .await
        .expect("write project context");

    let (conversation, sessions) = build_conversation_service(
        ToolRegistry::new(),
        SessionService::new(
            Arc::new(SessionCache::new()),
            SqliteSessionRepository::new(
                SqliteManager::new(data_dir.path())
                    .await
                    .expect("create sqlite manager")
                    .pool
                    .clone(),
            ),
        ),
        data_dir.path(),
        prompts_dir.path(),
        false,
        NoopClient,
    );

    let session = sessions
        .create(Some("no-project".to_string()), "agent-a".to_string(), String::new())
        .await
        .expect("create session");
    let (event_tx, _event_rx) = mpsc::channel::<AgentEvent>(8);

    conversation
        .start_turn(&session.id, "hello", event_tx)
        .await
        .expect("run turn");

    let loaded = sessions.get(&session.id).await.expect("load session").expect("session exists");
    let control = loaded.control.read().expect("read control");
    let prompt_preview = control
        .last_turn_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.prompt_preview.as_ref())
        .cloned()
        .expect("prompt preview exists");
    let system_prompt = prompt_preview["system_prompt"]
        .as_str()
        .expect("system prompt string");

    assert!(system_prompt.contains("Project directory: (not set)"));
    assert!(!system_prompt.contains("should not be loaded"));
}

#[tokio::test]
async fn file_tools_require_project_for_relative_paths_but_allow_absolute_paths() {
    let registry = ToolRegistry::new();
    registry.register(Box::new(ReadTool::new(None)));
    registry.register(Box::new(WriteTool::new(None)));
    registry.register(Box::new(EditTool::new(None)));

    let temp = tempdir().expect("create tempdir");
    let file_path = temp.path().join("absolute.txt");
    tokio::fs::write(&file_path, "alpha\nbeta\n").await.expect("write file");

    for (tool_name, input) in [
        ("Read", json!({"file_path":"relative.txt"})),
        ("Write", json!({"file_path":"relative.txt","content":"x"})),
        (
            "Edit",
            json!({"file_path":"relative.txt","old_string":"a","new_string":"b"}),
        ),
    ] {
        let output = registry
            .execute(tool_name, input, Some(tool_context(None)))
            .await
            .expect("execute relative path tool");
        assert!(output.is_error);
        assert_eq!(
            output.content,
            "Current session has no project directory. Set a project before using relative paths."
        );
    }

    let output = registry
        .execute(
            "Read",
            json!({"file_path": file_path.to_string_lossy().to_string()}),
            Some(tool_context(None)),
        )
        .await
        .expect("read absolute path");
    assert!(!output.is_error);
    assert!(output.content.contains("1\talpha"));
}

#[tokio::test]
async fn project_manager_get_and_set_support_empty_project() {
    let data_dir = tempdir().expect("create data tempdir");
    let manager = SqliteManager::new(data_dir.path()).await.expect("create sqlite manager");
    let repository = SqliteSessionRepository::new(manager.pool.clone());
    let sessions = SessionService::new(Arc::new(SessionCache::new()), repository);
    let session = sessions
        .create(Some("project-manager".to_string()), "agent-a".to_string(), String::new())
        .await
        .expect("create session");
    let tool = ProjectManagerTool::new(Arc::new(sessions.clone()));

    let get_before = tool
        .execute(json!({"action":"get"}), Some(tool_context_with_session(&session.id, None)))
        .await
        .expect("get before set");
    assert_eq!(get_before.content, "Current project directory: (not set)");
    assert!(!get_before.is_error);

    let target = tempdir().expect("create target dir");
    let expected = tokio::fs::canonicalize(target.path())
        .await
        .unwrap_or_else(|_| target.path().to_path_buf());
    let set_output = tool
        .execute(
            json!({"action":"set","path": target.path().to_string_lossy().to_string()}),
            Some(tool_context_with_session(&session.id, None)),
        )
        .await
        .expect("set project dir");
    assert!(set_output.content.contains(expected.to_string_lossy().as_ref()));

    let get_after = tool
        .execute(
            json!({"action":"get"}),
            Some(tool_context_with_session(
                &session.id,
                Some(expected.to_string_lossy().to_string()),
            )),
        )
        .await
        .expect("get after set");
    assert!(get_after.content.contains(expected.to_string_lossy().as_ref()));
}

#[tokio::test]
async fn inherited_project_dir_is_used_by_prompt_and_relative_tool_execution() {
    let data_dir = tempdir().expect("create data tempdir");
    let prompts_dir = tempdir().expect("create prompts tempdir");
    let project_dir = tempdir().expect("create project dir");
    tokio::fs::write(project_dir.path().join("note.txt"), "hello from project\n")
        .await
        .expect("write project file");

    let tools = ToolRegistry::new();
    tools.register(Box::new(ReadTool::new(None)));

    let manager = SqliteManager::new(data_dir.path()).await.expect("create sqlite manager");
    let repository = SqliteSessionRepository::new(manager.pool.clone());
    let sessions = SessionService::new(Arc::new(SessionCache::new()), repository);

    let prior = sessions
        .create(Some("prior".to_string()), "agent-a".to_string(), String::new())
        .await
        .expect("create prior session");
    let expected_project = sessions
        .set_project_dir(&prior.id, project_dir.path())
        .await
        .expect("set prior project dir");

    let inherited = sessions
        .create_for_agent(
            Some("inherited".to_string()),
            "agent-a".to_string(),
            String::new(),
            Some(expected_project.clone()),
        )
        .await
        .expect("create inherited session");

    let (conversation, sessions) = build_conversation_service(
        tools,
        sessions,
        data_dir.path(),
        prompts_dir.path(),
        true,
        RelativeReadClient::new(),
    );
    let (event_tx, _event_rx) = mpsc::channel::<AgentEvent>(16);

    let turn_result = conversation
        .start_turn(&inherited.id, "read it", event_tx)
        .await
        .expect("run inherited project turn");

    let loaded = sessions
        .get(&inherited.id)
        .await
        .expect("load inherited session")
        .expect("session exists");
    let control = loaded.control.read().expect("read control");
    let prompt_preview = control
        .last_turn_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.prompt_preview.as_ref())
        .cloned()
        .expect("prompt preview exists");
    let system_prompt = prompt_preview["system_prompt"]
        .as_str()
        .expect("system prompt string");
    assert!(system_prompt.contains(expected_project.to_string_lossy().as_ref()));

    let tool_result_output = turn_result
        .messages
        .iter()
        .flat_map(|message| message.content.iter())
        .find_map(|block| match block {
            ContentBlock::ToolResult { output, .. } => Some(output.as_str()),
            _ => None,
        })
        .expect("tool result output");
    assert!(tool_result_output.contains("hello from project"));
}

struct NoopClient;

#[async_trait]
impl LlmClient for NoopClient {
    async fn stream(
        &self,
        _messages: &[nova_agent::message::Message],
        _tools: &[ToolDefinition],
        _config: &ModelConfig,
    ) -> Result<Box<dyn StreamReceiver>> {
        Ok(Box::new(SequenceReceiver {
            events: vec![ProviderStreamEvent::MessageComplete {
                usage: Usage::default(),
                stop_reason: Some(StopReason::EndTurn),
            }],
            index: 0,
        }))
    }
}

fn build_conversation_service<C: LlmClient + 'static>(
    tools: ToolRegistry,
    sessions: SessionService,
    data_dir: &std::path::Path,
    prompts_dir: &std::path::Path,
    use_turn_context: bool,
    client: C,
) -> (ConversationService<C>, SessionService) {
    let mut registry = AgentRegistry::new(agent_descriptor("agent-a"));
    registry.register(agent_descriptor("agent-b"));

    let runtime = AgentRuntime::new(
        client,
        tools,
        AgentConfig {
            max_iterations: 2,
            model_config: ModelConfig::default(),
            tool_timeout: Duration::from_secs(1),
            max_tokens: 256,
            use_turn_context,
            trimmer: TrimmerConfig::default(),
            config_dir: data_dir.to_path_buf(),
            prompts_dir: prompts_dir.to_path_buf(),
            project_context_file: None,
            initial_env_snapshot: Some(EnvironmentSnapshot {
                config_dir: data_dir.to_string_lossy().to_string(),
                project_dir: None,
                platform: "windows".to_string(),
                shell: "powershell".to_string(),
                git_branch: None,
                git_status_summary: None,
                recent_commits: None,
                model_id: Some("test-model".to_string()),
                current_date: "2026-05-04".to_string(),
            }),
        },
    );

    (ConversationService::new(runtime, registry, sessions.clone()), sessions)
}

fn agent_descriptor(id: &str) -> AgentDescriptor {
    AgentDescriptor {
        id: id.to_string(),
        display_name: id.to_string(),
        description: format!("{} description", id),
        aliases: Vec::new(),
        system_prompt_template: format!("system prompt for {}", id),
        system_prompt_base: format!("system prompt for {}", id),
        initial_template_vars: HashMap::new(),
        tool_whitelist: None,
        model_config: None,
    }
}

fn tool_context(project_dir: Option<String>) -> ToolContext {
    tool_context_with_session("session-1", project_dir)
}

fn tool_context_with_session(session_id: &str, project_dir: Option<String>) -> ToolContext {
    let (event_tx, _event_rx) = mpsc::channel(4);
    ToolContext {
        event_tx,
        tool_use_id: "tool-1".to_string(),
        session_id: session_id.to_string(),
        task_store: None,
        skill_registry: None,
        read_files: Arc::new(Mutex::new(std::collections::HashSet::new())),
        environment: Some(EnvironmentSnapshot {
            config_dir: "D:/config".to_string(),
            project_dir,
            platform: "windows".to_string(),
            shell: "powershell".to_string(),
            git_branch: None,
            git_status_summary: None,
            recent_commits: None,
            model_id: None,
            current_date: "2026-05-04".to_string(),
        }),
    }
}
