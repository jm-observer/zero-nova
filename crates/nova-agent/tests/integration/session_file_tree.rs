use anyhow::Result;
use nova_agent::agent_catalog::AgentDescriptor;
use nova_agent::app::agent_workspace_service::AgentWorkspaceService;
use nova_agent::config::{AppConfig, OriginAppConfig};
use nova_agent::conversation::{SessionCache, SessionService, SqliteManager, SqliteSessionRepository};
use nova_agent::AgentRegistry;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tempfile::tempdir;

fn build_registry() -> AgentRegistry {
    AgentRegistry::new(AgentDescriptor {
        id: "agent-a".to_string(),
        display_name: "agent-a".to_string(),
        description: "test agent".to_string(),
        aliases: Vec::new(),
        system_prompt_template: String::new(),
        system_prompt_base: String::new(),
        initial_template_vars: HashMap::new(),
        tool_whitelist: None,
        model_config: None,
        provider_id: "default".to_string(),
        llm_id: Some("default".to_string()),
    })
}

fn build_config() -> Arc<RwLock<AppConfig>> {
    Arc::new(RwLock::new(AppConfig::from_origin(
        OriginAppConfig::default(),
        PathBuf::from("."),
    )))
}

#[tokio::test]
async fn list_session_file_tree_returns_sorted_entries_for_root() -> Result<()> {
    let data = tempdir()?;
    let project = tempdir()?;
    tokio::fs::create_dir_all(project.path().join("b-dir")).await?;
    tokio::fs::create_dir_all(project.path().join("A-dir")).await?;
    tokio::fs::write(project.path().join("c.txt"), "c").await?;
    tokio::fs::write(project.path().join("B.txt"), "b").await?;

    let manager = SqliteManager::new(data.path()).await?;
    let repository = SqliteSessionRepository::new(manager.pool.clone());
    let sessions = SessionService::new(Arc::new(SessionCache::new()), repository);
    let session = sessions
        .create(Some("tree".to_string()), "agent-a".to_string(), String::new())
        .await?;
    sessions.set_project_dir(&session.id, project.path()).await?;
    let service = AgentWorkspaceService::new(build_registry(), sessions, build_config());

    let response = service.list_session_file_tree(&session.id, None).await?;
    let names = response.entries.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>();
    assert_eq!(names, vec!["A-dir", "b-dir", "B.txt", "c.txt"]);
    assert_eq!(response.base_relative_path, "");
    Ok(())
}

#[tokio::test]
async fn list_session_file_tree_rejects_outside_path() -> Result<()> {
    let data = tempdir()?;
    let project = tempdir()?;

    let manager = SqliteManager::new(data.path()).await?;
    let repository = SqliteSessionRepository::new(manager.pool.clone());
    let sessions = SessionService::new(Arc::new(SessionCache::new()), repository);
    let session = sessions
        .create(Some("tree".to_string()), "agent-a".to_string(), String::new())
        .await?;
    sessions.set_project_dir(&session.id, project.path()).await?;
    let service = AgentWorkspaceService::new(build_registry(), sessions, build_config());

    let err = service
        .list_session_file_tree(&session.id, Some("../secret".to_string()))
        .await
        .expect_err("outside path should fail");
    assert!(err.to_string().contains("PathAccessDenied"));
    Ok(())
}
