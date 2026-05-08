# Plan 1: 进度持久化边界审计

## 前置依赖

无

## 本次目标

明确页面刷新后可恢复的 Session 进度数据边界，并据此定义“前端主动拉取”的最小快照集合。

可验证标准：

- 列出当前已经落库的进度相关实体
- 列出当前未落库、只能实时推送的事件类型
- 明确前端刷新后恢复当前 Session 所需的查询接口集合

## 涉及文件

- `crates/nova-agent/src/conversation/sqlite_manager.rs`
- `crates/nova-agent/src/conversation/repository.rs`
- `crates/nova-agent/src/app/conversation_service.rs`
- `crates/nova-agent/src/app/agent_workspace_service.rs`
- `docs/2026-05-08-session-progress-push-center/session-progress-push-center.md`

## 详细设计

当前持久化结论：

- `messages`：持久化最终消息内容，适合刷新后恢复聊天记录
- `runs`：持久化 run 的状态、开始结束时间、usage，适合恢复“任务是否仍在运行/等待”
- `run_steps`：持久化工具步骤输入/输出和状态，适合恢复工具执行轨迹
- `permission_requests`：持久化待授权项，适合恢复等待用户决策状态
- `workspace_restore_state`：持久化控制台恢复状态，但它记录的是 UI 快照，不是完整实时流

当前未持久化结论：

- `token` / `thinking` / `tool_log` 只走 `AppEvent -> GatewayMessage -> WebSocket` 的内存链路
- 这些事件不会进入 SQLite，因此页面刷新后无法从数据库继续增量回放

刷新恢复最小快照集合：

- `sessions.messages`
- `session.runtime`
- `session.runs`
- `run.detail`（针对当前选中 run）
- `permission.pending`
- `workspace.restore`

若控制台已打开，可额外补拉：

- `session.artifacts`
- `audit.logs`
- `diagnostics.current`

## 测试案例

- 正常路径：已有运行中 run，刷新页面后重新拉取 `session.runs`，应能看到 `running` 状态
- 正常路径：已有待授权 request，刷新页面后重新拉取 `permission.pending`，应能看到待处理项
- 边界条件：run 已结束但 token 流中断，刷新后聊天消息和 run 状态应以数据库最终状态为准
- 异常场景：连接断开期间产生的 `tool_log` 未落库，刷新后允许丢失该增量，但不得影响最终 run 状态恢复

