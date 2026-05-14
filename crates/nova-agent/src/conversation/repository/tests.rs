use super::SqliteSessionRepository;
use crate::conversation::control::ControlState;
use crate::conversation::model::{RunRecord, RunStepRecord};
use crate::conversation::sqlite_manager::SqliteManager;
use anyhow::Result;
use serde_json::json;
use sqlx::Row;
use tempfile::tempdir;

#[tokio::test]
async fn permission_repository_matches_current_schema() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repo = SqliteSessionRepository::new(manager.pool.clone());

    repo.save_session("session-1", "title", "agent-1", 10, 10, &ControlState::new("agent-1"))
        .await?;

    repo.create_run(&RunRecord {
        id: "run-1".to_string(),
        session_id: "session-1".to_string(),
        status: "running".to_string(),
        created_at: 100,
        updated_at: 100,
        orchestration_model: Some(crate::conversation::control::ModelRef {
            provider: "default".to_string(),
            model: "gpt-4.1".to_string(),
        }),
        execution_model: Some(crate::conversation::control::ModelRef {
            provider: "default".to_string(),
            model: "gpt-4.1-mini".to_string(),
        }),
        tool_call_count: Some(0),
        usage: None,
    })
    .await?;

    repo.create_permission_request(&crate::conversation::model::PermissionRequestRecord {
        id: "perm-1".to_string(),
        session_id: "session-1".to_string(),
        run_id: "run-1".to_string(),
        capability: "filesystem".to_string(),
        resource: "D:/tmp/file.txt".to_string(),
        status: "pending".to_string(),
        reason: Some("need access".to_string()),
        created_at: 100,
    })
    .await?;

    let pending = repo.list_permission_requests("session-1").await?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "perm-1");
    assert_eq!(pending[0].capability, "filesystem");
    assert_eq!(pending[0].resource, "D:/tmp/file.txt");
    assert_eq!(pending[0].status, "pending");

    repo.resolve_permission_request("perm-1", "allowed", None).await?;

    let rows = sqlx::query("SELECT request_id, status, resolved_at FROM permission_requests WHERE request_id = ?")
        .bind("perm-1")
        .fetch_all(&manager.pool)
        .await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<String, _>("request_id"), "perm-1");
    assert_eq!(rows[0].get::<String, _>("status"), "allowed");
    assert!(rows[0].get::<Option<i64>, _>("resolved_at").is_some());

    Ok(())
}

#[tokio::test]
async fn run_repository_matches_current_schema() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repo = SqliteSessionRepository::new(manager.pool.clone());

    repo.save_session("session-1", "title", "agent-1", 10, 10, &ControlState::new("agent-1"))
        .await?;

    repo.create_run(&RunRecord {
        id: "run-1".to_string(),
        session_id: "session-1".to_string(),
        status: "running".to_string(),
        created_at: 100,
        updated_at: 100,
        orchestration_model: Some(crate::conversation::control::ModelRef {
            provider: "default".to_string(),
            model: "gpt-4.1".to_string(),
        }),
        execution_model: Some(crate::conversation::control::ModelRef {
            provider: "default".to_string(),
            model: "gpt-4.1-mini".to_string(),
        }),
        tool_call_count: Some(0),
        usage: None,
    })
    .await?;

    repo.create_run_step(&RunStepRecord {
        id: "step-1".to_string(),
        run_id: "run-1".to_string(),
        step_type: "tool_use".to_string(),
        status: "running".to_string(),
        input: Some(json!({"x": 1})),
        output: None,
        created_at: 110,
        updated_at: 110,
    })
    .await?;

    repo.update_run_step("step-1", "success", Some(&json!({"ok": true})), 120)
        .await?;
    repo.update_run_status("run-1", "success", 130).await?;

    let run = repo.get_run("run-1").await?.expect("run should exist");
    assert_eq!(run.id, "run-1");
    assert_eq!(run.session_id, "session-1");
    assert_eq!(run.status, "success");
    assert_eq!(run.created_at, 100);
    assert_eq!(run.updated_at, 130);
    assert_eq!(
        run.orchestration_model.as_ref().map(|model| model.model.as_str()),
        Some("gpt-4.1")
    );
    assert_eq!(
        run.execution_model.as_ref().map(|model| model.model.as_str()),
        Some("gpt-4.1-mini")
    );
    assert_eq!(run.tool_call_count, Some(1));

    let runs = repo.list_runs("session-1").await?;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, "run-1");
    assert_eq!(runs[0].tool_call_count, Some(1));

    Ok(())
}

#[tokio::test]
async fn audit_log_repository_matches_current_schema() -> Result<()> {
    let dir = tempdir()?;
    let manager = crate::conversation::sqlite_manager::SqliteManager::new(dir.path()).await?;
    let repo = SqliteSessionRepository::new(manager.pool.clone());

    repo.save_session("session-1", "title", "agent-1", 10, 10, &ControlState::new("agent-1"))
        .await?;

    repo.create_audit_log(&crate::conversation::model::AuditLogRecord {
        id: 0,
        session_id: "session-1".to_string(),
        run_id: Some("run-1".to_string()),
        action: "test_action".to_string(),
        details: serde_json::json!({"info": "test"}),
        created_at: 100,
    })
    .await?;

    let logs = repo.list_audit_logs("session-1").await?;
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].action, "test_action");
    assert_eq!(logs[0].details, serde_json::json!({"info": "test"}));
    assert!(logs[0].id > 0);

    Ok(())
}

#[tokio::test]
async fn find_latest_session_by_agent_uses_updated_at_desc() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repo = SqliteSessionRepository::new(manager.pool.clone());

    repo.save_session("session-1", "older", "agent-1", 10, 10, &ControlState::new("agent-1"))
        .await?;
    repo.save_session("session-2", "newer", "agent-1", 20, 30, &ControlState::new("agent-1"))
        .await?;
    repo.save_session("session-3", "other", "agent-2", 20, 40, &ControlState::new("agent-2"))
        .await?;

    let latest = repo
        .find_latest_session_by_agent("agent-1")
        .await?
        .expect("latest session should exist");
    assert_eq!(latest.0, "session-2");
    assert_eq!(latest.1, "newer");

    Ok(())
}

#[tokio::test]
async fn diagnostic_repository_matches_current_schema() -> Result<()> {
    let dir = tempdir()?;
    let manager = crate::conversation::sqlite_manager::SqliteManager::new(dir.path()).await?;
    let repo = SqliteSessionRepository::new(manager.pool.clone());

    repo.save_session("session-1", "title", "agent-1", 10, 10, &ControlState::new("agent-1"))
        .await?;

    repo.create_diagnostic_issue(&crate::conversation::model::DiagnosticIssue {
        id: "diag-1".to_string(),
        session_id: "session-1".to_string(),
        severity: "error".to_string(),
        message: "Something went wrong".to_string(),
        details: Some(serde_json::json!({"code": 500})),
        created_at: 100,
    })
    .await?;

    let issues = repo.list_diagnostics("session-1").await?;
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].id, "diag-1");
    assert_eq!(issues[0].message, "Something went wrong");
    assert_eq!(issues[0].details, Some(serde_json::json!({"code": 500})));

    Ok(())
}
