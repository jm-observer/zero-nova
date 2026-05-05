# Plan 3: DeskApp 连接状态整合展示

## 前置依赖
Plan 1, Plan 2

## 本次目标
1. 在 DeskApp 状态层同时维护网关连接状态和 Provider 健康状态。
2. 更新标题栏展示逻辑，正确表达“网关已连通但 Provider 不可用”的情况。
3. 为后续控制台和设置页复用保留状态模型。

## 涉及文件
- `deskapp/src/gateway-client.ts`
- `deskapp/src/core/state.ts`
- `deskapp/src/core/event-bus.ts`
- `deskapp/src/core/types.ts`
- `deskapp/src/ui/titlebar.ts`
- `deskapp/src/ui/agent-console-view.ts`
- `deskapp/src/i18n/zh.ts`
- `deskapp/src/i18n/en.ts`

## 详细设计

### 1. 新前端状态模型
新增：

- `ProviderHealthView`
- `GatewayAggregateStatus`

`AppState` 维护：

- `gatewayConnectionStatus`
- `providerHealthByScope`
- `lastProviderHealthUpdatedAt`

不要把 Provider 心跳硬塞进现有 `gateway:status` 字符串，否则后续状态判断会继续混乱。

### 2. `GatewayClient` 增加订阅面
新增 API：

- `getProviderHealth()`
- `onProviderHealthUpdated()`

连接完成后顺序建议：

1. 建立 WebSocket
2. 收到 welcome
3. 请求 / 接收 `provider.health.snapshot`
4. 写入 `AppState`

### 3. 标题栏聚合策略
标题栏最终展示规则：

1. WebSocket 未连上 => `disconnected` / `failed`
2. WebSocket 已连，但任一活跃 scope 为 `auth_failed` / `unreachable` / `misconfigured` => 红色错误
3. WebSocket 已连，Provider 有 `degraded` => 黄色或运行态文案
4. WebSocket 已连，所有活跃 scope `healthy` => 绿色 ready

推荐文案：

- `Gateway Connected / Provider Healthy`
- `Gateway Connected / Provider Degraded`
- `Gateway Connected / Provider Auth Failed`

中文文案保持简短，详细原因放 tooltip 或 hover 明细。

### 4. 与现有运行态事件的关系
`chat:iteration` 仍可临时把标题栏切到“运行中”文案，但不应覆盖 Provider 错误态。

建议逻辑：

- `running` 只表示“当前正在执行任务”
- `provider error` 优先级高于 `running`

即正在执行任务时如果心跳变红，标题栏仍应显示错误状态。

## 测试案例
1. 正常路径：收到 `provider.health.snapshot` 后，标题栏从“connected”升级为“provider healthy”。
2. 正常路径：WebSocket 连接正常，但收到 `auth_failed`，标题栏显示错误态。
3. 边界路径：运行中收到 `degraded` 更新，标题栏文案变化但不中断当前会话消息流。
4. 边界路径：重新连接后，旧 Provider 状态被新 snapshot 覆盖，不残留脏数据。
5. 回归路径：现有 `gateway:status` 连接/断连逻辑和相关测试仍通过。
