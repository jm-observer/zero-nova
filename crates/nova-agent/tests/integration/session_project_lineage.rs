use super::mock_client::MockClient;
use nova_agent::app::ConversationService;
use nova_agent::app::conversation_service::TurnPromptService;
use nova_agent::config::AppConfig;
use nova_agent::conversation::{SessionCache, SessionService, SqliteManager, SqliteSessionRepository};
use nova_agent::{
    AgentConfig, AgentDescriptor, AgentRegistry, AgentRuntime, ModelConfig, ToolRegistry,
    prompt::TrimmerConfig,
};
use nova_agent::loop_guard::LoopGuardConfig;
use nova_agent::agent::{PromptDiagnosticsConfig, ToolResultCompactionConfig};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::task::JoinSet;

#[tokio::test]
async fn create_for_agent_inherits_latest_project_dir_from_same_agent_only() {
    let data_dir = tempdir().expect("create data tempdir");
    let manager = SqliteManager::new(data_dir.path()).await.expect("create sqlite manager");
    let repository = SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);

    let agent_a_latest = service
        .create(Some("agent-a-latest".to_string()), "agent-a".to_string(), String::new())
        .await
        .expect("create latest agent-a session");
    let project_a = tempdir().expect("create project a dir");
    let expected_project_a = tokio::fs::canonicalize(project_a.path())
        .await
        .unwrap_or_else(|_| project_a.path().to_path_buf());
    service
        .set_project_dir(&agent_a_latest.id, project_a.path())
        .await
        .expect("set project dir for agent-a");

    let agent_b_latest = service
        .create(Some("agent-b-latest".to_string()), "agent-b".to_string(), String::new())
        .await
        .expect("create latest agent-b session");
    let project_b = tempdir().expect("create project b dir");
    service
        .set_project_dir(&agent_b_latest.id, project_b.path())
        .await
        .expect("set project dir for agent-b");

    let inherited_project = service
        .find_latest_session_by_agent("agent-a")
        .await
        .expect("query latest agent-a session")
        .and_then(|session| {
            let control = session.control.try_read().ok()?;
            control.project_dir.clone()
        });
    let created = service
        .create_for_agent(
            Some("agent-a-new".to_string()),
            "agent-a".to_string(),
            String::new(),
            inherited_project,
        )
        .await
        .expect("create inherited session");

    let created_control = created.control.read().await;
    assert_eq!(created_control.project_dir, Some(expected_project_a));
    assert_eq!(created_control.active_agent, "agent-a");
}

#[tokio::test]
async fn switch_agent_restores_latest_session_for_target_agent() {
    let data_dir = tempdir().expect("create data tempdir");
    let manager = SqliteManager::new(data_dir.path()).await.expect("create sqlite manager");
    let repository = SqliteSessionRepository::new(manager.pool.clone());
    let sessions = SessionService::new(Arc::new(SessionCache::new()), repository);
    let conversation = build_conversation_service(sessions.clone(), data_dir.path());

    let source = sessions
        .create(Some("source".to_string()), "agent-a".to_string(), String::new())
        .await
        .expect("create source session");
    let older = sessions
        .create(Some("older".to_string()), "agent-b".to_string(), String::new())
        .await
        .expect("create older target session");
    tokio::time::sleep(Duration::from_millis(2)).await;
    let latest = sessions
        .create(Some("latest".to_string()), "agent-b".to_string(), String::new())
        .await
        .expect("create latest target session");

    let (agent, restored) = conversation
        .switch_agent(&source.id, "agent-b")
        .await
        .expect("switch agent should restore latest session");

    assert_eq!(agent.id, "agent-b");
    assert_eq!(restored.id, latest.id);
    assert_ne!(restored.id, older.id);
}

#[tokio::test]
async fn switch_agent_creates_new_session_when_target_agent_has_no_history() {
    let data_dir = tempdir().expect("create data tempdir");
    let manager = SqliteManager::new(data_dir.path()).await.expect("create sqlite manager");
    let repository = SqliteSessionRepository::new(manager.pool.clone());
    let sessions = SessionService::new(Arc::new(SessionCache::new()), repository);
    let conversation = build_conversation_service(sessions.clone(), data_dir.path());

    let source = sessions
        .create(Some("source".to_string()), "agent-a".to_string(), String::new())
        .await
        .expect("create source session");

    let (agent, created) = conversation
        .switch_agent(&source.id, "agent-c")
        .await
        .expect("switch agent should create session");

    assert_eq!(agent.id, "agent-c");
    let control = created.control.read().await;
    assert_eq!(control.active_agent, "agent-c");
    assert_eq!(control.project_dir, None);
}

#[tokio::test]
async fn concurrent_reads_with_interleaved_writes_keep_project_dir_consistent() {
    let data_dir = tempdir().expect("create data tempdir");
    let manager = SqliteManager::new(data_dir.path()).await.expect("create sqlite manager");
    let repository = SqliteSessionRepository::new(manager.pool.clone());
    let sessions = SessionService::new(Arc::new(SessionCache::new()), repository);
    let session = sessions
        .create(Some("rw-contention".to_string()), "agent-a".to_string(), String::new())
        .await
        .expect("create session");

    let project_a = tempdir().expect("create project a");
    let project_b = tempdir().expect("create project b");
    let expected_a = tokio::fs::canonicalize(project_a.path())
        .await
        .unwrap_or_else(|_| project_a.path().to_path_buf());
    let expected_b = tokio::fs::canonicalize(project_b.path())
        .await
        .unwrap_or_else(|_| project_b.path().to_path_buf());

    let mut reads = JoinSet::new();
    for _ in 0..24 {
        let sessions_clone = sessions.clone();
        let session_id = session.id.clone();
        let expected_a_clone = expected_a.clone();
        let expected_b_clone = expected_b.clone();
        reads.spawn(async move {
            for _ in 0..20 {
                let value = sessions_clone
                    .get_project_dir(&session_id)
                    .await
                    .expect("read project dir");
                if let Some(path) = value {
                    assert!(path == expected_a_clone || path == expected_b_clone);
                }
                tokio::task::yield_now().await;
            }
        });
    }

    sessions
        .set_project_dir(&session.id, project_a.path())
        .await
        .expect("set project a");
    tokio::time::sleep(Duration::from_millis(2)).await;
    sessions
        .set_project_dir(&session.id, project_b.path())
        .await
        .expect("set project b");

    while let Some(result) = reads.join_next().await {
        result.expect("reader task should pass");
    }

    let final_project = sessions
        .get_project_dir(&session.id)
        .await
        .expect("read final project dir");
    assert_eq!(final_project, Some(expected_b));
}

#[tokio::test]
async fn latest_session_lookup_remains_isolated_between_agents() {
    let data_dir = tempdir().expect("create data tempdir");
    let manager = SqliteManager::new(data_dir.path()).await.expect("create sqlite manager");
    let repository = SqliteSessionRepository::new(manager.pool.clone());
    let sessions = SessionService::new(Arc::new(SessionCache::new()), repository);

    for idx in 0..6 {
        let a = sessions
            .create(
                Some(format!("agent-a-{}", idx)),
                "agent-a".to_string(),
                String::new(),
            )
            .await
            .expect("create session for agent-a");
        let a_dir = tempdir().expect("create agent-a project dir");
        sessions
            .set_project_dir(&a.id, a_dir.path())
            .await
            .expect("set project for agent-a");

        let b = sessions
            .create(
                Some(format!("agent-b-{}", idx)),
                "agent-b".to_string(),
                String::new(),
            )
            .await
            .expect("create session for agent-b");
        let b_dir = tempdir().expect("create agent-b project dir");
        sessions
            .set_project_dir(&b.id, b_dir.path())
            .await
            .expect("set project for agent-b");
    }

    let latest_a = sessions
        .find_latest_session_by_agent("agent-a")
        .await
        .expect("find latest agent-a")
        .expect("latest agent-a exists");
    let latest_b = sessions
        .find_latest_session_by_agent("agent-b")
        .await
        .expect("find latest agent-b")
        .expect("latest agent-b exists");

    let control_a = latest_a.control.read().await;
    let control_b = latest_b.control.read().await;
    assert_eq!(control_a.active_agent, "agent-a");
    assert_eq!(control_b.active_agent, "agent-b");
    assert_ne!(control_a.project_dir, control_b.project_dir);
}

fn build_conversation_service(
    sessions: SessionService,
    data_dir: &std::path::Path,
) -> ConversationService {
    let mut registry = AgentRegistry::new(agent_descriptor("agent-a"));
    registry.register(agent_descriptor("agent-b"));
    registry.register(agent_descriptor("agent-c"));

    let runtime = AgentRuntime::new(
        MockClient::new("ok", false),
        ToolRegistry::new(),
        AgentConfig {
            max_iterations: 1,
            model_config: ModelConfig {
                provider: Some("default".to_string()),
                model: "test-model".to_string(),
                max_tokens: 256,
                temperature: None,
                top_p: None,
                thinking_budget: None,
                reasoning_effort: None,
                max_tokens_field: "both".to_string(),
                extra_body: None,
            },
            tool_timeout: Duration::from_secs(1),
            max_tokens: 256,
            trimmer: TrimmerConfig::default(),
            config_dir: data_dir.to_path_buf(),
            prompts_dir: data_dir.to_path_buf(),
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
                enabled: true,
                max_chars: 12_000,
                head_chars: 4_000,
                tail_chars: 4_000,
                disable_for_tools: std::collections::HashSet::new(),
            },
        },
    );

    ConversationService::new(
        runtime,
        registry,
        sessions,
        Arc::new(AppConfig::new(data_dir.to_path_buf())),
        TurnPromptService::empty(),
    )
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
        provider_id: "openai_compat".to_string(),
        llm_id: "gpt_oss_primary".to_string(),
    }
}
