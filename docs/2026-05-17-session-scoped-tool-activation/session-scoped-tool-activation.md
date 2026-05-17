# 会话级工具激活隔离（方案 A）

- 创建时间：2026-05-17
- 关联前置：`docs/2026-05-17-external-tool-injection/`（deferred 工具注入与 ToolSearch 激活）

## 项目现状

- 进程内仅有一个 `AgentRuntime`，持有一个全局 `ToolRegistry`，所有 session 共享。
- `RegistryState` 结构：
  - `tools: Vec<Arc<dyn Tool>>`：always-on + 已解析的 deferred 工具。
  - `deferred: Vec<DeferredToolEntry>`：deferred 工具（名称/描述/schema/工厂/类别）。
- `ToolSearch select:X` → `resolve_deferred_with_outcome(name)` → 把工具 **push 进全局 `state.tools`**。
- 后果：
  1. **跨会话泄漏**：session A 激活的 deferred 工具，会出现在 session B 的 `prepare_turn` / `get_turn_view`，进而进入 B 的 `tools` 数组与系统提示词。
  2. **无限累积**：激活态进程级单调增长，无失效/驱逐，长期运行 `tools` 数组持续膨胀，浪费 token 且不可回收。
- `prepare_turn` 当前不接收 `session_id`；`get_turn_view`、`execute`、`tool_metadata` 均无会话维度。
- 子代理（`tool/builtin/agent.rs`）已各自 `ToolRegistry::new()`，天然隔离，不在本次受影响范围。

## 整体目标

deferred 工具的激活态按 `session_id` 隔离：

- 一个 session 激活的 deferred 工具只对该 session 可见、可调用。
- 全局 `deferred` 工厂表只读共享；全局 `tools` 仅含 always-on，永不被解析行为改写。
- 每个 session 的激活集合天然以"deferred 工具总数"为上界，且随 session 删除而释放。
- `tools` 数组与系统提示词的 `## Deferred Tools` 区块按 session 正确呈现（已激活的移入 loaded，未激活的留在 deferred）。

## 设计要点

### 新状态模型

`RegistryState`：

| 字段 | 语义 | 是否随激活改写 |
|------|------|----------------|
| `tools: Vec<Arc<dyn Tool>>` | 仅 always-on 工具 | 否（仅注册期写入） |
| `deferred: Vec<DeferredToolEntry>` | 全局 deferred 工厂表 | 否（仅注册期写入） |
| `session_activations: HashMap<String, HashMap<String, Arc<dyn Tool>>>` | `session_id → (tool_name → 已实例化工具)` | 是（解析/清理时写入） |

### API 会话化

| 方法 | 旧签名 | 新签名 | 行为变化 |
|------|--------|--------|----------|
| `resolve_deferred_with_outcome` | `(name)` | `(session_id, name)` | 实例化工厂后存入 `session_activations[session_id]`，不再 push 全局 `tools`；已在该 session 激活 → `AlreadyLoaded` |
| `resolve_deferred` | `(name)` | `(session_id, name)` | 同上（thin wrapper） |
| `load_deferred_by_category` | `(category, ...)` | `(session_id, category, ...)` | 逐项按 session 解析 |
| `get_turn_view` | `(ts, sk, tk)` | `(session_id, ts, sk, tk)` | `loaded` = always-on + 该 session 已激活；`deferred` = 未被该 session 激活的 deferred 项 |
| `tool_metadata` | `(name)` | `(session_id, name)` | 命中顺序：always-on → 该 session 激活 → deferred |
| `execute` | `(name, input, ctx)` | 不变（`session_id` 取自 `ctx.session_id`） | 工具查找：always-on → 该 session 激活集合 |
| `has_loaded_tool` | `(name)` | `(name)` | 不变，仅判 always-on（测试用） |

`prepare_turn` 新增 `session_id: &str` 参数，向 `get_turn_view` 透传。

### session_id 贯通路径

- `conversation_service.rs`：两处 `prepare_turn` 调用点已有 `session_id`，直接传入。
- `agent/runtime.rs::run_turn*`：已有 `session_id`，传入 `prepare_turn`。
- `agent/runtime/tool_exec.rs::execute_turn_loop`：轮内刷新已有 `session_id`，改调 `get_turn_view(session_id, ...)`。
- `tool/builtin/tool_search.rs`：handler 从 `ToolContext.session_id` 取 session，调用 `resolve_deferred_with_outcome(session_id, name)`。当前 `builtin::tool_search::execute(self, input)` 未接收 context，需补 `context` 参数。
- `tool/builtin/agent.rs`、`orchestrate_task.rs`：子代理独立 registry，传子代理自身 session_id 即可，行为不变。

### 会话清理

`application.rs::delete_session` 增加 `registry.clear_session_activations(session_id)` 调用，释放该 session 的激活集合。无显式 session 结束事件的路径（CLI 一次性）进程退出自然回收。

### 边界与约束

- 空 `session_id`（如部分内部测试路径）：归入一个固定 key（如 `""`），行为与单 session 等价，不破坏现有测试语义。
- ToolSearch / ToolInfo 的 dispatch 特判保持不变，仅 `tool_search::execute` 增加 context 透传。
- 不改变"轮内激活、轮内即时可见"的既有语义；仅把可见域从全局收敛到 session。

## Plan 拆分（依赖关系与顺序）

| Plan | 标题 | 前置 | 状态 |
|------|------|------|------|
| Plan 1 | Registry 状态模型与会话化核心 API | 无 | 已完成 |
| Plan 2 | 调用链 session_id 贯通 | Plan 1 | 已完成 |
| Plan 3 | 会话清理钩子与测试 | Plan 1, 2 | 已完成 |

> 实施说明：Plan 1 与 Plan 2 因签名变更跨工作区联动，作为一次原子改动落地以保持编译绿；Plan 3 独立完成。

## 风险与待定项

- `prepare_turn` 在一个 turn 内被调用两次（构建 prompt 一次、最终一次），两次需传相同 `session_id`，否则视图不一致——Plan 2 显式约束。
- 空 `session_id` 归并策略需在 Plan 1 测试覆盖，确认不回归现有无 session 的单元测试。
- 设计资产影响：需更新 `docs/design/system-overview.md` 中工具系统章节，并在 `docs/adr/` 追加一条；归入 Plan 3 完成动作。
