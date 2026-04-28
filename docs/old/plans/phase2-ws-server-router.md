# Phase 2：网关主链路与路由收敛

> 前置依赖：Phase 1  
> 基线代码：`src/gateway/server.rs`、`src/gateway/router.rs`、`src/gateway/mod.rs`

## 1. 目标

当前仓库已经有可运行的 gateway，但 `router` 混合了：

- 真正支持的 handler
- 大量 stub / 透传响应
- 会话逻辑
- chat 调度逻辑

第二阶段的目标不是“再建一个 server”，而是：  
**把现有 gateway 主链路整理成可扩展的后端内核。**

## 2. 当前问题

基于 `src/gateway/router.rs` 现状，主要问题是：

1. `handle_message()` 的 match 分支过大，已经承担了过多职责。
2. 很多消息类型只是临时响应 JSON，缺少明确“支持/不支持”边界。
3. `handle_chat()`、`sessions.*`、`agents.*` 都耦合在一个文件里。
4. `server.rs` 为每个入站消息直接 `spawn` handler，缺少统一的执行边界。
5. `mod.rs::start_server()` 同时做：
   - tools 初始化
   - prompt 初始化
   - runtime 初始化
   - app state 组装
   - server 启动

这会阻碍后续引入 control plane、workflow、multi-agent。

## 3. 本 phase 范围

### 3.1 要做

- 拆分 `router` 责任
- 收敛 AppState
- 固定 server -> router -> handler 调用链
- 明确哪些消息是“后端已支持”，哪些是“显式未实现”

### 3.2 不做

- 不引入 workflow
- 不引入 pending interaction
- 不引入 skill 路由
- 不做持久化

## 4. 设计结论

### 4.1 `handle_message()` 只做一级路由

建议最终形态：

```rust
pub async fn handle_message(...) {
    match msg.envelope {
        MessageEnvelope::Chat(payload) => chat::handle(...).await,
        MessageEnvelope::SessionsList => sessions::handle_list(...).await,
        MessageEnvelope::SessionsMessages(payload) => sessions::handle_get(...).await,
        MessageEnvelope::SessionsCreate(payload) => sessions::handle_create(...).await,
        MessageEnvelope::AgentsList => agents::handle_list(...).await,
        MessageEnvelope::AgentsSwitch(payload) => agents::handle_switch(...).await,
        _ => send_not_supported(...),
    }
}
```

也就是说：

- `router.rs` 保留入口
- 具体 handler 下沉到子模块

### 4.2 对“不支持消息”要明确失败，不要继续堆 stub

当前很多分支会返回“看起来成功但实际没实现”的 payload。  
这对后续后端演进是负资产。

建议改成两类：

- **已支持**：真实 handler
- **未支持**：统一 `ErrorPayload { code: "NOT_IMPLEMENTED" }`

### 4.3 `AppState` 只保留后端真正共享的核心对象

当前：

```rust
pub struct AppState<C: LlmClient> {
    pub agent: AgentRuntime<C>,
    pub sessions: SessionStore,
}
```

这个结构本身还可以，但 Phase 2 要为后续 phase 留位。建议按职责预留：

- `runtime`
- `sessions`
- `agent_registry`（后续）
- `control_plane`（后续）

此时未必实现，但文档里要明确将来的扩展点。

## 5. 实现细节

### 5.1 拆 handler 模块

建议拆成：

```text
src/gateway/
├── router.rs
├── handlers/
│   ├── mod.rs
│   ├── chat.rs
│   ├── sessions.rs
│   ├── agents.rs
│   └── system.rs
```

这一步本身就是给后续 control plane 减压。

### 5.2 `server.rs` 保持纯连接层

`server.rs` 只负责：

- accept websocket
- 收 frame
- parse `GatewayMessage`
- 交给 `handle_message()`
- 发回响应

不要让 `server.rs` 直接理解：

- session
- workflow
- agent switching

### 5.3 `start_server()` 组装逻辑下沉为 builder/init

建议把 `gateway::mod.rs` 里的初始化逻辑拆开：

- `build_tool_registry(&config)`
- `build_system_prompt(&tools)`
- `build_agent_runtime(config, client, tools, prompt)`
- `build_app_state(...)`

这样后续 Phase 4/5 引入新状态不会把 `start_server()` 撑爆。

## 6. 测试方案

### 6.1 Router 单元测试

覆盖：

- 支持消息是否路由到正确 handler
- 未支持消息是否返回统一错误

### 6.2 网关集成测试

覆盖：

- `welcome`
- `auth`
- `sessions.create`
- `sessions.list`
- `agents.list`

注意：这个阶段的 `chat` 可以还保留当前实现，但主要验证路由和错误边界。

## 7. 风险点

### 7.1 继续在 `router.rs` 里堆分支

这会直接阻断后续引入 control plane。

### 7.2 “伪成功”响应继续扩散

要么支持，要么明确不支持。不要再返回占位成功数据。

## 8. 完成定义

- `router` 已拆出清晰 handler 边界
- `server` 只承担连接职责
- 未支持消息统一失败
- 网关主链路可测试

## 9. 给下一阶段的交接信息

Phase 3 将在整理后的 gateway 主链路上，修正 chat 生命周期和 session 写入模型。
