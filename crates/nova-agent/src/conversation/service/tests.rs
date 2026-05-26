use super::skill_bindings::merge_skill_bindings;
use super::SessionService;
use crate::conversation::cache::SessionCache;
use crate::conversation::control::{TitleSource, TitleState, TitleStatus};
use crate::conversation::sqlite_manager::SqliteManager;
use crate::conversation::title_generator::{TitleGenerationError, TitleGenerator};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::Mutex;

/// Mock title generator: 按构造时给定的 outcome 列表逐次返回，列表用尽后保持返回最后一个。
struct MockTitleGenerator {
    outcomes: Mutex<Vec<Result<String, TitleGenerationError>>>,
    calls: Mutex<usize>,
}

impl MockTitleGenerator {
    fn new(outcomes: Vec<Result<String, TitleGenerationError>>) -> Arc<Self> {
        Arc::new(Self {
            outcomes: Mutex::new(outcomes),
            calls: Mutex::new(0),
        })
    }

    async fn call_count(&self) -> usize {
        *self.calls.lock().await
    }
}

#[async_trait]
impl TitleGenerator for MockTitleGenerator {
    async fn generate(&self, _session_id: &str, _user_texts: &[String]) -> Result<String, TitleGenerationError> {
        *self.calls.lock().await += 1;
        let mut outcomes = self.outcomes.lock().await;
        if outcomes.len() > 1 {
            outcomes.remove(0)
        } else if outcomes.len() == 1 {
            // 保留最后一个供后续重复调用复用（消费 + 重新插回）
            let v = outcomes.remove(0);
            let cloned = match &v {
                Ok(s) => Ok(s.clone()),
                Err(TitleGenerationError::Retryable(e)) => Err(TitleGenerationError::Retryable(anyhow::anyhow!("{e}"))),
                Err(TitleGenerationError::NonRetryable(e)) => {
                    Err(TitleGenerationError::NonRetryable(anyhow::anyhow!("{e}")))
                }
            };
            outcomes.push(cloned);
            v
        } else {
            Err(TitleGenerationError::NonRetryable(anyhow::anyhow!(
                "MockTitleGenerator outcomes exhausted"
            )))
        }
    }
}

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

// ---------------------------------------------------------------------------
// TitleGenerator 注入路径测试（2026-05-26 Plan 1）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn title_generation_uses_injected_generator() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let mut service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let mock = MockTitleGenerator::new(vec![Ok("自定义标题".to_string())]);
    service.set_title_generator(mock.clone());

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

    assert_eq!(mock.call_count().await, 1);
    let title_state = session.title_state.read().await;
    assert_eq!(title_state.status, TitleStatus::Succeeded);
    assert_eq!(title_state.source, TitleSource::Ai);
    drop(title_state);
    assert_eq!(session.get_name().await, "自定义标题");
    Ok(())
}

#[tokio::test]
async fn title_generation_retryable_error_keeps_failed_state() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let mut service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let mock = MockTitleGenerator::new(vec![Err(TitleGenerationError::Retryable(anyhow::anyhow!("boom")))]);
    service.set_title_generator(mock);

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
    assert_eq!(title_state.status, TitleStatus::Failed);
    assert_eq!(title_state.attempt_count, 1);
    assert!(title_state
        .last_error
        .as_deref()
        .map(|s| s.starts_with("retryable:"))
        .unwrap_or(false));
    assert!(title_state.should_retry());
    Ok(())
}

#[tokio::test]
async fn title_generation_non_retryable_error_consumes_attempt() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let mut service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let mock = MockTitleGenerator::new(vec![Err(TitleGenerationError::NonRetryable(anyhow::anyhow!(
        "format-bad"
    )))]);
    service.set_title_generator(mock);

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
    assert_eq!(title_state.status, TitleStatus::Failed);
    assert_eq!(title_state.attempt_count, 1);
    assert!(title_state
        .last_error
        .as_deref()
        .map(|s| s.starts_with("non_retryable:"))
        .unwrap_or(false));
    Ok(())
}

// ---------------------------------------------------------------------------
// Plan 2: 父子 Session 派生路径测试
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_with_parent_persists_parent_columns() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository.clone());

    // 先建一个父 Session
    let parent = service
        .create(Some("parent".to_string()), "agent-1".to_string(), String::new())
        .await?;

    let child = service
        .create_with_parent(
            Some("child".to_string()),
            "agent-1".to_string(),
            parent.id.clone(),
            "toolu_xyz".to_string(),
            None,
        )
        .await?;

    // 内存侧 parent_* 字段正确
    assert_eq!(child.parent_session_id.as_deref(), Some(parent.id.as_str()));
    assert_eq!(child.parent_tool_use_id.as_deref(), Some("toolu_xyz"));

    // SQLite 侧落盘正确
    let row = repository.load_session_meta(&child.id).await?.expect("child row");
    assert_eq!(row.6.as_deref(), Some(parent.id.as_str()));
    assert_eq!(row.7.as_deref(), Some("toolu_xyz"));

    // list_child_session_ids 可查到
    let children_ids = repository.list_child_session_ids(&parent.id).await?;
    assert_eq!(children_ids, vec![child.id.clone()]);
    Ok(())
}

// ---------------------------------------------------------------------------
// 子 Agent 会话根解析（v0.3.14）：root_session_id / ancestor_ids
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_for_agent_sets_self_as_root() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository.clone());

    let root = service
        .create(Some("root".to_string()), "agent-1".to_string(), String::new())
        .await?;
    assert_eq!(root.root_session_id.as_deref(), Some(root.id.as_str()));
    assert_eq!(root.ancestor_ids, Some(Vec::<String>::new()));

    // 落盘列也对：SessionRow index 8 = root_session_id, 9 = ancestor_ids
    let row = repository.load_session_meta(&root.id).await?.expect("root row");
    assert_eq!(row.8.as_deref(), Some(root.id.as_str()));
    assert_eq!(row.9, Some(Vec::<String>::new()));
    Ok(())
}

#[tokio::test]
async fn create_with_parent_derives_root_and_ancestors() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);

    let root = service
        .create(Some("root".to_string()), "agent-1".to_string(), String::new())
        .await?;
    let child = service
        .create_with_parent(
            Some("child".to_string()),
            "agent-1".to_string(),
            root.id.clone(),
            "tu1".to_string(),
            None,
        )
        .await?;

    assert_eq!(child.root_session_id.as_deref(), Some(root.id.as_str()));
    assert_eq!(child.ancestor_ids, Some(vec![root.id.clone()]));
    Ok(())
}

#[tokio::test]
async fn nested_subagent_keeps_top_root() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);

    let root = service
        .create(Some("root".to_string()), "agent-1".to_string(), String::new())
        .await?;
    let child = service
        .create_with_parent(None, "agent-1".to_string(), root.id.clone(), "tu1".to_string(), None)
        .await?;
    let grandchild = service
        .create_with_parent(None, "agent-1".to_string(), child.id.clone(), "tu2".to_string(), None)
        .await?;

    assert_eq!(grandchild.root_session_id.as_deref(), Some(root.id.as_str()));
    assert_eq!(grandchild.ancestor_ids, Some(vec![root.id.clone(), child.id.clone()]));
    Ok(())
}

#[tokio::test]
async fn ancestor_invariants_hold() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);

    let root = service
        .create(Some("root".to_string()), "agent-1".to_string(), String::new())
        .await?;
    let child = service
        .create_with_parent(None, "agent-1".to_string(), root.id.clone(), "tu1".to_string(), None)
        .await?;
    let grandchild = service
        .create_with_parent(None, "agent-1".to_string(), child.id.clone(), "tu2".to_string(), None)
        .await?;

    for s in [&root, &child, &grandchild] {
        let anc = s.ancestor_ids.clone().expect("ancestor_ids populated");
        // root == ancestors[0]（非空时）else 自身
        let expected_root = anc.first().cloned().unwrap_or_else(|| s.id.clone());
        assert_eq!(s.root_session_id.as_deref(), Some(expected_root.as_str()));
        // parent_session_id == ancestors[-1]（非空时）else None
        assert_eq!(s.parent_session_id.as_deref(), anc.last().map(String::as_str));
    }
    Ok(())
}

#[tokio::test]
async fn get_session_root_walks_up() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);

    let root = service
        .create(Some("root".to_string()), "agent-1".to_string(), String::new())
        .await?;
    let child = service
        .create_with_parent(None, "agent-1".to_string(), root.id.clone(), "tu1".to_string(), None)
        .await?;
    let grandchild = service
        .create_with_parent(None, "agent-1".to_string(), child.id.clone(), "tu2".to_string(), None)
        .await?;

    assert_eq!(service.resolve_session_root(&root.id).await?, root.id);
    assert_eq!(service.resolve_session_root(&child.id).await?, root.id);
    assert_eq!(service.resolve_session_root(&grandchild.id).await?, root.id);
    assert_eq!(
        service.resolve_session_ancestors(&grandchild.id).await?,
        vec![root.id.clone(), child.id.clone()]
    );
    Ok(())
}

#[tokio::test]
async fn get_session_root_falls_back_for_legacy_row() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository.clone());

    let ctrl = crate::conversation::control::ControlState::new("agent-1");
    // 存量根：root_session_id / ancestor_ids 两列为 NULL。
    repository
        .save_session("legacy-root", "t", "agent-1", 1, 1, &ctrl, None, None, None, None)
        .await?;
    // 存量子：parent 指向 legacy-root，root/ancestor 列同样 NULL。
    repository
        .save_session(
            "legacy-child",
            "t",
            "agent-1",
            2,
            2,
            &ctrl,
            Some("legacy-root"),
            Some("tu"),
            None,
            None,
        )
        .await?;

    // resolve_session_root 在列为 NULL 时降级 walk parent_session_id 链。
    assert_eq!(service.resolve_session_root("legacy-root").await?, "legacy-root");
    assert_eq!(service.resolve_session_root("legacy-child").await?, "legacy-root");
    assert_eq!(
        service.resolve_session_ancestors("legacy-child").await?,
        vec!["legacy-root".to_string()]
    );
    assert!(service.resolve_session_ancestors("legacy-root").await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn try_get_loaded_returns_session_when_cached() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);

    let session = service
        .create(Some("s".to_string()), "agent-1".to_string(), String::new())
        .await?;

    let got = service.try_get_loaded(&session.id).await;
    assert!(got.is_some());
    assert_eq!(got.unwrap().id, session.id);

    // 未知 id 返回 None，不报错
    assert!(service.try_get_loaded("nonexistent").await.is_none());
    Ok(())
}

#[tokio::test]
async fn write_handle_create_child_session_pushes_into_parent_memory() -> Result<()> {
    use crate::app::conversation_service::ConversationWriteHandle;

    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let writer = ConversationWriteHandle::new(service.clone());

    // 父 Session 创建并进入 cache
    let parent = service
        .create(Some("p".to_string()), "agent-1".to_string(), String::new())
        .await?;

    let child_id = writer
        .create_child_session(&parent.id, "toolu_x", "agent-1", None)
        .await?;

    // 父内存侧已 push_child
    let parent_children = parent.get_child_ids().await;
    assert_eq!(parent_children, vec![child_id.clone()]);
    Ok(())
}

#[tokio::test]
async fn write_handle_persist_subagent_turn_attaches_provider_http_trace() -> Result<()> {
    use crate::agent::TurnResult;
    use crate::app::conversation_service::ConversationWriteHandle;
    use crate::message::{ContentBlock, Message, Role};
    use crate::provider::types::Usage;
    use serde_json::json;

    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository.clone());
    let writer = ConversationWriteHandle::new(service.clone());

    // 父 + 子
    let parent = service
        .create(Some("p".to_string()), "agent-1".to_string(), String::new())
        .await?;
    let child_id = writer
        .create_child_session(&parent.id, "toolu_x", "agent-1", None)
        .await?;

    let now = chrono::Utc::now().timestamp_millis();
    let turn = TurnResult {
        messages: vec![
            Message::new(Role::User, vec![ContentBlock::Text { text: "hi".to_string() }], now),
            Message::new(
                Role::Assistant,
                vec![ContentBlock::Text {
                    text: "hello".to_string(),
                }],
                now,
            ),
        ],
        usage: Usage::default(),
        provider_request_body: Some(json!({"req": "body"})),
        provider_response_body: Some(json!({"resp": "body"})),
    };

    writer.persist_subagent_turn(&child_id, &turn).await?;

    // 重新从 DB 加载子 Session.history，确认 Assistant Message 带 trace
    let loaded = repository.load_session(&child_id).await?.expect("child should exist");
    let history = loaded.6; // index 6: history (前 0..5 是 id/title/agent_id/created_at/updated_at/runtime_control)
                            // 等等：load_session 返回元组实际是 (id, title, agent_id, created_at, updated_at, runtime_control, history, parent_session_id, parent_tool_use_id)
                            // 所以 history 在 index 6
    assert_eq!(history.len(), 2);
    let assistant = history.iter().find(|m| m.role == Role::Assistant).expect("assistant");
    let metadata = assistant.metadata.as_ref().expect("assistant metadata");
    let trace = metadata.provider_http_trace.as_ref().expect("provider_http_trace");
    assert_eq!(trace.request_body, json!({"req":"body"}));
    assert_eq!(trace.response_body, json!({"resp":"body"}));

    // User Message 不应带 trace
    let user = history.iter().find(|m| m.role == Role::User).expect("user");
    assert!(user
        .metadata
        .as_ref()
        .and_then(|m| m.provider_http_trace.as_ref())
        .is_none());
    Ok(())
}

// ---------------------------------------------------------------------------
// Plan 3: 父子树查询 + 级联删除测试
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_child_session_summaries_returns_summaries_in_created_order() -> Result<()> {
    use crate::app::conversation_service::ConversationWriteHandle;

    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let writer = ConversationWriteHandle::new(service.clone());

    let parent = service
        .create(Some("p".to_string()), "agent-1".to_string(), String::new())
        .await?;
    let c1 = writer
        .create_child_session(&parent.id, "t1", "agent-1", Some("c1".to_string()))
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let c2 = writer
        .create_child_session(&parent.id, "t2", "agent-1", Some("c2".to_string()))
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let c3 = writer
        .create_child_session(&parent.id, "t3", "agent-1", Some("c3".to_string()))
        .await?;

    let summaries = service.list_child_session_summaries(&parent.id).await?;
    let ids: Vec<String> = summaries.iter().map(|s| s.id.clone()).collect();
    assert_eq!(ids, vec![c1, c2, c3]);
    Ok(())
}

#[tokio::test]
async fn list_child_session_summaries_empty_for_leaf_or_unknown() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);

    let root = service
        .create(Some("r".to_string()), "agent-1".to_string(), String::new())
        .await?;
    assert!(service.list_child_session_summaries(&root.id).await?.is_empty());
    assert!(service.list_child_session_summaries("ghost").await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn delete_session_tree_returns_zero_for_nonexistent_root() -> Result<()> {
    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);

    let count = service.delete_session_tree("ghost").await?;
    assert_eq!(count, 0);
    Ok(())
}

#[tokio::test]
async fn delete_session_tree_cascades_full_subtree() -> Result<()> {
    use crate::app::conversation_service::ConversationWriteHandle;

    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository.clone());
    let writer = ConversationWriteHandle::new(service.clone());

    // 构造：p + 2 子 + 3 孙
    let p = service
        .create(Some("p".to_string()), "agent-1".to_string(), String::new())
        .await?;
    let c1 = writer.create_child_session(&p.id, "t1", "agent-1", None).await?;
    let c2 = writer.create_child_session(&p.id, "t2", "agent-1", None).await?;
    let _g1 = writer.create_child_session(&c1, "tg1", "agent-1", None).await?;
    let _g2 = writer.create_child_session(&c1, "tg2", "agent-1", None).await?;
    let _g3 = writer.create_child_session(&c2, "tg3", "agent-1", None).await?;

    let deleted = service.delete_session_tree(&p.id).await?;
    assert_eq!(deleted, 6);
    assert!(repository.load_session_meta(&p.id).await?.is_none());
    assert!(repository.load_session_meta(&c1).await?.is_none());
    assert!(repository.load_session_meta(&c2).await?.is_none());
    Ok(())
}

#[tokio::test]
async fn build_session_tree_depth_zero_returns_root_only_with_truncated() -> Result<()> {
    use crate::app::conversation_service::ConversationWriteHandle;
    use crate::app::session_tree::build_session_tree;

    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let writer = ConversationWriteHandle::new(service.clone());

    let p = service
        .create(Some("p".to_string()), "agent-1".to_string(), String::new())
        .await?;
    let _c = writer.create_child_session(&p.id, "t1", "agent-1", None).await?;

    let tree = build_session_tree(&service, &p.id, 0).await?;
    assert!(tree.children.is_empty());
    assert!(tree.truncated, "root has child but depth=0 → truncated");
    assert_eq!(tree.summary.id, p.id);
    Ok(())
}

#[tokio::test]
async fn build_session_tree_recurses_to_full_depth_with_parent_tool_use_id() -> Result<()> {
    use crate::app::conversation_service::ConversationWriteHandle;
    use crate::app::session_tree::build_session_tree;

    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let writer = ConversationWriteHandle::new(service.clone());

    let p = service
        .create(Some("p".to_string()), "agent-1".to_string(), String::new())
        .await?;
    let c = writer.create_child_session(&p.id, "toolu_c", "agent-1", None).await?;
    let _g = writer.create_child_session(&c, "toolu_g", "agent-1", None).await?;

    let tree = build_session_tree(&service, &p.id, 8).await?;
    assert!(!tree.truncated);
    assert!(tree.parent_tool_use_id.is_none(), "root has no parent_tool_use_id");
    assert_eq!(tree.children.len(), 1);
    let child = &tree.children[0];
    assert_eq!(child.parent_tool_use_id.as_deref(), Some("toolu_c"));
    assert!(!child.truncated);
    assert_eq!(child.children.len(), 1);
    let grand = &child.children[0];
    assert_eq!(grand.parent_tool_use_id.as_deref(), Some("toolu_g"));
    assert!(!grand.truncated);
    assert!(grand.children.is_empty());
    Ok(())
}

#[tokio::test]
async fn build_session_tree_truncates_at_depth_limit() -> Result<()> {
    use crate::app::conversation_service::ConversationWriteHandle;
    use crate::app::session_tree::build_session_tree;

    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);
    let writer = ConversationWriteHandle::new(service.clone());

    // 5 层链：root → c1 → c2 → c3 → c4
    let root = service
        .create(Some("r".to_string()), "agent-1".to_string(), String::new())
        .await?;
    let c1 = writer.create_child_session(&root.id, "t1", "agent-1", None).await?;
    let c2 = writer.create_child_session(&c1, "t2", "agent-1", None).await?;
    let c3 = writer.create_child_session(&c2, "t3", "agent-1", None).await?;
    let _c4 = writer.create_child_session(&c3, "t4", "agent-1", None).await?;

    // max_depth=2：展开 root + c1 + c2 三层节点（root 用初始 depth=2 进入，c1 用 depth=1，c2 用 depth=0 即 leaf-with-truncated）
    let tree = build_session_tree(&service, &root.id, 2).await?;
    let level2 = &tree.children[0].children[0]; // c2 节点（remaining=0 时构建）
    assert!(level2.children.is_empty(), "c2 sits at depth boundary");
    assert!(level2.truncated, "c2 has child c3 but remaining_depth=0 → truncated");
    Ok(())
}

#[tokio::test]
async fn build_session_tree_returns_err_for_unknown_root() -> Result<()> {
    use crate::app::session_tree::build_session_tree;

    let dir = tempdir()?;
    let manager = SqliteManager::new(dir.path()).await?;
    let repository = crate::conversation::repository::SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);

    let err = build_session_tree(&service, "ghost", 8).await;
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("not found"));
    Ok(())
}
