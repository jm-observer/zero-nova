# Plan 2: Gateway 后端心跳调度与事件广播

## 前置依赖
Plan 1

## 本次目标
1. 在 Rust 后端实现 Provider 心跳调度器和状态缓存。
2. 让 Gateway 在连接建立、配置变更、探测结果变化时广播 Provider 状态。
3. 把当前会话 `project_dir` 注入 runtime snapshot。

## 涉及文件
- `crates/nova-agent/src/provider/mod.rs`
- `crates/nova-agent/src/provider/openai_compat.rs`
- `crates/nova-agent/src/provider/anthropic.rs`
- `crates/nova-agent/src/app/application.rs`
- `crates/nova-agent/src/app/agent_workspace_service.rs`
- `crates/nova-agent/src/app/snapshot_assembler.rs`
- `crates/nova-gateway-core/src/router.rs`
- `crates/nova-gateway-core/src/handlers/mod.rs`
- `crates/nova-gateway-core/src/handlers/system.rs`
- `crates/nova-gateway-core/src/handlers/config.rs`
- `crates/nova-server/src/bin/nova_gateway_ws.rs`
- `crates/nova-server/src/bin/nova_gateway_stdio.rs`

## 详细设计

### 1. 新增心跳管理器
增加 `ProviderHeartbeatManager`，职责仅包含：

- 构建待探测 provider 集合
- 周期性发起轻量探针
- 维护最近一次健康状态缓存
- 对外提供 `snapshot()` 与 `subscribe()` / `emit_if_changed()`

建议使用：

- `tokio::sync::RwLock<HashMap<ProviderScopeKey, ProviderHealthSnapshot>>`
- `tokio::sync::watch` 或应用级事件发送器

其中 `ProviderScopeKey` = `scope + provider`，避免 orchestration/execution 同 provider 时被覆盖。

### 2. 探测策略
定义常量：

- `PROVIDER_HEARTBEAT_INTERVAL_SECS = 30`
- `PROVIDER_HEARTBEAT_TIMEOUT_SECS = 5`
- `PROVIDER_HEARTBEAT_DEGRADED_MS = 1500`
- `PROVIDER_HEARTBEAT_FAILURE_BACKOFF_SECS = 10`

探测规则：

- 成功且耗时低于阈值 => `healthy`
- 成功但耗时超阈值 => `degraded`
- 401/403 => `auth_failed`
- 4xx 配置类错误 => `misconfigured`
- 超时、连接拒绝、DNS、TLS => `unreachable`

### 3. Provider 探针抽象
不修改 `LlmClient::stream()` 签名，新增更窄的探针抽象，例如：

- `ProviderHealthProbe`
  - `async fn check(&self) -> ProviderHealthSnapshot`

具体 Provider 适配器内部决定 URL、Header 和错误分类：

- OpenAI / compatible：`GET {base_url}/models`
- Anthropic：`GET {base_url}/v1/models`

这样可避免把“真实推理逻辑”和“心跳逻辑”耦合到同一个 trait。

### 4. 广播触发点
以下时机广播 `provider.health.updated`：

1. 首轮探测完成
2. 状态枚举变化
3. 同状态但错误消息发生变化
4. 配置更新导致 provider 集合重建

客户端首次连接时，还需要主动下发 `provider.health.snapshot`，避免 UI 等待下一轮定时器。

### 5. `project_dir` 注入 runtime
`RuntimeSnapshotAssembler::assemble_session_runtime()` 读取 `ControlState.project_dir` 并写入 `SessionRuntimeSnapshot.project_dir`。

关键要求：

- 该字段与工具执行环境使用同一来源
- 不重新做字符串猜测或前端拼接
- Windows 路径保留原生绝对路径，UI 层再决定展示裁剪

### 6. 配置更新联动
现有 `config.update` 成功后，应触发：

1. 重建心跳探测集合
2. 立即执行一次主动探测
3. 广播最新状态

避免用户刚改 API Key 后仍看到旧状态 30 秒以上。

## 测试案例
1. 正常路径：OpenAI-compatible provider 探测成功，状态变为 `healthy` 并带 `latency_ms`。
2. 正常路径：Anthropic provider 返回 401，状态分类为 `auth_failed`。
3. 正常路径：配置更新后立即触发一次新探测，并广播 `provider.health.updated`。
4. 边界路径：同一 provider 同时用于 orchestration/execution，状态缓存不会互相覆盖。
5. 边界路径：未配置 API key 或 base url 时，状态为 `misconfigured`，而不是 panic。
6. 正常路径：`session.runtime.response` / `session.runtime.updated` 包含 `project_dir`。
