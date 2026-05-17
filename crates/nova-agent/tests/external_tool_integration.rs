use nova_agent::tool::external::register_external_tools;
use nova_agent::tool::{DeferredResolveOutcome, ToolRegistry};
use std::fs;
use tempfile::TempDir;

#[tokio::test]
async fn test_full_flow_load_and_resolve() {
    let dir = TempDir::new().unwrap();
    let tool_dir = dir.path().join("test-tool");
    fs::create_dir_all(&tool_dir).unwrap();
    fs::write(
        tool_dir.join("test-tool.toml"),
        r#"
[[tools]]
name = "test-echo"
description = "Echo a message for testing"
type = "command"
command = "echo"
cwd = false

[[tools.parameters]]
name = "message"
description = "Message to echo"
type = "string"
required = true
arg = [""]
"#,
    )
    .unwrap();

    let registry = ToolRegistry::new();
    register_external_tools(&registry, dir.path()).await;

    let view = registry.get_turn_view("s1", false, false, false).await;
    assert!(view.deferred.iter().any(|d| d.name == "test-echo"));
    assert!(!registry.has_loaded_tool("test-echo").await);

    let outcome = registry.resolve_deferred_with_outcome("s1", "test-echo").await;
    assert_eq!(outcome, DeferredResolveOutcome::Loaded);

    // 激活体现在 s1 视图，不污染全局 always-on，也不泄漏到 s2
    let v1 = registry.get_turn_view("s1", false, false, false).await;
    assert!(v1.loaded.iter().any(|d| d.name == "test-echo"));
    let v2 = registry.get_turn_view("s2", false, false, false).await;
    assert!(!v2.loaded.iter().any(|d| d.name == "test-echo"));
    assert!(v2.deferred.iter().any(|d| d.name == "test-echo"));
    assert!(!registry.has_loaded_tool("test-echo").await);

    // 删除 session 释放激活
    registry.clear_session_activations("s1").await;
    let v1_after = registry.get_turn_view("s1", false, false, false).await;
    assert!(!v1_after.loaded.iter().any(|d| d.name == "test-echo"));
    assert!(v1_after.deferred.iter().any(|d| d.name == "test-echo"));
}

#[tokio::test]
async fn test_empty_dir_no_error() {
    let dir = TempDir::new().unwrap();
    let registry = ToolRegistry::new();
    register_external_tools(&registry, dir.path()).await;

    let view = registry.get_turn_view("s1", false, false, false).await;
    assert!(view.deferred.is_empty());
}

#[tokio::test]
async fn test_resolve_nonexistent_tool() {
    let registry = ToolRegistry::new();
    let outcome = registry.resolve_deferred_with_outcome("s1", "nonexistent").await;
    assert_eq!(outcome, DeferredResolveOutcome::NotFound);
}

#[tokio::test]
async fn test_execute_external_tool() {
    let dir = TempDir::new().unwrap();
    let tool_dir = dir.path().join("echo-tool");
    fs::create_dir_all(&tool_dir).unwrap();

    #[cfg(windows)]
    let toml_content = r#"
[[tools]]
name = "test-cmd"
description = "Run cmd echo"
type = "command"
command = "cmd"
subcommands = ["/C", "echo", "hello"]
cwd = false
"#;

    #[cfg(not(windows))]
    let toml_content = r#"
[[tools]]
name = "test-cmd"
description = "Run echo"
type = "command"
command = "echo"
subcommands = ["hello"]
cwd = false
"#;

    fs::write(tool_dir.join("echo-tool.toml"), toml_content).unwrap();

    let registry = ToolRegistry::new();
    register_external_tools(&registry, dir.path()).await;
    // execute(None) 的 session_id 为 ""，故以 "" 解析保持一致
    registry.resolve_deferred("", "test-cmd").await;

    let output = registry.execute("test-cmd", serde_json::json!({}), None).await.unwrap();
    assert!(!output.is_error);
    assert!(output.content.contains("hello"));
}
