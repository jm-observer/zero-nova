# Zero-Nova WebSocket Gateway 设计文档

## 1. Overview

在 zero-nova 库内实现一个独立的 WebSocket Gateway Server 模块（`src/gateway/`），通过 WebSocket 协议对外暴露 AgentRuntime 的能力。该模块作为 library 代码存在，同时提供一个独立的 bin target（`nova_gateway`）用于独立运行和测试。

**核心策略**：先实现独立可运行的 WS Gateway，验证协议兼容性后，再嵌入 Tauri（见 `openflux-integration-design.md`）。

### 1.1 与 OpenFlux 集成方案的关系

本文档是 `openflux-integration-design.md` 的前置实现。两者的关系：

```
本文档 (Phase A)                        openflux-integration-design.md (Phase B)
─────────────────                        ──────────────────────────────────────────
在 zero-nova 内实现 gateway 模块          在 OpenFlux Tauri 中引用 zero_nova::gateway
独立 bin: nova_gateway                    嵌入 Tauri 生命周期
cargo run --bin nova_gateway             commands/gateway.rs 调用 start_server()
手动测试 / WS 客户端测试                  OpenFlux 前端直连
```

Phase B 的代码位置从 `src-tauri/src/nova_gateway/` 变为直接 `use zero_nova::gateway`，核心逻辑不重复。

## 2. Architecture

```
┌──────────────────────────────────────────────────────────┐
│  zero-nova crate                                         │
│                                                          │
│  src/gateway/         (feature = "gateway")              │
│  ┌────────────────────────────────────────────────────┐  │
│  │                                                    │  │
│  │  ┌──────────┐   ┌────────────┐   ┌─────────────┐  │  │
│  │  │ server   │──►│  router    │──►│  bridge     │  │  │
│  │  │ (WS)     │   │  (dispatch)│   │  (convert)  │  │  │
│  │  └──────────┘   └─────┬──────┘   └──────┬──────┘  │  │
│  │                       │                  │         │  │
│  │                 ┌─────▼──────┐    ┌──────▼──────┐  │  │
│  │                 │  session   │    │ AgentRuntime │  │  │
│  │                 │  (store)   │    │ (zero-nova)  │  │  │
│  │                 └────────────┘    └─────────────┘  │  │
│  │                                                    │  │
│  │  protocol.rs   — 协议类型定义                       │  │
│  └────────────────────────────────────────────────────┘  │
│                                                          │
│  src/bin/nova_gateway.rs   (thin entry point)            │
└──────────────────────────────────────────────────────────┘

         ▲ WebSocket (ws://localhost:9090)
         │
    WS Client (OpenFlux 前端 / 测试客户端 / 任意 WS 工具)
```

### 2.1 模块结构

```
src/gateway/
├── mod.rs              # 模块入口, GatewayConfig, start_server()
├── protocol.rs         # 协议类型定义 (GatewayMessage, payload types)
├── bridge.rs           # AgentEvent → GatewayMessage 转换
├── router.rs           # 消息路由 + handler 实现
├── session.rs          # SessionStore (in-memory)
└── server.rs           # WS server, 连接管理

src/bin/
├── nova_cli.rs         # 现有 CLI (不改动)
└── nova_gateway.rs     # 新增: 独立 WS gateway 入口
```

## 3. Protocol Design

### 3.1 消息格式

与 OpenFlux 前端协议完全一致，所有消息采用统一信封格式：

```json
{
  "type": "message_type",
  "id": "uuid (request/response correlation)",
  "payload": { ... }
}
```

### 3.2 消息类型总表

| Type              | Direction        | Description                  |
|-------------------|------------------|------------------------------|
| `auth`            | Client → Server  | 认证请求                      |
| `auth.success`    | Server → Client  | 认证成功                      |
| `chat`            | Client → Server  | 发送聊天消息                   |
| `chat.start`      | Server → Client  | Agent 开始处理                 |
| `chat.progress`   | Server → Client  | 流式进度 (token/tool/thinking) |
| `chat.complete`   | Server → Client  | 回复完成                      |
| `chat.error`      | Server → Client  | 执行出错                      |
| `sessions.list`   | Client → Server  | 列出会话                      |
| `sessions.get`    | Client → Server  | 获取会话详情 + 消息历史         |
| `sessions.create` | Client → Server  | 创建新会话                    |
| `agents.list`     | Client → Server  | 列出可用 Agent                |

### 3.3 Payload 定义

#### `auth` (Client → Server)

```json
{ "token": "optional-token" }
```

#### `auth.success` (Server → Client)

```json
{}
```

#### `chat` (Client → Server)

```json
{
  "session_id": "uuid",
  "message": "user input text",
  "model": "optional-model-override"
}
```

#### `chat.start` (Server → Client)

```json
{ "session_id": "uuid" }
```

#### `chat.progress` (Server → Client)

payload 使用 `kind` 字段区分子类型：

```json
// kind: token
{ "session_id": "uuid", "kind": "token", "text": "Hello" }

// kind: tool_start
{ "session_id": "uuid", "kind": "tool_start", "id": "tool-use-id", "name": "bash", "input": {...} }

// kind: tool_end
{ "session_id": "uuid", "kind": "tool_end", "id": "tool-use-id", "name": "bash", "output": "...", "is_error": false }

// kind: thinking
{ "session_id": "uuid", "kind": "thinking", "text": "..." }
```

#### `chat.complete` (Server → Client)

```json
{
  "session_id": "uuid",
  "messages": [ /* Vec<Message> — zero-nova Message 类型直接序列化 */ ],
  "usage": { "input_tokens": 100, "output_tokens": 50 }
}
```

#### `chat.error` (Server → Client)

```json
{
  "session_id": "uuid",
  "error": "error description",
  "code": "ERROR_CODE"
}
```

#### `error` (Universal Error, Server → Client)

用于非 chat 场景的通用错误返回。

```json
{
  "message": "error description",
  "code": "ERROR_CODE"
}
```

#### `sessions.create` (Client → Server)

```json
{ "name": "optional session name" }
```

#### `sessions.create` response (Server → Client)

```json
{
  "id": "uuid",
  "name": "Session abc",
  "message_count": 0,
  "created_at": "1713200000"
}
```

#### `sessions.list` response (Server → Client)

```json
{
  "sessions": [
    { "id": "uuid", "name": "...", "message_count": 5, "created_at": "..." }
  ]
}
```

#### `sessions.get` (Client → Server)

```json
{ "session_id": "uuid" }
```

#### `sessions.get` response (Server → Client)

```json
{
  "id": "uuid",
  "name": "...",
  "messages": [ /* Vec<Message> */ ],
  "created_at": 1713200000000
}
```

#### `agents.list` response (Server → Client)

```json
{
  "agents": [
    { "id": "nova", "name": "Zero-Nova", "description": "...", "tools": ["bash", "read_file", ...] }
  ]
}
```

## 4. Core Components Design

### 4.1 Protocol Types (protocol.rs)

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 统一消息信封 (与 OpenFlux 前端完全一致)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub id: String,
    pub payload: Value,
}

// --- Inbound payloads ---

#[derive(Debug, Deserialize)]
pub struct AuthPayload {
    pub token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatPayload {
    pub session_id: String,
    pub message: String,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SessionGetPayload {
    pub session_id: String,
}

#[derive(Debug, Deserialize)]
pub struct SessionCreatePayload {
    pub name: Option<String>,
}

// --- Outbound payloads ---

#[derive(Debug, Clone, Serialize)]
pub struct ChatProgressPayload {
    pub session_id: String,
    #[serde(flatten)]
    pub progress: ProgressType,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProgressType {
    Token { text: String },
    ToolStart { id: String, name: String, input: Value },
    ToolEnd { id: String, name: String, output: String, is_error: bool },
    Thinking { text: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletePayload {
    pub session_id: String,
    pub messages: Vec<crate::message::Message>,
    pub usage: crate::provider::types::Usage,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatErrorPayload {
    pub session_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub message_count: usize,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
}
```

### 4.2 Bridge (bridge.rs)

将 zero-nova 的 `AgentEvent` 转换为 `GatewayMessage`。消费 event by value（`AgentEvent` 不实现 Clone，`anyhow::Error` 不可 Clone）。

```rust
/// AgentEvent → GatewayMessage 映射关系:
///
///  AgentEvent::TextDelta(text)
///    → { type: "chat.progress", payload: { kind: "token", text, session_id } }
///
///  AgentEvent::ToolStart { id, name, input }
///    → { type: "chat.progress", payload: { kind: "tool_start", ... } }
///
///  AgentEvent::ToolEnd { id, name, output, is_error }
///    → { type: "chat.progress", payload: { kind: "tool_end", ... } }
///
///  AgentEvent::TurnComplete { new_messages, usage }
///    → { type: "chat.complete", payload: { messages, usage, session_id } }
///
///  AgentEvent::Error(e)
///    → { type: "chat.error", payload: { error: e.to_string(), session_id } }

pub fn agent_event_to_gateway(
    event: AgentEvent,       // 取 ownership, 不要求 Clone
    request_id: &str,
    session_id: &str,
) -> GatewayMessage { ... }
```

**关键设计决策**: 函数签名取 `AgentEvent` by value（非 `&AgentEvent`），因为 `mpsc::Receiver::recv()` 返回 owned value，且 `anyhow::Error` 不支持 Clone。这样 zero-nova 的 `event.rs` 无需任何修改。

### 4.3 Session Store (session.rs)

```rust
/// 轻量级内存会话存储
/// history 使用 RwLock 包装以支持 Arc 共享下的可变访问
/// chat_lock 确保同一会话内的聊天请求串行执行

pub struct Session {
    pub id: String,
    pub name: String,
    pub history: RwLock<Vec<Message>>,
    pub created_at: String,             // unix timestamp string, 无需 chrono 依赖
    pub chat_lock: Mutex<()>,           // 每个 session 的聊天串行锁
}

pub struct SessionStore {
    sessions: RwLock<HashMap<String, Arc<Session>>>,
}

impl SessionStore {
    pub fn new() -> Self;
    pub async fn create(&self, name: Option<String>) -> Arc<Session>;
    pub async fn get(&self, id: &str) -> Option<Arc<Session>>;
    pub async fn list(&self) -> Vec<Arc<Session>>;
}
```

**并发模型**:
- 多个 WS 连接可同时访问不同 session（`SessionStore` 用 `RwLock`）
- 同一 session 的 chat 请求通过 `chat_lock: Mutex<()>` 串行化
- session 的 history 通过 `RwLock<Vec<Message>>` 支持并发读、排他写

### 4.4 Router (router.rs)

```rust
/// 共享应用状态
pub struct AppState {
    pub agent: AgentRuntime<AnthropicClient>,
    pub sessions: SessionStore,
}

/// 消息路由
/// "auth"            → handle_auth()           简化: 直接返回 auth.success
/// "chat"            → handle_chat()           核心: 调用 AgentRuntime
/// "sessions.list"   → handle_sessions_list()
/// "sessions.get"    → handle_session_get()
/// "sessions.create" → handle_session_create()
/// "agents.list"     → handle_agents_list()    返回 agent + 工具列表
/// _                 → error response

pub async fn handle_message(
    msg: GatewayMessage,
    state: &Arc<AppState>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
);
```

#### Chat Handler 核心流程

```
1. 解析 ChatPayload: { session_id, message }
2. 从 SessionStore 获取 session (不存在则返回 error)
3. 获取 session.chat_lock (防止并发 chat)
4. 发送 chat.start 给前端
5. 读取 session.history (clone)
6. 创建 mpsc channel (event_tx, event_rx)
7. spawn 转发任务: event_rx.recv() → bridge 转换 → outbound_tx.send()
8. 调用 agent.run_turn(&history, &message, event_tx)
9. 成功: 追加 user message + new_messages 到 session.history
10. 失败: 发送 chat.error
11. 等待转发任务结束
```

**注意**: `run_turn` 内部已经会在本地 copy 中追加 user message，但不影响 session store。所以 step 9 中需要手动把 user message 和返回的 new_messages 都追加到 session 的持久化 history 中。

### 4.5 Server (server.rs)

```rust
/// 启动 WS server，接受连接，管理读写分离
pub async fn run_server(addr: SocketAddr, state: Arc<AppState>) -> Result<()>;
```

每个连接的处理流程：

```
1. TcpListener::accept()
2. tokio_tungstenite::accept_async(stream)
3. split() → ws_sink + ws_source
4. 创建 unbounded_channel (outbound_tx, outbound_rx)
5. spawn write task: outbound_rx → ws_sink.send()
6. read loop:
   - ws_source.next() → parse GatewayMessage
   - spawn handle_message(msg, state, outbound_tx)  // 不阻塞读循环
7. 连接关闭: drop outbound_tx, 等待 write task 结束
```

**关键**: 每个 inbound message 的 handler 都 spawn 在独立 task 中，这样长时间运行的 chat 请求不会阻塞同一连接上的其他消息（如 sessions.list）。

### 4.6 Gateway Module Entry (mod.rs)

```rust
/// Gateway 配置
pub struct GatewayConfig {
    pub host: String,           // default: "127.0.0.1"
    pub port: u16,              // default: 9090
    pub model: String,          // default: "gpt-oss-120b"
    pub max_tokens: u32,        // default: 8192
    pub max_iterations: usize,  // default: 10
    pub api_key: Option<String>,    // None = 从环境变量读取
    pub base_url: Option<String>,   // None = 使用默认
}

/// 主入口，供 bin target 和未来 Tauri 调用
/// 内部构建 AnthropicClient → ToolRegistry → SystemPromptBuilder → AgentRuntime → AppState
/// 然后调用 server::run_server()
pub async fn start_server(config: GatewayConfig) -> Result<()>;
```

### 4.7 Binary Entry (nova_gateway.rs)

```rust
/// 独立运行的 WS gateway binary
/// 解析命令行参数 (clap): --host, --port, --model, --max-tokens, --base-url
/// 初始化 logger
/// 调用 zero_nova::gateway::start_server(config)
```

## 5. Cargo.toml Changes

```toml
# 新增可选依赖
uuid = { version = "1", features = ["v4", "serde"], optional = true }

# 新增 feature
gateway = ["uuid"]
# default 不包含 gateway, 保持核心库轻量

# 新增 bin target
[[bin]]
name = "nova_gateway"
path = "src/bin/nova_gateway.rs"
required-features = ["gateway", "cli"]
```

不需要新增 `tokio-tungstenite`、`futures-util`、`serde`、`serde_json` — 已有。

`clap` 通过 `cli` feature 已可用，`nova_gateway` bin 复用。

## 6. AgentEvent 与 zero-nova 改动

### 不需要改动 zero-nova 核心代码

bridge 层通过 `match event { ... }` 消费 `AgentEvent` by value，完全不依赖 Serialize 或 Clone。`anyhow::Error` 通过 `format!("{:#}", e)` 转为 String。

这与 `openflux-integration-design.md` 中 5.1 节的方案不同——那里建议给 `AgentEvent` 加 `Clone + Serialize` 派生。本文档的方案避免了修改核心 event 类型，更符合关注点分离原则。

## 7. Data Flow

```
WS Client                  nova_gateway                        zero-nova
   │                              │                                │
   │ ── {type:"auth"} ──────────►│                                │
   │ ◄── {type:"auth.success"} ──│                                │
   │                              │                                │
   │ ── {type:"sessions.create"}─►│                                │
   │ ◄── {type:"sessions.create",│                                │
   │      payload:{id,name}} ────│                                │
   │                              │                                │
   │ ── {type:"chat", payload:   │                                │
   │     {session_id,message}} ──►│                                │
   │                              │ session.chat_lock.lock()       │
   │                              │ history = session.history      │
   │ ◄── {type:"chat.start"} ────│                                │
   │                              │ ── run_turn(history,msg,tx) ──►│
   │                              │                                │ LLM stream
   │                              │ ◄── AgentEvent::TextDelta ────│
   │ ◄── {chat.progress/token} ──│ (bridge converts)              │
   │                              │ ◄── AgentEvent::ToolStart ────│
   │ ◄── {chat.progress/tool_start}│                               │
   │                              │                                │ execute tool
   │                              │ ◄── AgentEvent::ToolEnd ──────│
   │ ◄── {chat.progress/tool_end}│                                │
   │                              │ ◄── AgentEvent::TextDelta ────│
   │ ◄── {chat.progress/token} ──│                                │
   │                              │ ◄── AgentEvent::TurnComplete ─│
   │ ◄── {type:"chat.complete"} ─│                                │
   │                              │ session.history.append(msgs)   │
   │                              │ session.chat_lock.release()    │
```

## 8. Concurrency Model

```
                    ┌─────────────────────┐
                    │  Arc<AppState>       │
                    │  ├── AgentRuntime    │  ← &self (immutable), 支持并发 run_turn
                    │  └── SessionStore    │  ← RwLock<HashMap>
                    └──────────┬──────────┘
                               │ clone Arc
                    ┌──────────┼──────────┐
                    ▼          ▼          ▼
              Connection 1  Connection 2  Connection 3
              (WS client)  (WS client)   (WS client)
                    │          │          │
                    ▼          ▼          ▼
              handle_message  ...        ...
              (spawned task per inbound message)
```

- `AgentRuntime::run_turn(&self, ...)` — 不可变引用，天然支持多 session 并发
- 同一 session 的 `chat_lock: Mutex<()>` 保证聊天请求串行
- 不同 session 可完全并行执行
- WS 连接的 read loop 不被 handler 阻塞（handler 均 spawn 独立 task）

## 9. Phase Plan

### Phase 1: Protocol Types + Session Store

**目标**: 基础类型和存储，可单独编译和单元测试。

- [ ] 修改 `Cargo.toml`: 添加 `uuid` 依赖和 `gateway` feature
- [ ] 创建 `src/gateway/protocol.rs`: GatewayMessage, payload types
- [ ] 创建 `src/gateway/session.rs`: SessionStore, Session
- [ ] 单元测试: GatewayMessage 序列化 round-trip
- [ ] 单元测试: SessionStore CRUD

### Phase 2: Bridge

**目标**: AgentEvent → GatewayMessage 转换逻辑，独立可测。

- [ ] 创建 `src/gateway/bridge.rs`: `agent_event_to_gateway()` 函数
- [ ] 单元测试: 每个 AgentEvent variant 的转换正确性
- [ ] 验证 Error variant 的 `anyhow::Error` → String 转换

### Phase 3: Router + Handlers

**目标**: 消息路由和所有 handler 实现。

- [ ] 创建 `src/gateway/router.rs`: AppState, handle_message, 所有 handler
- [ ] handle_auth — 简化认证
- [ ] handle_sessions_list / handle_session_get / handle_session_create
- [ ] handle_agents_list — 返回 agent 信息 + 工具列表
- [ ] handle_chat — 核心聊天流程（事件通道 + 转发 + session 历史管理）

### Phase 4: WS Server + Binary

**目标**: 完整可运行的独立 gateway。

- [ ] 创建 `src/gateway/server.rs`: WS server, 连接管理
- [ ] 创建 `src/gateway/mod.rs`: GatewayConfig, start_server()
- [ ] 修改 `src/lib.rs`: feature-gated `pub mod gateway`
- [ ] 创建 `src/bin/nova_gateway.rs`: CLI 入口
- [ ] 手动测试: `cargo run --bin nova_gateway --features gateway`
- [ ] 使用 WS 客户端工具 (websocat / wscat) 验证: auth → sessions.create → chat

### Phase 5: 对齐 OpenFlux 前端

**目标**: 确保 OpenFlux 前端可以直连 nova_gateway。

- [ ] 对齐 `chat.progress` payload 格式与 OpenFlux 前端 `gateway-client` 的期望
    - [ ] 确保 `id` 为稳定的 UUID
    - [ ] 确保 `tool_start` 的 `input` 符合前端渲染要求
- [ ] 统一时间戳格式为 **毫秒 (ms)** 数值
- [ ] 完善错误处理：引入统一的 `ErrorPayload` 和错误码 (Error Codes)
- [ ] 稳定化消息序列化：确保 `Message` 结构对前端友好
- [ ] 使用 OpenFlux 前端实际连接测试
- [ ] 修复格式差异

## 10. Testing Strategy

### 单元测试 (无网络, 无 LLM)

| 模块         | 测试内容                                              |
|-------------|------------------------------------------------------|
| protocol.rs | GatewayMessage 序列化/反序列化 round-trip              |
| protocol.rs | 各 payload type 的 JSON 格式验证                       |
| session.rs  | create/get/list 基本功能                               |
| session.rs  | 并发创建 session 不冲突                                |
| bridge.rs   | 每个 AgentEvent variant → GatewayMessage 的正确映射     |
| bridge.rs   | Error variant 的 error message 保持完整                |

### 集成测试 (需要 WS 但不需要 LLM)

使用 `tokio-tungstenite` client 连接到 server:

1. 连接 → auth → auth.success
2. sessions.create → 返回 session info
3. sessions.list → 包含刚创建的 session
4. agents.list → 返回工具列表
5. 未知 type → error response

### 端到端测试 (需要 LLM API Key)

1. auth → sessions.create → chat → 收到 chat.start → chat.progress(token) → chat.complete
2. 多轮对话: chat → complete → chat → complete, 验证历史上下文保持

## 11. Future Considerations (不在本期实现)

- **Tauri 嵌入**: Phase B，见 `openflux-integration-design.md` Phase 5
- **Graceful shutdown**: 接受 `CancellationToken` 用于优雅关闭（嵌入 Tauri 时需要）
- **Session 持久化**: 当前 in-memory，后续可扩展为 SQLite
- **认证**: 当前 auth 直接返回 success，后续按需实现 token 验证
- **多 Agent 支持**: 当前只有一个 AgentRuntime 实例，后续可支持多个 agent profile
- **WebSocket 重连**: 前端 gateway-client 已有重连逻辑，server 侧无需特殊处理
