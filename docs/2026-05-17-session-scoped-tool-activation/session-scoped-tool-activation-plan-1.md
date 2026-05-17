# Plan 1: Registry 状态模型与会话化核心 API

## 前置依赖

无。

## 任务目标

`ToolRegistry` 不再因解析行为改写全局 loaded 集合；deferred 工具激活态按 `session_id` 隔离存储；核心查询/执行 API 按 session 维度返回正确视图。`cargo clippy/fmt/test` 全绿。

## 执行范围

- 必须修改：`crates/nova-agent/src/tool/registry.rs`
- 允许修改：`crates/nova-agent/src/tool/mod.rs`（仅导出，如需要）
- 禁止修改：调用方（`conversation_service.rs`、`runtime.rs`、`tool_exec.rs`、`tool_search.rs`、`agent.rs`）——属 Plan 2

## Agent 执行步骤

1. 必须在 `RegistryState` 新增字段 `session_activations: HashMap<String, HashMap<String, Arc<dyn Tool>>>`，`RegistryState::new` 初始化为空。
2. 必须保留 `tools`、`deferred` 字段语义；`resolve_*` 禁止再 push 进 `state.tools`。
3. 必须修改 `resolve_deferred_with_outcome` 签名为 `(&self, session_id: &str, name: &str)`：
   - 若 `name` 命中 always-on `state.tools` → 返回 `AlreadyLoaded`。
   - 若该 session 激活集合已含 `name` → 返回 `AlreadyLoaded`。
   - 若 `deferred` 无此项 → `NotFound`。
   - 调用工厂（保留现有 panic 捕获逻辑），失败 → `FactoryFailed { message }`。
   - 成功 → 存入 `session_activations[session_id][name]`，刷新 snapshot，返回 `Loaded`。
4. 必须修改 `resolve_deferred` 签名为 `(&self, session_id: &str, name: &str)`，内部转调上一步。
5. 必须修改 `load_deferred_by_category` 签名新增首参 `session_id: &str`，逐项调用会话化 `resolve_deferred_with_outcome`。
6. 必须修改 `get_turn_view` 签名为 `(&self, session_id: &str, tool_search_enabled: bool, skill_tool_enabled: bool, task_tools_enabled: bool)`：
   - `loaded` = always-on `state.tools` 的 provider 定义 + 该 session 激活工具的 provider 定义（名称去重，always-on 优先）。
   - `deferred` = `deferred_representations` 中**未被该 session 激活**的项，沿用现有 `task_tools_enabled` 过滤。
7. 必须修改 `tool_metadata` 签名为 `(&self, session_id: &str, name: &str)`，命中顺序：always-on → 该 session 激活（`loaded:true, deferred:false`）→ deferred（`loaded:false, deferred:true`）。
8. 必须修改 `execute` 内工具查找：在 always-on `state.tools` 未命中时，按 `context.as_ref().map(|c| c.session_id)`（缺省为 `""`）在 `session_activations` 中查找；命中则执行。`ToolInfo`/`ToolSearch` 特判保持不变。
9. 必须新增 `pub async fn clear_session_activations(&self, session_id: &str)`：移除该 session 的激活集合并刷新 snapshot；session 不存在时为 no-op。
10. 必须更新 `RegistrySnapshot::from_state`：snapshot 仅反映 always-on + deferred 全集；会话维度数据不进 snapshot（snapshot 保持进程级语义，turn view 在 `get_turn_view` 内按 session 组装）。
11. 必须保留并适配本文件内全部既有单元测试；对签名变更的测试统一传入测试用 `session_id`（如 `"s1"` 或 `""`）。

## 目标数据结构 / 接口契约

```rust
struct RegistryState {
    tools: Vec<Arc<dyn Tool>>,                                   // always-on only
    deferred: Vec<DeferredToolEntry>,                            // factories, read-only post-register
    session_activations: HashMap<String, HashMap<String, Arc<dyn Tool>>>,
}

impl ToolRegistry {
    pub async fn resolve_deferred_with_outcome(&self, session_id: &str, name: &str) -> DeferredResolveOutcome;
    pub async fn resolve_deferred(&self, session_id: &str, name: &str) -> bool;
    pub async fn load_deferred_by_category(&self, session_id: &str, category: &DeferredToolCategory, include_subagent: bool) -> CategoryLoadOutcome;
    pub async fn get_turn_view(&self, session_id: &str, tool_search_enabled: bool, skill_tool_enabled: bool, task_tools_enabled: bool) -> TurnToolView;
    pub async fn tool_metadata(&self, session_id: &str, name: &str) -> Option<ToolMetadataView>;
    pub async fn clear_session_activations(&self, session_id: &str);
    // execute 签名不变；session_id 取自 context.session_id，缺省 ""
}
```

## 行为规则

| 输入 | 处理路径 | 期望输出 |
|------|----------|----------|
| `resolve_deferred_with_outcome("s1","T")`，T 为未激活 deferred | 工厂实例化，存入 `session_activations["s1"]["T"]` | `Loaded`；全局 `state.tools` 不变 |
| 同上再次调用 | 命中 session 激活集合 | `AlreadyLoaded` |
| `resolve_deferred_with_outcome("s2","T")` 后 `get_turn_view("s1",..)` | s1 未激活 T | `loaded` 不含 T，`deferred` 含 T |
| `get_turn_view("s2",..)`（s2 已激活 T） | s2 激活集合含 T | `loaded` 含 T，`deferred` 不含 T |
| `execute("T", .., ctx{session_id:"s2"})` | always-on 未命中 → s2 激活集合命中 | 执行 T |
| `execute("T", .., ctx{session_id:"s1"})`，s1 未激活 | 均未命中 | `Tool 'T' not found` |
| `clear_session_activations("s2")` 后 `get_turn_view("s2",..)` | s2 激活集合已清空 | `loaded` 不含 T，`deferred` 含 T |

## 禁止事项

- 禁止在 `resolve_*` 中向 `state.tools` push。
- 禁止改动调用方文件（Plan 2 范围）。
- 禁止用 `#[allow(...)]` 压制因签名变更产生的告警；应修正调用。
- 禁止把会话激活数据写入 `RegistrySnapshot`。

## 测试要求

文件：`crates/nova-agent/src/tool/registry.rs`（`#[cfg(test)] mod tests`）

| 测试名 | 输入 | 期望断言 |
|--------|------|----------|
| `resolve_deferred_is_session_scoped` | s1 解析 T 后查 s1/s2 turn view | s1 `loaded` 含 T；s2 `loaded` 不含 T、`deferred` 含 T |
| `resolve_deferred_does_not_touch_global_tools` | s1 解析 T | `has_loaded_tool("T")` 为 `false`（仅 always-on） |
| `execute_resolves_session_activated_tool` | s1 激活 T 后以 ctx{session:"s1"} execute T | 非 "not found"；ctx{session:"s2"} → "not found" |
| `clear_session_activations_releases_tools` | s1 激活 T → clear → 查 s1 turn view | T 回到 `deferred` |
| `already_loaded_on_repeat_resolve` | s1 连续两次解析 T | 第二次 `AlreadyLoaded` |
| 既有测试适配 | 全部传入 session_id | 全绿 |

验证命令：
```
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test -p nova-agent tool::registry
cargo test --workspace
```

## 完成条件

- [ ] `RegistryState.session_activations` 已定义并初始化
- [ ] `resolve_*` / `get_turn_view` / `tool_metadata` 已会话化，`resolve_*` 不再写全局 `tools`
- [ ] `execute` 支持按 session 查激活工具
- [ ] `clear_session_activations` 已实现
- [ ] 新增 5 个测试 + 既有测试全部适配通过
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过
