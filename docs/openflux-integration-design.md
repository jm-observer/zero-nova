# OpenFlux + Zero-Nova Integration Design

## 1. Overview

将 zero-nova agent runtime 集成到 OpenFlux 桌面应用中，通过在 Tauri 层实现一个兼容 OpenFlux WebSocket 协议的 Rust Gateway，替代原有的 Node.js Gateway sidecar，前端零改动。

## 2. Current Architecture (OpenFlux)

```
┌──────────────────┐   WebSocket (ws://localhost:18801)  ┌────────────────────┐
│  Frontend (TS)   │ ◄─────────────────────────────────► │  Node.js Gateway   │
│                  │   Protocol: {type, id, payload}     │  (Sidecar Process)  │
│  gateway-client  │                                     │  ├── agent/loop     │
│  evolution-ui    │                                     │  ├── llm/           │
│  markdown/voice  │                                     │  ├── tools/         │
└──────────────────┘                                     │  └── sessions/     │
        │                                                └────────────────────┘
        │ Tauri IPC                                              ▲
        ▼                                                        │
┌──────────────────┐   spawn/kill child process                  │
│  Tauri Shell     │ ────────────────────────────────────────────┘
│  (Rust)          │
│  commands/       │
│  gateway.rs      │   管理 Gateway 生命周期
│  config.rs       │   读取 openflux.yaml
└──────────────────┘
```

### 2.1 WebSocket Protocol (Frontend ↔ Gateway)

所有消息格式统一为：

```json
{
  "type": "message_type",
  "id": "uuid (request/response correlation)",
  "payload": { ... }
}
```

核心消息类型：

| Type              | Direction        | Description                        |
|-------------------|------------------|------------------------------------|
| `auth`            | Client → Server  | Token 认证                         |
| `auth.success`    | Server → Client  | 认证成功                            |
| `chat`            | Client → Server  | 发送聊天消息                        |
| `chat.start`      | Server → Client  | Agent 开始处理                      |
| `chat.progress`   | Server → Client  | 流式进度 (token/tool/iteration)     |
| `chat.complete`   | Server → Client  | Agent 回复完成                      |
| `chat.error`      | Server → Client  | 执行出错                            |
| `sessions.list`   | Client → Server  | 列出会话                            |
| `sessions.get`    | Client → Server  | 获取会话详情+消息历史                |
| `sessions.create` | Client → Server  | 创建新会话                          |
| `agents.list`     | Client → Server  | 列出可用 Agent                      |

### 2.2 Gateway Agent Loop Callbacks

Node.js Gateway 中 Agent 执行时通过回调发送进度：

| Callback          | 触发时机           | 映射的 WebSocket 消息          |
|-------------------|--------------------|-------------------------------|
| `onToken(char)`   | 逐字符流式输出      | `chat.progress` + token       |
| `onToolStart()`   | 工具调用开始        | `chat.progress` + tool_start  |
| `onToolCall()`    | 工具调用完成        | `chat.progress` + tool_call   |
| `onIteration()`   | 每轮 LLM 响应      | `chat.progress` + iteration   |
| `onThinking()`    | 思考内容提取        | `chat.progress` + thinking    |

## 3. Target Architecture

```
┌──────────────────┐   WebSocket (ws://localhost:18801)
│  Frontend (TS)   │ ◄─────────────────────────────────┐
│  (UNCHANGED)     │   Protocol: {type, id, payload}    │
│                  │   完全不改动                        │
└──────────────────┘                                    │
        │ Tauri IPC                                     │
        ▼                                               │
┌───────────────────────────────────────────────────────┤
│  Tauri Shell (Rust)                                   │
│                                                       │
│  ┌─────────────────────────────────────────────────┐  │
│  │  nova-gateway (新模块, Rust WebSocket Server)    │  │
│  │                                                  │  │
│  │  ┌──────────────┐    ┌───────────────────────┐  │  │
│  │  │  WS Server   │───►│  Message Router       │  │  │
│  │  │  (tungstenite)│    │  type → handler       │  │  │
│  │  └──────────────┘    └───────┬───────────────┘  │  │
│  │                              │                   │  │
│  │                    ┌─────────▼─────────┐         │  │
│  │                    │  Nova Bridge      │         │  │
│  │                    │                   │         │  │
│  │                    │  ┌─────────────┐  │         │  │
│  │                    │  │ zero-nova   │  │         │  │
│  │                    │  │ AgentRuntime│  │         │  │
│  │                    │  │ ToolRegistry│  │         │  │
│  │                    │  │ LlmClient   │  │         │  │
│  │                    │  └─────────────┘  │         │  │
│  │                    │                   │         │  │
│  │                    │  SessionStore     │         │  │
│  │                    │  (in-memory)      │         │  │
│  │                    └───────────────────┘         │  │
│  └─────────────────────────────────────────────────┘  │
│                                                       │
│  commands/gateway.rs  (改造: spawn WS server 而非 Node)│
└───────────────────────────────────────────────────────┘
```

### 3.1 Channel 模式设计

在 `commands/gateway.rs` 中，原来是 spawn Node.js 子进程，改为提供两种 channel：

```rust
pub enum GatewayChannel {
    /// 原有模式：启动 Node.js sidecar
    NodeSidecar,
    /// 新增模式：进程内 Rust WebSocket server + zero-nova
    NovaChannel,
}
```

通过 `openflux.yaml` 配置选择：

```yaml
gateway:
  channel: "nova"        # "nova" | "node"
  host: "localhost"
  port: 18801

nova:
  model: "claude-sonnet-4-20250514"
  base_url: "https://api.anthropic.com"
  max_iterations: 10
  max_tokens: 8192
```

## 4. Core Components Design

### 4.1 Nova Gateway (WS Server)

新建模块 `src-tauri/src/nova_gateway/`，实现兼容 OpenFlux 协议的 WebSocket server。

```
src-tauri/src/nova_gateway/
├── mod.rs              # 模块入口，启动 WS server
├── server.rs           # WebSocket server, 连接管理
├── router.rs           # 消息路由 (type → handler)
├── bridge.rs           # zero-nova AgentEvent ↔ OpenFlux Protocol 转换
├── session.rs          # 会话存储 (in-memory)
└── protocol.rs         # 协议类型定义
```

### 4.2 Protocol Types (protocol.rs)

```rust
use serde::{Deserialize, Serialize};

/// 统一消息格式 (与 OpenFlux 前端完全一致)
#[derive(Debug, Serialize, Deserialize)]
pub struct GatewayMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

/// chat.progress 的 payload 子类型
#[derive(Debug, Serialize)]
#[serde(tag = "kind")]
pub enum ChatProgress {
    Token { token: String },
    ToolStart { name: String, input: serde_json::Value },
    ToolEnd { name: String, output: String, is_error: bool },
    Iteration { iteration: u32, response: String },
    Thinking { content: String },
}
```

### 4.3 Bridge (bridge.rs) - 核心转换层

将 zero-nova 的 `AgentEvent` 转换为 OpenFlux WebSocket 消息：

```rust
use zero_nova::event::AgentEvent;

/// AgentEvent → GatewayMessage 映射
///
///  AgentEvent::TextDelta(text)
///    → { type: "chat.progress", id, payload: { kind: "token", token: text } }
///
///  AgentEvent::ToolStart { id, name, input }
///    → { type: "chat.progress", id, payload: { kind: "tool_start", name, input } }
///
///  AgentEvent::ToolEnd { id, name, output, is_error }
///    → { type: "chat.progress", id, payload: { kind: "tool_end", name, output, is_error } }
///
///  AgentEvent::TurnComplete { new_messages, usage }
///    → { type: "chat.complete", id, payload: { messages, usage } }
///
///  AgentEvent::Error(e)
///    → { type: "chat.error", id, payload: { message: e.to_string() } }

pub fn agent_event_to_ws_message(
    event: &AgentEvent,
    request_id: &str,
) -> GatewayMessage {
    match event {
        AgentEvent::TextDelta(text) => GatewayMessage {
            msg_type: "chat.progress".into(),
            id: Some(request_id.into()),
            payload: Some(serde_json::json!({
                "kind": "token",
                "token": text,
            })),
        },
        AgentEvent::ToolStart { name, input, .. } => GatewayMessage {
            msg_type: "chat.progress".into(),
            id: Some(request_id.into()),
            payload: Some(serde_json::json!({
                "kind": "tool_start",
                "name": name,
                "input": input,
            })),
        },
        AgentEvent::ToolEnd { name, output, is_error, .. } => GatewayMessage {
            msg_type: "chat.progress".into(),
            id: Some(request_id.into()),
            payload: Some(serde_json::json!({
                "kind": "tool_end",
                "name": name,
                "output": output,
                "is_error": is_error,
            })),
        },
        AgentEvent::TurnComplete { usage, .. } => GatewayMessage {
            msg_type: "chat.complete".into(),
            id: Some(request_id.into()),
            payload: Some(serde_json::json!({
                "usage": {
                    "input_tokens": usage.input_tokens,
                    "output_tokens": usage.output_tokens,
                }
            })),
        },
        AgentEvent::Error(e) => GatewayMessage {
            msg_type: "chat.error".into(),
            id: Some(request_id.into()),
            payload: Some(serde_json::json!({
                "message": e.to_string(),
            })),
        },
    }
}
```

### 4.4 Message Router (router.rs)

```rust
/// 消息路由表
///
/// "auth"            → handle_auth()        — 简化: 直接返回 auth.success
/// "chat"            → handle_chat()        — 核心: 调用 Nova Bridge
/// "sessions.list"   → handle_sessions()    — 返回内存中的会话列表
/// "sessions.get"    → handle_session_get() — 返回会话消息历史
/// "sessions.create" → handle_session_new() — 创建新会话
/// "agents.list"     → handle_agents()      — 返回 zero-nova agent 信息
/// _                 → 返回 error 消息

pub async fn route_message(
    msg: GatewayMessage,
    state: &AppState,
    ws_tx: &WsSender,
) {
    match msg.msg_type.as_str() {
        "auth"            => handle_auth(msg, ws_tx).await,
        "chat"            => handle_chat(msg, state, ws_tx).await,
        "sessions.list"   => handle_sessions_list(msg, state, ws_tx).await,
        "sessions.get"    => handle_session_get(msg, state, ws_tx).await,
        "sessions.create" => handle_session_create(msg, state, ws_tx).await,
        "agents.list"     => handle_agents_list(msg, state, ws_tx).await,
        other             => send_error(ws_tx, msg.id, format!("Unknown type: {other}")).await,
    }
}
```

### 4.5 Chat Handler (核心流程)

```rust
/// handle_chat 核心流程:
///
/// 1. 解析 payload: { input, sessionId?, agentId? }
/// 2. 从 SessionStore 加载历史消息
/// 3. 发送 { type: "chat.start", id } 给前端
/// 4. 调用 agent.run_turn(history, input, event_tx)
/// 5. 启动转发任务: event_rx → bridge 转换 → ws_tx 发送
/// 6. run_turn 完成后, 保存新消息到 SessionStore
/// 7. 发送 { type: "chat.complete", id, payload } 给前端

async fn handle_chat(
    msg: GatewayMessage,
    state: &AppState,
    ws_tx: &WsSender,
) {
    let request_id = msg.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let payload = msg.payload.unwrap_or_default();
    let input = payload["input"].as_str().unwrap_or("");
    let session_id = payload["sessionId"].as_str().unwrap_or("default");

    // 1. 加载会话历史
    let history = state.sessions.get_messages(session_id);

    // 2. 通知前端开始
    send_message(ws_tx, "chat.start", &request_id, json!({})).await;

    // 3. 创建事件通道
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(100);

    // 4. 转发任务: AgentEvent → WS message
    let ws_tx_clone = ws_tx.clone();
    let rid = request_id.clone();
    let forwarder = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let ws_msg = bridge::agent_event_to_ws_message(&event, &rid);
            let _ = ws_tx_clone.send(ws_msg).await;
        }
    });

    // 5. 执行 Agent
    let agent = state.agent.lock().await;
    match agent.run_turn(&history, input, event_tx).await {
        Ok(new_msgs) => {
            forwarder.await.ok();
            state.sessions.append_messages(session_id, &new_msgs);
            send_message(ws_tx, "chat.complete", &request_id, json!({
                "output": extract_text(&new_msgs),
            })).await;
        }
        Err(e) => {
            forwarder.abort();
            send_message(ws_tx, "chat.error", &request_id, json!({
                "message": e.to_string(),
            })).await;
        }
    }
}
```

### 4.6 Session Store (session.rs)

```rust
use std::collections::HashMap;
use tokio::sync::RwLock;
use zero_nova::message::Message;

/// 轻量级内存会话存储
/// 后续可扩展为 SQLite 持久化
pub struct SessionStore {
    sessions: RwLock<HashMap<String, Session>>,
}

pub struct Session {
    pub id: String,
    pub title: String,
    pub messages: Vec<Message>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}
```

### 4.7 Server Lifecycle (与 Tauri 集成)

```rust
// commands/gateway.rs 改造

#[tauri::command]
pub async fn start_gateway(
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    match state.config.gateway.channel.as_str() {
        "nova" => {
            // 进程内启动 Rust WS server
            nova_gateway::start_server(
                &state.config.gateway.host,
                state.config.gateway.port,
                state.nova_state.clone(),
            ).await.map_err(|e| e.to_string())
        }
        "node" | _ => {
            // 保留原有 Node.js sidecar 启动逻辑
            start_node_sidecar(&state).await.map_err(|e| e.to_string())
        }
    }
}
```

## 5. zero-nova Side Changes

zero-nova 库本身需要的改动极小：

### 5.1 AgentEvent 加 Serialize + Clone

```rust
// src/event.rs
// 现在:  #[derive(Debug)]
// 改为:
#[derive(Debug, Clone, Serialize)]
pub enum AgentEvent { ... }
```

需要对 `Error` variant 做特殊处理 (anyhow::Error 不实现 Serialize):

```rust
#[derive(Debug, Clone, Serialize)]
pub enum AgentEvent {
    TextDelta(String),
    ToolStart { id: String, name: String, input: serde_json::Value },
    ToolEnd { id: String, name: String, output: String, is_error: bool },
    TurnComplete { new_messages: Vec<Message>, usage: Usage },
    /// 序列化时转为 string
    #[serde(serialize_with = "serialize_error")]
    Error(#[serde(skip)] anyhow::Error),
}
```

或更简单的方案：bridge 层直接 match 处理，不依赖 Serialize。

### 5.2 可选: AgentRuntime 支持 Arc 共享

当前 `run_turn` 需要 `&self`，已经是不可变引用，可以直接用 `Arc<Mutex<AgentRuntime>>` 包装。无需改动 zero-nova 代码。

## 6. Dependency Graph

```
src-tauri/Cargo.toml:

[dependencies]
zero-nova = { path = "../../zero-nova" }    # 核心 agent
tauri = { version = "2", features = [...] }
tokio = { version = "1", features = ["full"] }
tokio-tungstenite = "0.24"                  # WS server
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
```

## 7. Data Flow (Complete Chat Request)

```
Frontend                    Nova Gateway (Rust)              zero-nova
   │                              │                              │
   │ ─── {type:"chat"} ────────► │                              │
   │                              │ parse payload                │
   │                              │ load session history         │
   │ ◄── {type:"chat.start"} ─── │                              │
   │                              │ ── run_turn(history,input) ─►│
   │                              │                              │ LLM stream
   │                              │ ◄─── TextDelta("Hello") ────│
   │ ◄── {chat.progress/token} ──│                              │
   │                              │ ◄─── TextDelta(" world") ───│
   │ ◄── {chat.progress/token} ──│                              │
   │                              │ ◄─── ToolStart{bash} ───────│
   │ ◄── {chat.progress/tool} ───│                              │
   │                              │                              │ execute tool
   │                              │ ◄─── ToolEnd{bash,ok} ──────│
   │ ◄── {chat.progress/tool} ───│                              │
   │                              │ ◄─── TurnComplete{usage} ───│
   │ ◄── {type:"chat.complete"} ─│                              │
   │                              │ save to SessionStore         │
   │                              │                              │
```

## 8. Phase Plans

### Phase 1: Foundation — Protocol & Bridge

**目标**: 建立协议类型和核心转换层，可单独编译测试。

- [ ] 创建 `src-tauri/src/nova_gateway/` 模块结构
- [ ] 实现 `protocol.rs`: GatewayMessage, ChatProgress 等类型定义
- [ ] 实现 `bridge.rs`: AgentEvent → GatewayMessage 转换函数
- [ ] 修改 zero-nova `event.rs`: AgentEvent 加 Clone 派生
- [ ] 单元测试: bridge 转换逻辑的正确性

### Phase 2: WebSocket Server & Router

**目标**: 实现 WS server 骨架，能接收连接和路由消息。

- [ ] 实现 `server.rs`: tokio-tungstenite WS server，连接管理
- [ ] 实现 `router.rs`: 消息路由分发
- [ ] 实现 `handle_auth`: 简化认证 (直接 success)
- [ ] 实现 `handle_agents_list`: 返回 agent 信息
- [ ] 集成测试: WS 客户端连接 → auth → agents.list

### Phase 3: Session Management

**目标**: 实现会话存储，支持前端的会话管理 UI。

- [ ] 实现 `session.rs`: SessionStore (in-memory HashMap)
- [ ] 实现 `handle_sessions_list` / `handle_session_get` / `handle_session_create`
- [ ] 会话消息持久化 (先 in-memory，后续可扩展 SQLite)
- [ ] 单元测试: 会话 CRUD

### Phase 4: Chat Integration

**目标**: 核心聊天功能，前端可以正常对话。

- [ ] 实现 `handle_chat`: 完整聊天流程
- [ ] 集成 SessionStore: 加载历史 + 保存新消息
- [ ] 事件转发: mpsc channel → bridge → WS 发送
- [ ] 错误处理: agent 执行失败 → chat.error
- [ ] 端到端测试: 发消息 → 流式收到 progress → complete

### Phase 5: Tauri Integration & Channel Switch

**目标**: 集成到 Tauri 生命周期，支持 channel 切换。

- [ ] 改造 `commands/gateway.rs`: 支持 nova/node 双 channel
- [ ] 扩展 `config.rs`: 读取 nova 配置 (model, base_url 等)
- [ ] 改造 `lib.rs`: 根据配置初始化 AgentRuntime
- [ ] Gateway 生命周期管理: start/stop/restart commands
- [ ] 集成测试: Tauri 启动 → WS server ready → 前端连接成功

### Phase 6: Polish & Production Ready

**目标**: 完善细节，确保生产可用。

- [ ] 前端兼容性验证: 所有 message type 对齐
- [ ] Chat progress 格式微调: 对齐 OpenFlux 前端渲染器期望的字段
- [ ] 多会话并发支持
- [ ] 优雅关闭: Ctrl+C / 窗口关闭时正确断开 WS 和停止 agent
- [ ] 日志: 请求/响应日志，错误追踪
- [ ] 配置文档和使用说明
