# Plan 2: 调用链 session_id 贯通

## 前置依赖

Plan 1（会话化 API 已就绪）。

## 任务目标

所有 `prepare_turn` / `get_turn_view` / `resolve_deferred*` 调用点正确传入 `session_id`；ToolSearch 按调用 session 激活工具；轮内刷新按 session 取视图。`cargo clippy/fmt/test` 全绿。

## 执行范围

- 必须修改：
  - `crates/nova-agent/src/agent/runtime.rs`（`prepare_turn` 签名 + 调用 `get_turn_view`）
  - `crates/nova-agent/src/agent/runtime/tool_exec.rs`（轮内刷新）
  - `crates/nova-agent/src/tool/builtin/tool_search.rs`（handler 接收 context，按 session 解析）
  - `crates/nova-agent/src/tool/registry.rs`（`execute` 内 `tool_search::execute` 调用补传 context）
  - `crates/nova-agent/src/app/conversation_service.rs`（两处 `prepare_turn` 传 session_id）
  - 其余 `prepare_turn` 调用点（`agent.rs` 子代理路径、`orchestrate_task.rs` 如有）
- 禁止修改：Plan 1 已定型的 registry 状态模型与会话化语义

## Agent 执行步骤

1. 必须修改 `runtime.rs::prepare_turn` 签名新增 `session_id: &str`，将 `self.tools.get_turn_view(true,true,true)` 改为 `get_turn_view(session_id, true,true,true)`。
2. 必须修改 `runtime.rs` 内部 `run_turn*` 调用 `prepare_turn` 处，透传已有 `session_id`；空 session 场景传 `""`。
3. 必须修改 `tool_exec.rs::execute_turn_loop` 轮内刷新：`self.tools.get_turn_view(true,true,true)` → `get_turn_view(session_id, true,true,true)`（`session_id` 已在 `ExecuteTurnLoopRequest`）。
4. 必须修改 `builtin::tool_search::execute` 签名为 `(registry, input, context: Option<&ToolContext>)`：从 `context` 取 `session_id`（缺省 `""`），`handle_selection` / `handle_category_selection` 透传 `session_id` 给 `registry.resolve_deferred_with_outcome` / `load_deferred_by_category`。
5. 必须修改 `registry.rs::execute` 中对 `builtin::tool_search::execute` 的调用，补传 `context.as_ref()`。
6. 必须修改 `conversation_service.rs` 两处 `prepare_turn(input, history, ...)` 调用，传入 `session_id`；确认同一 turn 两次调用传相同 `session_id`。
7. 必须排查并修正其余 `prepare_turn` 调用点（`agent.rs` 子代理、`orchestrate_task.rs`）：子代理传其自身 session 标识；无 session 概念处传 `""`。
8. 必须全量编译，依据编译错误补齐所有签名不匹配的调用点；禁止用 `#[allow]` 绕过。

## 目标接口契约

```rust
// runtime.rs
pub async fn prepare_turn(&self, input: &str, current_history: Arc<Vec<Message>>, system_prompt: String, session_id: &str) -> Result<TurnContext>;

// tool_search.rs
pub async fn execute(registry: &ToolRegistry, input: Value, context: Option<&ToolContext>) -> Result<ToolOutput>;
```

## 行为规则

| 输入 | 处理路径 | 期望输出 |
|------|----------|----------|
| session s1 ToolSearch `select:T` | handler 取 ctx.session_id=s1 → `resolve_deferred_with_outcome("s1","T")` | T 进入 s1 激活集合 |
| 紧接 s1 下一迭代 | `execute_turn_loop` 刷新 `get_turn_view("s1",..)` | s1 的 `tools` 数组含 T |
| 同时 s2 的 turn | s2 `prepare_turn(..,"s2")` → `get_turn_view("s2",..)` | s2 `tools` 数组不含 T，系统提示词 deferred 区块含 T |
| 同一 turn 两次 `prepare_turn` | 均传相同 session_id | 两次视图一致 |

## 禁止事项

- 禁止修改 Plan 1 的 registry 语义。
- 禁止在同一 turn 的两次 `prepare_turn` 传不同 session_id。
- 禁止新增依赖。

## 测试要求

文件：`crates/nova-agent/src/tool/builtin/tool_search.rs`、`crates/nova-agent/src/agent/runtime/tool_exec.rs`（或既有集成测试 `crates/nova-agent/tests/external_tool_integration.rs`）

| 测试名 | 输入 | 期望断言 |
|--------|------|----------|
| `tool_search_activates_for_calling_session` | ctx{session:"s1"} 执行 ToolSearch `select:T` | s1 turn view `loaded` 含 T；s2 不含 |
| `turn_loop_refresh_is_session_scoped` | 模拟 s1 轮内激活 T 后刷新 | 刷新后 `tool_definitions` 含 T |
| 既有 tool_search 测试适配 | 传 context | 全绿 |

验证命令：
```
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test -p nova-agent
cargo test --workspace
```

## 完成条件

- [ ] `prepare_turn` 全调用点传 `session_id`
- [ ] `tool_search::execute` 接收 context 并按 session 解析
- [ ] `execute_turn_loop` 轮内刷新按 session 取视图
- [ ] 新增 2 个测试 + 既有测试适配通过
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过
