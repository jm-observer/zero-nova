# Session Progress Push Center

## 时间

- 创建日期：2026-05-08
- 最后更新：2026-05-09

## 项目现状

当前 WebSocket 实时事件分发以“单次请求绑定单条连接”为主：

- `crates/nova-gateway-core/src/handlers/chat.rs` 在收到 `chat` 请求后，为该请求创建 `event_tx/event_rx`，并直接把事件转发到当前连接的 `outbound_tx`
- 连接刷新或断开后，旧连接上的实时事件流会终止，但后台任务不会停止
- 新连接建立时，`AgentApplication::on_connect` 仅返回 `Welcome`，不会补发“当前运行中的 Session / Run / Permission”状态
- 前端刷新后会重建 `GatewayClient` 和所有事件监听器，只能重新请求快照接口，不能自动继续之前的流式进度

当前数据库中，部分可观测数据已经持久化，但粒度不完整：

- 已持久化：
  - `sessions`
  - `messages`
  - `runs`
  - `run_steps`
  - `permission_requests`
  - `audit_logs`
  - `diagnostic_issues`
  - `workspace_restore_state`
- 未见独立持久化：
  - token 级流式文本增量
  - thinking 增量
  - tool log 增量 stdout/stderr
  - 单连接临时转发队列中的中间事件

这意味着“页面刷新后恢复当前 Session 进度”只能依赖已持久化的快照型数据，不能依赖旧连接上的瞬时流。

## 整体目标

实现一个后端推送中心，负责：

- 管理已建立的 WebSocket 连接
- 按 Session 维度向相关连接实时分发运行态事件
- 在连接断开时自动移除失效连接
- 在新连接建立后允许该连接订阅指定 Session 的实时事件
- 支持前端刷新后主动拉取当前 Session 的进度快照并恢复 UI

同时明确“可恢复进度”的数据来源必须来自数据库中已持久化的记录，而不是依赖丢失后无法重建的流式增量。

## Plan 拆分

| Plan | 描述 | 依赖 | 顺序 | 状态 |
|------|------|------|------|------|
| Plan 1 | 审计并补齐 Session 进度恢复所需的持久化快照边界 | 无 | 1 | 已完成 |
| Plan 2 | 实现后端 Push Center，集中管理连接、订阅关系与事件广播 | Plan 1 | 2 | 已完成 |
| Plan 3 | 前端刷新恢复：重连后主动拉取当前 Session 进度并恢复页面状态 | Plan 1, Plan 2 | 3 | 已完成 |

执行顺序：

1. 先确定哪些进度数据已持久化、哪些需要补拉快照
2. 再实现连接注册与按 Session 分发
3. 最后实现页面刷新后的恢复逻辑和验证

## 风险与待定项

- 风险：如果继续依赖 token / thinking / tool log 这类未持久化增量，刷新后只能恢复到“最近已落库状态”，无法无损还原中间流
- 风险：若 Push Center 只做广播而没有订阅过滤，不同 Session 的事件会互相污染
- 待定项：是否需要将 `tool_log` 也持久化；若需要，需要新增表或扩展 `run_steps.payload`
- 已确认：前端恢复范围只覆盖当前展示的 Session
