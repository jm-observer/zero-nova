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

    repo.save_session(
        "session-1",
        "title",
        "agent-1",
        10,
        10,
        &ControlState::new("agent-1"),
        None,
        None,
    )
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

    repo.save_session(
        "session-1",
        "title",
        "agent-1",
        10,
        10,
        &ControlState::new("agent-1"),
        None,
        None,
    )
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

    repo.save_session(
        "session-1",
        "title",
        "agent-1",
        10,
        10,
        &ControlState::new("agent-1"),
        None,
        None,
    )
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

    repo.save_session(
        "session-1",
        "older",
        "agent-1",
        10,
        10,
        &ControlState::new("agent-1"),
        None,
        None,
    )
    .await?;
    repo.save_session(
        "session-2",
        "newer",
        "agent-1",
        20,
        30,
        &ControlState::new("agent-1"),
        None,
        None,
    )
    .await?;
    repo.save_session(
        "session-3",
        "other",
        "agent-2",
        20,
        40,
        &ControlState::new("agent-2"),
        None,
        None,
    )
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

    repo.save_session(
        "session-1",
        "title",
        "agent-1",
        10,
        10,
        &ControlState::new("agent-1"),
        None,
        None,
    )
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

// ---------------------------------------------------------------------------
// Plan 1: 父子 Session 持久化测试
// ---------------------------------------------------------------------------

#[tokio::test]
async fn save_load_root_session_has_null_parent_columns() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repo = SqliteSessionRepository::new(manager.pool.clone());

    repo.save_session(
        "root",
        "title",
        "agent-1",
        10,
        10,
        &ControlState::new("agent-1"),
        None,
        None,
    )
    .await?;

    let row = repo.load_session_meta("root").await?.expect("session should exist");
    assert_eq!(row.0, "root");
    assert_eq!(row.6, None);
    assert_eq!(row.7, None);
    Ok(())
}

#[tokio::test]
async fn save_load_child_session_round_trips_parent_columns() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repo = SqliteSessionRepository::new(manager.pool.clone());

    repo.save_session(
        "child",
        "child-title",
        "agent-1",
        10,
        10,
        &ControlState::new("agent-1"),
        Some("parent-id"),
        Some("toolu_xyz"),
    )
    .await?;

    let row = repo.load_session_meta("child").await?.expect("session should exist");
    assert_eq!(row.6, Some("parent-id".to_string()));
    assert_eq!(row.7, Some("toolu_xyz".to_string()));
    Ok(())
}

#[tokio::test]
async fn list_child_session_ids_returns_children_in_created_order() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repo = SqliteSessionRepository::new(manager.pool.clone());

    repo.save_session(
        "parent",
        "p",
        "agent-1",
        10,
        10,
        &ControlState::new("agent-1"),
        None,
        None,
    )
    .await?;
    repo.save_session(
        "c1",
        "c1",
        "agent-1",
        20,
        20,
        &ControlState::new("agent-1"),
        Some("parent"),
        Some("t1"),
    )
    .await?;
    repo.save_session(
        "c2",
        "c2",
        "agent-1",
        30,
        30,
        &ControlState::new("agent-1"),
        Some("parent"),
        Some("t2"),
    )
    .await?;
    repo.save_session(
        "c3",
        "c3",
        "agent-1",
        40,
        40,
        &ControlState::new("agent-1"),
        Some("parent"),
        Some("t3"),
    )
    .await?;

    let children = repo.list_child_session_ids("parent").await?;
    assert_eq!(children, vec!["c1".to_string(), "c2".to_string(), "c3".to_string()]);
    Ok(())
}

#[tokio::test]
async fn list_child_session_ids_empty_when_no_children() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repo = SqliteSessionRepository::new(manager.pool.clone());

    repo.save_session(
        "root",
        "r",
        "agent-1",
        10,
        10,
        &ControlState::new("agent-1"),
        None,
        None,
    )
    .await?;

    assert!(repo.list_child_session_ids("root").await?.is_empty());
    assert!(repo.list_child_session_ids("nonexistent").await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn upsert_does_not_overwrite_parent_columns() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repo = SqliteSessionRepository::new(manager.pool.clone());

    repo.save_session(
        "child",
        "c",
        "agent-1",
        10,
        10,
        &ControlState::new("agent-1"),
        Some("p1"),
        Some("t1"),
    )
    .await?;

    // 第二次以 None 覆写——ON CONFLICT 不包含 parent_* 列，应保持首写值。
    repo.save_session(
        "child",
        "c-updated",
        "agent-1",
        10,
        20,
        &ControlState::new("agent-1"),
        None,
        None,
    )
    .await?;

    let row = repo.load_session_meta("child").await?.expect("session should exist");
    assert_eq!(row.1, "c-updated"); // title 被覆写
    assert_eq!(row.6, Some("p1".to_string())); // parent_session_id 保留
    assert_eq!(row.7, Some("t1".to_string())); // parent_tool_use_id 保留
    Ok(())
}
