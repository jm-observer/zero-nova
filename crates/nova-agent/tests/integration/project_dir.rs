use nova_agent::conversation::{SessionCache, SessionService, SqliteManager, SqliteSessionRepository};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::test]
async fn session_project_dir_can_set_and_reset() {
    let data_dir = tempdir().expect("create data tempdir");
    let manager = SqliteManager::new(data_dir.path()).await.expect("create sqlite manager");
    let repository = SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);

    let session = service
        .create(Some("project-dir-test".to_string()), "default".to_string(), String::new())
        .await
        .expect("create session");

    let target_dir = tempdir().expect("create target tempdir");
    let expected_target = tokio::fs::canonicalize(target_dir.path())
        .await
        .unwrap_or_else(|_| PathBuf::from(target_dir.path()));

    let updated = service
        .set_project_dir(&session.id, target_dir.path())
        .await
        .expect("set project dir");
    assert_eq!(updated, expected_target);

    let current = service
        .get_project_dir(&session.id)
        .await
        .expect("get project dir after set");
    assert_eq!(current, expected_target);

    let reset = service
        .reset_project_dir(&session.id)
        .await
        .expect("reset project dir");
    let cwd = std::env::current_dir().expect("get cwd");
    let expected_cwd = tokio::fs::canonicalize(&cwd)
        .await
        .unwrap_or(cwd);
    assert_eq!(reset, expected_cwd);
}

#[tokio::test]
async fn set_project_dir_keeps_raw_path_when_canonicalize_fails() {
    let data_dir = tempdir().expect("create data tempdir");
    let manager = SqliteManager::new(data_dir.path()).await.expect("create sqlite manager");
    let repository = SqliteSessionRepository::new(manager.pool.clone());
    let service = SessionService::new(Arc::new(SessionCache::new()), repository);

    let session = service
        .create(Some("project-dir-raw-path".to_string()), "default".to_string(), String::new())
        .await
        .expect("create session");

    let missing_dir = data_dir.path().join("missing-project-dir");
    let updated = service
        .set_project_dir(&session.id, &missing_dir)
        .await
        .expect("set missing project dir should keep raw path");
    assert_eq!(updated, missing_dir);
}

#[tokio::test]
async fn session_uses_configured_default_project_dir_for_create_and_reset() {
    let data_dir = tempdir().expect("create data tempdir");
    let manager = SqliteManager::new(data_dir.path()).await.expect("create sqlite manager");
    let repository = SqliteSessionRepository::new(manager.pool.clone());

    let configured_default = tempdir().expect("create configured default tempdir");
    let expected_default = tokio::fs::canonicalize(configured_default.path())
        .await
        .unwrap_or_else(|_| PathBuf::from(configured_default.path()));

    let service = SessionService::new_with_default_project_dir(
        Arc::new(SessionCache::new()),
        repository,
        expected_default.clone(),
    );

    let session = service
        .create(Some("project-dir-default".to_string()), "default".to_string(), String::new())
        .await
        .expect("create session");

    let initial = service
        .get_project_dir(&session.id)
        .await
        .expect("get initial project dir");
    assert_eq!(initial, expected_default);

    let switched_dir = tempdir().expect("create switched tempdir");
    service
        .set_project_dir(&session.id, switched_dir.path())
        .await
        .expect("set project dir");

    let reset = service
        .reset_project_dir(&session.id)
        .await
        .expect("reset project dir");
    assert_eq!(reset, expected_default);
}
