# Phase 2: WebSocket Server & Router

## Goal

实现 WebSocket server 骨架，能接受前端连接、解析消息、路由到对应 handler，返回响应。此阶段不涉及真实 agent 调用，先用 stub handler 验证通信链路。

## Prerequisites

- Phase 1 完成 (protocol.rs, bridge.rs)

## Tasks

### 2.1 实现 server.rs — WS Server

**依赖**: `tokio-tungstenite`

核心职责:
- 在指定 host:port 上监听 TCP
- Accept WebSocket upgrade
- 每个连接 spawn 一个 tokio task 处理消息收发
- 维护连接状态 (authenticated / not)

关键接口:

```rust
pub struct NovaGateway {
    state: Arc<GatewayState>,
}

pub struct GatewayState {
    pub agent: Mutex<AgentRuntime<AnthropicClient>>,
    pub sessions: SessionStore,
    pub config: NovaConfig,
}

impl NovaGateway {
    /// 启动 WS server，返回 JoinHandle 用于后续 shutdown
    pub async fn start(
        host: &str,
        port: u16,
        state: Arc<GatewayState>,
    ) -> Result<tokio::task::JoinHandle<()>>;
}
```

连接处理循环:

```
loop {
    recv ws frame → deserialize GatewayMessage
                  → route_message(msg, state, ws_tx)
                  → handler 通过 ws_tx 发送响应
}
```

### 2.2 实现 router.rs — 消息路由

```rust
pub async fn route_message(
    msg: GatewayMessage,
    state: &GatewayState,
    ws_tx: &WsSender,
) {
    match msg.msg_type.as_str() {
        "auth"            => handle_auth(msg, ws_tx).await,
        "agents.list"     => handle_agents_list(msg, state, ws_tx).await,
        "sessions.list"   => ...,
        "sessions.get"    => ...,
        "sessions.create" => ...,
        "chat"            => ...,  // Phase 4 实现，先 stub
        other             => send_error(ws_tx, msg.id, other).await,
    }
}
```

### 2.3 实现 stub handlers

**handle_auth**: OpenFlux 前端连接后先发 auth，我们简化处理:

```rust
async fn handle_auth(msg: GatewayMessage, ws_tx: &WsSender) {
    // 不做真实验证，直接返回成功
    send(ws_tx, GatewayMessage {
        msg_type: "auth.success".into(),
        id: msg.id,
        payload: Some(json!({ "user": "local" })),
    }).await;
}
```

**handle_agents_list**: 返回 zero-nova 作为唯一 agent:

```rust
async fn handle_agents_list(msg: GatewayMessage, state: &GatewayState, ws_tx: &WsSender) {
    let tools = state.agent.lock().await.tools().tool_definitions();
    send(ws_tx, GatewayMessage {
        msg_type: "agents.list".into(),
        id: msg.id,
        payload: Some(json!([{
            "id": "nova",
            "name": "Zero Nova",
            "description": "Rust-native AI agent",
            "isDefault": true,
            "tools": tools.iter().map(|t| &t.name).collect::<Vec<_>>(),
        }])),
    }).await;
}
```

**handle_chat (stub)**: 先返回固定文本，验证通信链路:

```rust
async fn handle_chat(msg: GatewayMessage, ws_tx: &WsSender) {
    let id = msg.id.clone();
    send(ws_tx, msg_type: "chat.start", id).await;
    send(ws_tx, msg_type: "chat.progress", id, { kind: "token", token: "Hello from Nova!" }).await;
    send(ws_tx, msg_type: "chat.complete", id, { output: "Hello from Nova!" }).await;
}
```

### 2.4 集成测试

使用 `tokio-tungstenite` 客户端编写测试:

1. 连接 WS server
2. 发送 `{type: "auth", payload: {token: "test"}}`
3. 收到 `{type: "auth.success"}`
4. 发送 `{type: "agents.list"}`
5. 收到 agent 列表
6. 发送 `{type: "chat", payload: {input: "hi"}}`
7. 依次收到 chat.start → chat.progress → chat.complete

## New Files

```
src-tauri/src/nova_gateway/
├── mod.rs       # 更新: 加 pub mod server, router
├── server.rs    # NEW
└── router.rs    # NEW
```

## Definition of Done

- [ ] WS server 能在指定端口启动
- [ ] 客户端可连接并完成 auth 握手
- [ ] agents.list 返回正确的 agent 信息
- [ ] chat stub 能完整走通 start → progress → complete 流程
- [ ] 集成测试通过
