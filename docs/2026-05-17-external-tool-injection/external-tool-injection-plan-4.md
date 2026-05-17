# Plan 4: 集成测试

## 前置依赖

Plan 1, 2, 3

## 任务目标

编写端到端集成测试，验证从 .toml 文件加载 → deferred 注册 → tool_search 激活 → 执行的完整流程。

## 执行范围

- **必须新增**：`crates/nova-agent/tests/external_tool_integration.rs`
- **禁止修改**：src/ 下的任何实现代码

## Agent 执行步骤

### 步骤 1：编写集成测试

```rust
//! 外部 tool 注入集成测试

use nova_agent::tool::external::{load_tool_file, register_external_tools};
use nova_agent::tool::{ToolRegistry, DeferredResolveOutcome};
use std::path::PathBuf;
use tempfile::TempDir;
use std::fs;

#[tokio::test]
async fn test_full_flow_load_and_resolve() {
    // 1. 准备测试 tool 文件
    let dir = TempDir::new().unwrap();
    let tool_dir = dir.path().join("test-tool");
    fs::create_dir_all(&tool_dir).unwrap();
    fs::write(
        tool_dir.join("test-tool.toml"),
        r#"
[[tools]]
name = "test-echo"
description = "Echo input for testing"
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
    ).unwrap();

    // 2. 注册
    let registry = ToolRegistry::new();
    register_external_tools(&registry, dir.path()).await;

    // 3. 验证 deferred 列表
    let view = registry.get_turn_view(true, false, false).await;
    assert!(view.deferred.iter().any(|d| d.name == "test-echo"));

    // 4. 激活
    let outcome = registry.resolve_deferred_with_outcome("test-echo").await;
    assert_eq!(outcome, DeferredResolveOutcome::Loaded);

    // 5. 验证已加载
    assert!(registry.has_loaded_tool("test-echo").await);
}

#[tokio::test]
async fn test_execute_external_tool() {
    // 使用系统 echo 命令验证实际执行
    // ...
}

#[tokio::test]
async fn test_empty_dir_no_error() {
    let dir = TempDir::new().unwrap();
    let registry = ToolRegistry::new();
    register_external_tools(&registry, dir.path()).await;
    let view = registry.get_turn_view(true, false, false).await;
    assert!(view.deferred.is_empty() || view.deferred.len() == 1); // only tool_search itself
}
```

## 行为规则

| 测试场景 | 期望结果 |
|----------|----------|
| 正常 .toml 加载 + resolve | deferred → loaded，可执行 |
| 空目录 | 无 deferred tool，不报错 |
| 执行 echo 命令 | 返回预期 stdout |
| resolve 不存在的 tool | 返回 NotFound |

## 禁止事项

- 禁止依赖网络（测试用 echo/cmd 等本地命令）
- 禁止修改 src/ 实现代码

## 完成条件

- [ ] `test_full_flow_load_and_resolve` 通过
- [ ] `test_execute_external_tool` 通过
- [ ] `test_empty_dir_no_error` 通过
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过
