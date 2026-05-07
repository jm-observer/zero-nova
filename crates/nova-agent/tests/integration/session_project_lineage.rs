use super::mock_client::MockClient;
use nova_agent::app::ConversationService;
use nova_agent::config::{AppConfig, OriginAppConfig};
use nova_agent::conversation::{SessionCache, SessionService, SqliteManager, SqliteSessionRepository};
use nova_agent::{
    AgentConfig, AgentDescriptor, AgentRegistry, AgentRuntime, ModelConfig, ToolRegistry,
    prompt::TrimmerConfig,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;

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
            let control = session.control.read().ok()?;
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

    let created_control = created.control.read().expect("read created control");
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
    let control = created.control.read().expect("read created session control");
    assert_eq!(control.active_agent, "agent-c");
    assert_eq!(control.project_dir, None);
}

fn build_conversation_service(
    sessions: SessionService,
    data_dir: &std::path::Path,
) -> ConversationService<MockClient> {
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
            },
            tool_timeout: Duration::from_secs(1),
            max_tokens: 256,
            use_turn_context: false,
            trimmer: TrimmerConfig::default(),
            config_dir: data_dir.to_path_buf(),
            prompts_dir: data_dir.to_path_buf(),
            project_context_file: None,
            initial_env_snapshot: None,
        },
    );

    ConversationService::new(
        runtime,
        registry,
        sessions,
        AppConfig::from_origin(OriginAppConfig::default(), data_dir.to_path_buf()),
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
        provider_id: "default".to_string(),
        llm_id: Some("default".to_string()),
    }
}
