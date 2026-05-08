# Plan 2: 后端 Push Center

## 前置依赖

- Plan 1

## 本次目标

实现一个集中式后端推送中心，按 Session 管理连接订阅关系，并将运行态事件广播给当前订阅该 Session 的 WebSocket 连接。

可验证标准：

- 新连接可注册到 Push Center
- 连接断开会自动移除
- 连接可订阅/取消订阅 Session
- 后端接收事件后只分发给相关 Session 的连接

## 涉及文件

- `crates/nova-gateway-core/src/lib.rs`
- `crates/nova-gateway-core/src/router.rs`
- `crates/nova-gateway-core/src/handlers/chat.rs`
- `crates/nova-agent/src/app/application.rs`
- 可能新增：
  - `crates/nova-gateway-core/src/push_center.rs`

## 详细设计

核心思路：

- 在 gateway 层引入 `PushCenter`
- `PushCenter` 保存：
  - `peer_id -> ResponseSink<GatewayMessage>`
  - `session_id -> HashSet<peer_id>`
- `on_connect` 时注册连接
- `on_disconnect` 时移除连接以及其订阅关系
- 当前会话选择、聊天开始、工作区恢复应用后，由前端显式告知“当前关注的 session”，服务端更新订阅关系

事件分发策略：

- `chat.progress`、`chat.complete`、`session.runtime.updated`、`run.status.updated`、`run.step.updated`、`permission.requested` 等都按 `session_id` 广播
- 若事件不带 `session_id`，不进入 Push Center 广播，仍走原有点对点返回

与现有 `chat` 请求路径的衔接：

- 保留请求-响应语义：`chat.start` / 请求级错误 仍回到当前请求连接
- 运行中事件改为同时投递到 Push Center，而不是只绑定当前 `outbound_tx`
- 这样刷新后新连接只要重新订阅 Session，就能继续收到后续实时事件

## 测试案例

- 正常路径：两个连接同时订阅同一 Session，收到相同的 run status 更新
- 正常路径：连接 A 订阅 Session A，连接 B 订阅 Session B，事件不得串流
- 边界条件：连接断开后再推送事件，不得 panic，也不得继续向失效 sink 发送
- 异常场景：同一 peer 重复订阅同一 Session，订阅集合应保持幂等

