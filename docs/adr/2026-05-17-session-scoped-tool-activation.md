# ADR: 会话级工具激活隔离

- 日期：2026-05-17
- 状态：已采纳

## 背景

`ToolSearch` 解析 deferred 工具时，旧实现把工具实例 push 进全局 `RegistryState.tools`。由于进程内单一 `ToolRegistry` 被所有 session 共享，导致：

1. 跨会话泄漏：session A 激活的工具出现在 session B 的 `tools` 数组与系统提示词。
2. 无限累积：激活态进程级单调增长，无失效/驱逐，长期运行持续膨胀。

## 决策

deferred 工具激活态按 `session_id` 隔离：

- 新增 `RegistryState.session_activations: HashMap<session_id, HashMap<tool_name, Arc<dyn Tool>>>`。
- `resolve_deferred*` 不再写全局 `tools`、不移除全局 `deferred`，仅写入调用 session 的激活集合。
- `get_turn_view` / `tool_metadata` / `execute` 增加 `session_id` 维度；`prepare_turn` 新增 `session_id` 参数并贯通全部调用点；`tool_search::execute` 接收 `ToolContext` 取 `session_id`。
- `delete_session` 调用 `clear_session_activations` 释放该 session 激活集合。
- 空 `session_id`（CLI 一次性等无 session 路径）归并到固定 key `""`，与单 session 等价。

## 影响

- 被取代行为：deferred 工具解析后进入全局 loaded 集合的进程级共享语义。
- `tool_definitions()` / `loaded_definitions()` / `has_loaded_tool()` 现仅反映 always-on；激活工具只能经 `get_turn_view(session_id, ..)` 观察。
- 子代理各自独立 `ToolRegistry`，行为不变。
- 激活态上界 = deferred 工具总数 × 活跃 session 数，且随 session 删除释放。

## 替代方案

- 方案 B（进程级共享 + LRU/TTL 驱逐）：复杂度高，未采纳。
- 方案 C（维持现状）：不解决泄漏，未采纳。
