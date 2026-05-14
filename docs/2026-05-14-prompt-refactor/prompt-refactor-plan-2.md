# Plan 2: 消除同步/异步双写，统一为 async

## Plan 编号与标题

Plan 2: 消除同步/异步双写

## 前置依赖

Plan 1（`mod.rs` 拆分完成，子模块成为 source of truth）

## 本次目标

- 删除所有 `load_*`、`from_config`、`resolve_deferred_*` 等函数的同步版本
- 统一为 async 函数；测试代码中需同步调用的地方改用 `#[tokio::test]`
- 工具注册表 `ToolRegistry` 中的 `startup_only` 系列改为统一的 `async` 方法

## 涉及文件

| 文件 | 删除 / 修改 |
|------|-----------|
| `src/prompt/context.rs` | 删除 `load_project_context`、`load_project_context_with_config`、`load_developer_project_prompt` 同步版本；删除 `read_to_string_runtime_aware` |
| `src/prompt/workflow.rs` | 删除 `WorkflowStagePrompts::load_from_file` 同步版本，只保留 async |
| `src/prompt/builder.rs` | 删除 `SystemPromptBuilder::from_config` 同步版本 |
| `src/tool/registry.rs` | 清理 `startup_only` 系列方法，统一为 async |
| 各调用方 | 改为 `.await` |

## 详细设计

### 同步版本删除列表

```
context.rs:
  - load_project_context(dir) → 删除，调用方改用 load_project_context_async(dir).await
  - load_project_context_with_config(dir, path) → 删除
  - load_developer_project_prompt(dir, files) → 删除
  - read_to_string_runtime_aware(path) → 删除

workflow.rs:
  - WorkflowStagePrompts::load_from_file(path) → 删除
  - WorkflowStagePrompts::parse_workflow_stage_content(content) → 保留（纯同步逻辑）

builder.rs:
  - SystemPromptBuilder::from_config(config, skills) → 删除
  - 只保留 from_config_async

tool/registry.rs:
  - lock_state_startup_only() → 重命名为 lock_state()，内部用普通 lock().await
  - lock_snapshot_startup_only() → 整合
  - refresh_snapshot_locked_startup_only() → 整合
  - has_loaded_tool() sync → 删除
  - load_deferred_by_category() sync → 删除
```

### 测试影响

将 `#[test]` 改为 `#[tokio::test]`，其他内容不变。

## 测试案例

- `cargo test -p nova-agent` 全部通过
- `cargo clippy -- -D warnings` 无警告（不再有 `std::fs` 在 async 上下文的隐患）
