use super::skill_bindings::merge_skill_bindings;
use super::SessionService;
use crate::conversation::cache::SessionCache;
use crate::conversation::control::{TitleSource, TitleState, TitleStatus};
use crate::conversation::sqlite_manager::SqliteManager;
use anyhow::Result;
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn merge_skill_bindings_is_idempotent_and_deduplicates_by_skill_id() {
    let mut existing = vec![serde_json::json!({
        "skill_id": "skill-a",
        "name": "Skill A",
        "status": "active",
        "description": "v1"
    })];
    let incoming = vec![
        serde_json::json!({"skill_id":"skill-a","name":"Skill A","status":"active","description":"v1"}),
        serde_json::json!({"skill_id":"skill-a","name":"Skill A","status":"active","description":"v1"}),
    ];

    merge_skill_bindings(&mut existing, incoming);
    assert_eq!(existing.len(), 1);
    assert_eq!(existing[0]["skill_id"], "skill-a");
}

#[test]
fn merge_skill_bindings_overwrites_existing_fields_with_new_values() {
    let mut existing = vec![serde_json::json!({
        "skill_id": "skill-a",
        "name": "Old Name",
        "status": "inactive",
        "description": "old"
    })];
    let incoming = vec![serde_json::json!({
        "skill_id":"skill-a",
        "name":"New Name",
        "status":"active",
        "description":"new"
    })];

    merge_skill_bindings(&mut existing, incoming);
    assert_eq!(existing.len(), 1);
    assert_eq!(existing[0]["name"], "New Name");
    assert_eq!(existing[0]["status"], "active");
    assert_eq!(existing[0]["description"], "new");
}

#[test]
fn merge_skill_bindings_ignores_invalid_items_without_skill_id() {
    let mut existing = vec![serde_json::json!({
        "skill_id": "skill-a",
        "name": "Skill A",
        "status": "active",
        "description": "desc"
    })];
    let incoming = vec![serde_json::json!({
        "name": "Invalid Skill",
        "status": "active",
        "description": "invalid"
    })];

    merge_skill_bindings(&mut existing, incoming);
    assert_eq!(existing.len(), 1);
    assert_eq!(existing[0]["skill_id"], "skill-a");
}

#[tokio::test]
async fn create_starts_without_project_dir() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);

    let session = service
        .create(Some("s".to_string()), "agent-1".to_string(), String::new())
        .await?;

    let control = session.control.read().await;
    assert_eq!(control.project_dir, None);
    Ok(())
}

#[tokio::test]
async fn create_for_agent_inherits_project_dir_only() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let inherited = dir.path().join("project-a");

    let session = service
        .create_for_agent(
            Some("s".to_string()),
            "agent-1".to_string(),
            String::new(),
            Some(inherited.clone()),
        )
        .await?;

    let control = session.control.read().await;
    assert_eq!(control.project_dir, Some(inherited));
    assert_eq!(control.active_agent, "agent-1");
    assert_eq!(control.skill_bindings.len(), 0);
    Ok(())
}

#[tokio::test]
async fn touch_session_refreshes_latest_agent_ordering() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository.clone());

    let first = service
        .create(Some("first".to_string()), "agent-1".to_string(), String::new())
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let second = service
        .create(Some("second".to_string()), "agent-1".to_string(), String::new())
        .await?;

    service.touch_session(&first.id).await?;

    let latest = service
        .find_latest_session_by_agent("agent-1")
        .await?
        .expect("latest session should exist");
    assert_eq!(latest.id, first.id);
    assert_ne!(latest.id, second.id);

    let stored = repository
        .find_latest_session_by_agent("agent-1")
        .await?
        .expect("stored latest session should exist");
    assert_eq!(stored.0, first.id);
    Ok(())
}

#[tokio::test]
async fn skill_bindings_are_persisted_after_service_rebuild() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository.clone());

    let session = service
        .create(Some("s".to_string()), "agent-1".to_string(), String::new())
        .await?;
    service
        .update_runtime_state(
            &session.id,
            None,
            None,
            Some(vec![serde_json::json!({
                "skill_id":"skill-a",
                "name":"Skill A",
                "status":"active",
                "description": serde_json::Value::Null
            })]),
        )
        .await?;

    let rebuilt = SessionService::new(Arc::new(SessionCache::new()), repository);
    let loaded = rebuilt.get(&session.id).await?.expect("session should exist");
    let control = loaded.control.read().await;
    assert_eq!(control.skill_bindings.len(), 1);
    assert_eq!(control.skill_bindings[0]["skill_id"], "skill-a");
    Ok(())
}

#[tokio::test]
async fn concurrent_skill_updates_do_not_lose_or_duplicate_bindings() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);

    let session = service
        .create(Some("s".to_string()), "agent-1".to_string(), String::new())
        .await?;
    let session_id = session.id.clone();

    let service_left = service.clone();
    let service_right = service.clone();
    let left = tokio::spawn(async move {
        service_left
            .update_runtime_state(
                &session_id,
                None,
                None,
                Some(vec![serde_json::json!({
                    "skill_id":"skill-a",
                    "name":"Skill A",
                    "status":"active",
                    "description": serde_json::Value::Null
                })]),
            )
            .await
    });

    let session_id_for_right = session.id.clone();
    let right = tokio::spawn(async move {
        service_right
            .update_runtime_state(
                &session_id_for_right,
                None,
                None,
                Some(vec![serde_json::json!({
                    "skill_id":"skill-b",
                    "name":"Skill B",
                    "status":"active",
                    "description": serde_json::Value::Null
                })]),
            )
            .await
    });

    left.await??;
    right.await??;

    let loaded = service.get(&session.id).await?.expect("session should exist");
    let control = loaded.control.read().await;
    assert_eq!(control.skill_bindings.len(), 2);
    let mut ids = control
        .skill_bindings
        .iter()
        .filter_map(|value| value.get("skill_id").and_then(|skill_id| skill_id.as_str()))
        .collect::<Vec<_>>();
    ids.sort_unstable();
    assert_eq!(ids, vec!["skill-a", "skill-b"]);
    Ok(())
}

#[tokio::test]
async fn provider_http_trace_bound_message_id_is_persisted() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository.clone());

    let session = service
        .create(Some("s".to_string()), "agent-1".to_string(), String::new())
        .await?;

    let metadata = serde_json::json!({
        "providerHttpTrace": {
            "requestBody": {"foo":"bar"},
            "responseBody": {"id":"resp-1"},
            "format": "json",
            "boundMessageId": "",
            "capturedAt": 1,
            "truncated": false
        }
    });
    service
        .append_message(
            &session.id,
            crate::message::Role::Assistant,
            vec![crate::message::ContentBlock::Text {
                text: "assistant".to_string(),
            }],
            Some(metadata),
        )
        .await?;

    let rebuilt = SessionService::new(Arc::new(SessionCache::new()), repository);
    let loaded = rebuilt
        .get_with_history(&session.id)
        .await?
        .expect("session should exist");
    let history = loaded.get_history().await;
    let assistant = history
        .iter()
        .find(|message| message.role == crate::message::Role::Assistant)
        .expect("assistant message should exist");
    let trace = assistant
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.provider_http_trace.as_ref())
        .expect("provider trace should exist");
    assert_eq!(trace.bound_message_id, assistant.id);
    Ok(())
}

#[tokio::test]
async fn default_title_is_used_when_create_name_missing() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let session = service
        .create_for_agent(None, "agent-1".to_string(), String::new(), None)
        .await?;
    assert_eq!(session.get_name().await, super::DEFAULT_SESSION_TITLE);
    Ok(())
}

#[tokio::test]
async fn title_generation_starts_after_second_user_message_with_enough_chars() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let session = service
        .create_for_agent(None, "agent-1".to_string(), String::new(), None)
        .await?;

    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "我想做一个桌面端任务调度工具".to_string(),
            }],
            None,
        )
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    {
        let title_state = session.title_state.read().await;
        assert_eq!(title_state.attempt_count, 0);
    }

    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "要支持重试队列并且按项目分类展示".to_string(),
            }],
            None,
        )
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;

    {
        let title_state = session.title_state.read().await;
        assert_eq!(title_state.status, crate::conversation::control::TitleStatus::Succeeded);
        assert_eq!(title_state.source, crate::conversation::control::TitleSource::Ai);
        assert_eq!(title_state.attempt_count, 1);
    }
    assert_ne!(session.get_name().await, super::DEFAULT_SESSION_TITLE);
    Ok(())
}

#[tokio::test]
async fn title_generation_waits_for_third_message_when_chars_not_enough() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let session = service
        .create_for_agent(None, "agent-1".to_string(), String::new(), None)
        .await?;

    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "短句".to_string(),
            }],
            None,
        )
        .await?;
    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "再短".to_string(),
            }],
            None,
        )
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    {
        let title_state = session.title_state.read().await;
        assert_eq!(title_state.attempt_count, 0);
    }

    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "第三条补充足够语义信息用于触发标题自动生成".to_string(),
            }],
            None,
        )
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    {
        let title_state = session.title_state.read().await;
        assert_eq!(title_state.attempt_count, 1);
        assert_eq!(title_state.status, crate::conversation::control::TitleStatus::Succeeded);
    }
    Ok(())
}

#[test]
fn title_state_initializes_as_idle_with_default_source() {
    let title_state = TitleState::new_default();
    assert_eq!(title_state.status, TitleStatus::Idle);
    assert_eq!(title_state.source, TitleSource::Default);
    assert_eq!(title_state.attempt_count, 0);
    assert!(title_state.last_error.is_none());
    assert!(title_state.last_success_at.is_none());
}

#[test]
fn title_state_set_pending_increments_attempt_and_records_timestamp() {
    let mut title_state = TitleState::new_default();
    let before = chrono::Utc::now().timestamp_millis();
    title_state.set_pending(2);
    let after = chrono::Utc::now().timestamp_millis();

    assert_eq!(title_state.status, TitleStatus::Pending);
    assert_eq!(title_state.attempt_count, 1);
    assert!(title_state.last_attempt_at >= before);
    assert!(title_state.last_attempt_at <= after);
    assert_eq!(title_state.based_on_user_message_count, 2);
}

#[test]
fn title_state_set_succeeded_changes_source_to_ai() {
    let mut title_state = TitleState::new_default();
    title_state.set_pending(2);
    title_state.set_succeeded();

    assert_eq!(title_state.status, TitleStatus::Succeeded);
    assert_eq!(title_state.source, TitleSource::Ai);
    assert!(title_state.last_success_at.is_some());
    assert!(title_state.last_error.is_none());
}

#[test]
fn title_state_set_failed_records_error_and_status() {
    let mut title_state = TitleState::new_default();
    title_state.set_pending(2);
    title_state.set_failed("network error".to_string());

    assert_eq!(title_state.status, TitleStatus::Failed);
    assert_eq!(title_state.last_error, Some("network error".to_string()));
}

#[test]
fn title_state_should_retry_when_failed_and_under_max_attempts() {
    let mut title_state = TitleState::new_default();
    title_state.set_pending(2);
    title_state.set_failed("error".to_string());
    assert!(title_state.should_retry());
}

#[test]
fn title_state_should_not_retry_when_already_succeeded() {
    let mut title_state = TitleState::new_default();
    title_state.set_pending(2);
    title_state.set_succeeded();
    assert!(!title_state.should_retry());
}

#[test]
fn title_state_should_not_retry_when_max_attempts_reached() {
    let mut title_state = TitleState::new_default();
    title_state.attempt_count = super::TITLE_MAX_ATTEMPTS;
    title_state.status = TitleStatus::Failed;
    title_state.last_error = Some("error".to_string());
    assert!(!title_state.should_retry());
}

#[tokio::test]
async fn title_generation_does_not_trigger_on_first_user_message() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let session = service
        .create_for_agent(None, "agent-1".to_string(), String::new(), None)
        .await?;

    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "我想做一个桌面端任务调度工具".to_string(),
            }],
            None,
        )
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let title_state = session.title_state.read().await;
    assert_eq!(title_state.attempt_count, 0);
    assert_eq!(title_state.status, TitleStatus::Idle);
    Ok(())
}

#[tokio::test]
async fn title_generation_does_not_trigger_on_short_messages() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let session = service
        .create_for_agent(None, "agent-1".to_string(), String::new(), None)
        .await?;

    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "你好".to_string(),
            }],
            None,
        )
        .await?;
    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "继续".to_string(),
            }],
            None,
        )
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let title_state = session.title_state.read().await;
    assert_eq!(title_state.attempt_count, 0);
    Ok(())
}

#[tokio::test]
async fn title_generation_succeeded_then_continues_stable() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let session = service
        .create_for_agent(None, "agent-1".to_string(), String::new(), None)
        .await?;

    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "我想做一个桌面端任务调度工具".to_string(),
            }],
            None,
        )
        .await?;
    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "要支持重试队列并且按项目分类展示".to_string(),
            }],
            None,
        )
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;

    {
        let title_state = session.title_state.read().await;
        assert_eq!(title_state.status, TitleStatus::Succeeded);
    }

    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "再加一个定时任务功能".to_string(),
            }],
            None,
        )
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    {
        let title_state = session.title_state.read().await;
        assert_eq!(title_state.status, TitleStatus::Succeeded);
        assert_eq!(title_state.attempt_count, 1);
    }
    Ok(())
}

#[tokio::test]
async fn title_generation_concurrent_append_only_triggers_once() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let session = service
        .create_for_agent(None, "agent-1".to_string(), String::new(), None)
        .await?;

    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "我想做一个桌面端任务调度工具".to_string(),
            }],
            None,
        )
        .await?;

    let session_id = session.id.clone();
    let service_clone = service.clone();
    let (tx1, rx1) = tokio::sync::oneshot::channel();
    let (tx2, rx2) = tokio::sync::oneshot::channel();

    let session_id_for_t1 = session_id.clone();
    let service_clone_for_t1 = service_clone.clone();
    let t1 = tokio::spawn(async move {
        let _ = service_clone_for_t1
            .append_message(
                &session_id_for_t1,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "要支持重试队列".to_string(),
                }],
                None,
            )
            .await;
        tx1.send(()).unwrap();
    });
    let session_id_for_t2 = session_id.clone();
    let service_clone_for_t2 = service_clone.clone();
    let t2 = tokio::spawn(async move {
        let _ = service_clone_for_t2
            .append_message(
                &session_id_for_t2,
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "并且按项目分类展示".to_string(),
                }],
                None,
            )
            .await;
        tx2.send(()).unwrap();
    });

    rx1.await?;
    rx2.await?;
    t1.await?;
    t2.await?;

    tokio::time::sleep(std::time::Duration::from_millis(40)).await;

    let title_state = session.title_state.read().await;
    assert_eq!(title_state.attempt_count, 1);
    assert_eq!(title_state.status, TitleStatus::Succeeded);
    Ok(())
}

#[tokio::test]
async fn title_state_persists_after_service_rebuild() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let session = service
        .create_for_agent(None, "agent-1".to_string(), String::new(), None)
        .await?;

    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "我想做一个桌面端任务调度工具".to_string(),
            }],
            None,
        )
        .await?;
    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "要支持重试队列并且按项目分类展示".to_string(),
            }],
            None,
        )
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;

    let original_title = session.get_name().await;
    let _original_title_state = session.title_state.read().await;

    let rebuilt_repo = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let rebuilt_service = SessionService::new(Arc::new(SessionCache::new()), rebuilt_repo);

    let loaded_session = rebuilt_service
        .get(&session.id)
        .await?
        .expect("session should exist after rebuild");

    let loaded_title_state = loaded_session.title_state.read().await;

    assert_eq!(loaded_title_state.status, TitleStatus::Succeeded);
    assert_eq!(loaded_title_state.source, TitleSource::Ai);
    assert!(loaded_title_state.last_success_at.is_some());
    assert!(loaded_title_state.last_error.is_none());
    assert!(loaded_title_state.attempt_count > 0);

    assert_eq!(loaded_session.get_name().await, original_title);
    Ok(())
}

#[tokio::test]
async fn load_session_index_only_loads_metadata_until_history_requested() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let session = service
        .create_for_agent(None, "agent-1".to_string(), String::new(), None)
        .await?;

    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "hello indexed world".to_string(),
            }],
            None,
        )
        .await?;

    let rebuilt_repo = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let rebuilt_service = SessionService::new(Arc::new(SessionCache::new()), rebuilt_repo);
    rebuilt_service.load_session_index().await?;

    let indexed = rebuilt_service.get(&session.id).await?.expect("session should exist");
    assert!(indexed.history.read().await.is_empty());

    let loaded = rebuilt_service
        .ensure_session_history_loaded(&session.id)
        .await?
        .expect("session should load history");
    assert!(!loaded.history.read().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn ensure_session_history_loaded_deduplicates_concurrent_cold_loads() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let session = service
        .create_for_agent(None, "agent-1".to_string(), String::new(), None)
        .await?;

    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "cold load one".to_string(),
            }],
            None,
        )
        .await?;

    let rebuilt_repo = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let rebuilt_service = SessionService::new(Arc::new(SessionCache::new()), rebuilt_repo);
    rebuilt_service.load_session_index().await?;

    let indexed = rebuilt_service.get(&session.id).await?.expect("session should exist");
    assert!(indexed.history.read().await.is_empty());

    let service_a = rebuilt_service.clone();
    let service_b = rebuilt_service.clone();
    let id_a = session.id.clone();
    let id_b = session.id.clone();

    let a = tokio::spawn(async move { service_a.ensure_session_history_loaded(&id_a).await });
    let b = tokio::spawn(async move { service_b.ensure_session_history_loaded(&id_b).await });

    let loaded_a = a.await??.expect("session should load");
    let loaded_b = b.await??.expect("session should load");

    assert!(!loaded_a.history.read().await.is_empty());
    assert!(!loaded_b.history.read().await.is_empty());
    assert!(rebuilt_service.cache.is_history_loaded(&session.id).await);
    Ok(())
}

#[tokio::test]
async fn title_is_not_regenerated_after_success_and_reload() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let session = service
        .create_for_agent(None, "agent-1".to_string(), String::new(), None)
        .await?;

    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "我想做一个桌面端任务调度工具".to_string(),
            }],
            None,
        )
        .await?;
    service
        .append_message(
            &session.id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "要支持重试队列并且按项目分类展示".to_string(),
            }],
            None,
        )
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;

    let title_state = session.title_state.read().await;
    assert_eq!(title_state.status, TitleStatus::Succeeded);
    assert_eq!(title_state.attempt_count, 1);

    let rebuilt_repo = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let rebuilt_service = SessionService::new(Arc::new(SessionCache::new()), rebuilt_repo);
    rebuilt_service.load_session_index().await?;

    let loaded_session = rebuilt_service.get(&session.id).await?.expect("session should exist");

    let loaded_session_id = loaded_session.id.clone();
    rebuilt_service
        .append_message(
            &loaded_session_id,
            crate::message::Role::User,
            vec![crate::message::ContentBlock::Text {
                text: "再加一个定时任务功能".to_string(),
            }],
            None,
        )
        .await?;

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let loaded_title_state = loaded_session.title_state.read().await;
    assert_eq!(loaded_title_state.attempt_count, 1);
    assert_eq!(loaded_title_state.status, TitleStatus::Succeeded);
    Ok(())
}
