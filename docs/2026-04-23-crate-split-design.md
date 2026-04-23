# zero-nova Crate 拆分设计与实现文档

> 目标：将目前的单一 crate `zero-nova` 拆分成一组职责清晰的 crate，核心是把"通用 agent"作为独立库，不同 I/O 形态（WebSocket、stdio）以组合方式产生不同的部署二进制。本次仅为设计，不做代码改动。

---

## 1. 背景与动机

### 1.1 现状
当前仓库只有一个主 crate `zero-nova` + 一个工作区子 crate `crates/channel-websocket`，同时还包含一个 Tauri 桌面端 `deskapp/src-tauri`。

`zero-nova` 内部职责混杂：
- 纯 agent：`agent.rs` / `agent_catalog.rs` / `message.rs` / `event.rs` / `prompt.rs` / `tool.rs` / `tool/builtin/` / `skill.rs` / `provider/` / `mcp/`
- 配置：`config.rs`
- 会话与持久化：`conversation/`（SQLite + 内存 cache + service）
- 应用门面：`app/`（`GatewayApplication` trait 与实现、bootstrap、`AppEvent`）
- WebSocket 网关：`gateway/`（protocol、handlers、router、server、bridge）
- 两个 bin：`nova_cli`（REPL）与 `nova_gateway`（WS 侧车）
- 通过 feature（`gateway` / `cli` / `prod`）决定编译哪些模块

主要问题：
1. 应用门面与 WebSocket 绑死——`GatewayApplication` trait 里就有 `#[cfg(feature="gateway")]` 的 `connect/handle/disconnect`，直接引用 `channel_websocket::ResponseSink` 与 `gateway::protocol::GatewayMessage`。
2. 同一个 crate 既当库又当多个 bin，feature 组合越堆越多。
3. 想加 stdio 形态的 server 时，没法和 WebSocket 形态清晰并存。
4. `nova-core` 这类纯 agent 能力无法被外部（例如 deskapp、第三方）独立依赖，必须带上 sqlite/ws 等一堆依赖。

### 1.2 目标
- 把"通用 agent"提成独立 crate，纯库、依赖最小。
- 以"通用 agent"为基础，通过组合出两个独立的可部署 crate：
  - WebSocket + agent → WS gateway 二进制
  - stdin/stdout + agent → stdio gateway 二进制
- 消灭 `#[cfg(feature="gateway")]` 横穿核心模块的代码。
- 保留现有 CLI REPL 体验，放到自己的 crate 里。

### 1.3 非目标
- 不重构 agent 运行时的内部语义（turn 循环、事件语义、tool schema 不变）。
- 不修改 gateway JSON 协议（envelope 字段保持兼容，deskapp 前端不用动）。
- 不动 MCP 和 provider 的细节实现。
- 这次不处理 deskapp 侧 Rust 代码（`deskapp/src-tauri`）与前端对齐，只确保它仍能依赖新 crate。

---

## 2. 新 Workspace 布局

```
zero-nova/
├── Cargo.toml                       # 纯 workspace，[package] 移除
├── crates/
│   ├── nova-core/                   # 通用 agent（pure library）
│   ├── nova-conversation/           # 会话与持久化（可选依赖）
│   ├── nova-app/                    # 应用门面 + bootstrap，transport 无关
│   ├── nova-protocol/               # JSON 协议 DTO（前后端共享）
│   ├── channel-core/                # ChannelHandler trait + ResponseSink（抽象）
│   ├── channel-websocket/           # WS 实现（已存在，瘦身）
│   ├── channel-stdio/               # NDJSON over stdin/stdout（新增）
│   ├── nova-gateway-core/           # GatewayMessage ↔ AgentApplication 路由/桥接
│   ├── nova-server-ws/              # WS 适配器 + bin: nova_gateway_ws
│   ├── nova-server-stdio/           # stdio 适配器 + bin: nova_gateway_stdio
│   └── nova-cli/                    # REPL bin: nova_cli
└── deskapp/src-tauri/               # 不变，改为依赖新 crate
```

### 2.1 依赖关系（箭头 = "依赖于"）
```
nova-cli ──────────────┐
                       ▼
nova-server-ws ──▶ nova-gateway-core ──▶ nova-app ──▶ nova-conversation ──▶ nova-core
     │                  │                   ▲                                ▲
     │                  └──▶ nova-protocol  │                                │
     └──▶ channel-websocket ──▶ channel-core                                 │
                                                                             │
nova-server-stdio ──▶ nova-gateway-core ──▶ nova-app                         │
     │                                                                       │
     └──▶ channel-stdio ──▶ channel-core                                     │
                                                                             │
deskapp/src-tauri ──▶ nova-app / nova-core ──────────────────────────────────┘
```

关键不变式：`nova-core` 不依赖 `nova-conversation`、`nova-app`、任何 channel/transport、`nova-protocol`。

---

## 3. 每个 crate 的职责与实现细节

### 3.1 `nova-core`（通用 agent，纯库）

**职责**：LLM 对话循环、工具体系、MCP、system prompt、agent 配置清单。不知道会话存在哪、协议长什么样、怎么对外暴露。

**从现有 `src/` 搬入**：
- `agent.rs` → `src/agent.rs`
- `agent_catalog.rs` → `src/agent_catalog.rs`
- `message.rs` → `src/message.rs`
- `event.rs` → `src/event.rs`
- `prompt.rs` → `src/prompt.rs`
- `tool.rs` + `tool/builtin/` → `src/tool/`
- `skill.rs` → `src/skill.rs`
- `provider/` → `src/provider/`
- `mcp/` → `src/mcp/`
- `config.rs` 中 LLM / model / agent 相关部分 → `src/config.rs`（gateway 特有字段剥离）

**对外 API（crate root `lib.rs`）**：
```rust
pub mod agent;          // AgentRuntime, AgentConfig, TurnResult
pub mod agent_catalog;  // AgentDescriptor, AgentRegistry
pub mod event;          // AgentEvent
pub mod message;        // Message, ContentBlock, Role
pub mod prompt;         // SystemPromptBuilder
pub mod tool;           // Tool, ToolRegistry, ToolContext, builtin::*
pub mod skill;          // SkillRegistry
pub mod provider;       // LlmClient, ModelConfig, providers
pub mod mcp;            // McpClient
pub mod config;         // ModelConfig, AgentSpec 等核心配置结构
```

**依赖**：tokio、serde、serde_json、anyhow、async-trait、log、futures-util、reqwest、scraper、sysinfo、which、tokio-util、custom-utils。

**不要依赖**：sqlx、tokio-tungstenite、channel-*、clap、rustyline。

**Feature**：`default = []`。provider 细分可以暴露 `feature = "anthropic"` / `"openai-compat"`，但目前都强制打开即可，不给自己找事。

---

### 3.2 `nova-conversation`（会话与持久化）

**职责**：Session 聚合根、内存缓存、SQLite 仓库、取消 token 管理。

**搬入**：`src/conversation/*` 整体移入 `src/lib.rs` 为入口。

**对外 API**：
```rust
pub use cache::SessionCache;
pub use repository::{SqliteSessionRepository, SessionRepository};
pub use service::SessionService;
pub use session::Session;
pub use control::SessionControl;
pub use sqlite_manager::SqliteManager;
```

**设计要点**：
- 现有 `SessionService::append_message` 等操作保持签名。
- `Session` 里存的 `ContentBlock`、`Role` 来自 `nova_core::message`。
- SQLite schema 不变（避免 `deskapp` 等既有用户数据迁移）。
- 可选：抽象 `SessionRepository` trait，便于未来替换成 in-memory 或其他存储（**非目标**，列为后续扩展点）。

**依赖**：`nova-core`, sqlx, tokio, anyhow, serde, serde_json, uuid, chrono。

---

### 3.3 `nova-protocol`（JSON 协议 DTO）

**职责**：gateway 对外暴露的 JSON envelope 定义，前后端共享。

**搬入**：`src/gateway/protocol/*` 整体（`envelope.rs`, `agent.rs`, `chat.rs`, `config.rs`, `session.rs`, `system.rs`, `mod.rs`）。

**对外 API**：`GatewayMessage`, `MessageEnvelope`, `ProgressEvent`, `Session`, `Agent`, `MessageDTO`, `ContentBlockDTO`, `ErrorPayload`, `WelcomePayload` 等全部 `pub`。

**依赖**：serde, serde_json, uuid（仅结构），不依赖 `nova-core`。保持轻量让 deskapp/第三方前端也能直接用它做序列化类型。

**设计要点**：
- Transport-agnostic：既可以通过 WS 传，也可以通过 NDJSON 走 stdio，字段完全一样。
- `GatewayMessage` 保持 `{id, envelope}` 结构不变。

---

### 3.4 `nova-app`（应用门面 + bootstrap）

**职责**：把 `nova-core` + `nova-conversation` 组装成一个"可以被任何 transport 驱动的应用"。**不关心 WebSocket 还是 stdio**。

**搬入**：`src/app/*`（`application.rs`, `bootstrap.rs`, `conversation_service.rs`, `types.rs`）。

**重大改动**（关键）：
- 把 `GatewayApplication` 更名为 `AgentApplication`（避免 "gateway" 的 transport 暗示），文件 `application.rs` → `src/app.rs`。
- **删掉** trait 中的 `#[cfg(feature = "gateway")]` 块：
  ```rust
  // 删除：connect / handle / disconnect
  ```
  这三个方法是 transport 粘合层的事，不属于应用门面。
- trait 只保留业务语义：
  ```rust
  #[async_trait]
  pub trait AgentApplication: Send + Sync {
      async fn session_exists(&self, session_id: &str) -> Result<bool>;
      async fn start_turn(&self, session_id: &str, input: &str,
                          sender: mpsc::Sender<AppEvent>) -> Result<()>;
      async fn stop_turn(&self, session_id: &str) -> Result<()>;

      async fn list_sessions(&self) -> Result<Vec<AppSession>>;
      async fn session_messages(&self, session_id: &str) -> Result<Vec<AppMessage>>;
      async fn create_session(&self, title: Option<String>, agent_id: String) -> Result<AppSession>;
      async fn delete_session(&self, session_id: &str) -> Result<bool>;
      async fn copy_session(&self, session_id: &str, truncate_index: Option<usize>)
          -> Result<AppSession>;

      async fn switch_agent(&self, session_id: &str, agent_id: &str) -> Result<AppAgent>;
      fn list_agents(&self) -> Vec<AppAgent>;
      fn get_agent(&self, agent_id: &str) -> Option<AppAgent>;

      fn config_snapshot(&self) -> Result<Value>;
      async fn update_config(&self, payload: Value) -> Result<()>;

      // 新增：供 transport 回调的 welcome，返回 AppEvent（不是 GatewayMessage）
      async fn on_connect(&self) -> Result<Vec<AppEvent>>;
      async fn on_disconnect(&self, conn_id: &str);
  }
  ```
- `AppEvent` 中增加 `Welcome { require_auth, setup_required }` 之类的枚举变体，把当前 `connect()` 里产生的 `WelcomePayload` 改走 `AppEvent`。
- `bootstrap.rs`：`bootstrap()` 不再直接调 `gateway::run_server`，而是返回一个 `Arc<dyn AgentApplication>` 和相关 handle，由各 transport crate 自行驱动。

**对外 API**：
```rust
pub use app::{AgentApplication, AgentApplicationImpl};
pub use types::{AppAgent, AppEvent, AppMessage, AppSession};
pub use conversation_service::ConversationService;
pub use bootstrap::{build_application, BootstrapOptions};
```

**依赖**：`nova-core`, `nova-conversation`, tokio, anyhow, serde_json, async-trait, toml, log。

**不依赖** channel-* 与 `nova-protocol`。

---

### 3.5 `channel-core`（可选，但推荐）

**职责**：抽象 transport 无关的 handler 接口，让 WS 与 stdio 共享一套约定。

**内容**：
```rust
#[async_trait]
pub trait ChannelHandler: Send + Sync + 'static {
    type Req: DeserializeOwned + Send + 'static;
    type Resp: Serialize + Send + 'static;

    async fn on_connect(&self, peer: PeerId) -> Result<Vec<Self::Resp>>;
    async fn on_message(&self, peer: PeerId, req: Self::Req,
                        sink: ResponseSink<Self::Resp>) -> Result<()>;
    async fn on_disconnect(&self, peer: PeerId);
}

pub struct ResponseSink<R> { /* 把现有实现搬过来 */ }
pub enum ResponseSinkError { Closed, Full }

pub type PeerId = String; // WS: "ip:port"，stdio: "stdio"
```

**说明**：目前这个 trait 住在 `channel-websocket` 里。抽出去后 `channel-websocket` 与 `channel-stdio` 都实现它。

**取舍**：如果觉得额外一个 crate 太碎，也可以**不抽**，每个 transport 自己定义一份很短的 handler trait；但这样 server 适配层就要写两遍。推荐抽出。

**依赖**：tokio, async-trait, serde, anyhow。

---

### 3.6 `channel-websocket`（已存在，瘦身）

**改动**：
- 把现有的 `ChannelHandler` / `ResponseSink` 定义删掉，改为从 `channel-core` re-export。
- 保留 `run_server(addr, handler)` 实现。
- 其余不动。

---

### 3.7 `channel-stdio`（新增）

**职责**：NDJSON over stdin/stdout，每行一个 JSON 消息。

**设计**：
```rust
pub async fn run_stdio<H>(handler: Arc<H>) -> Result<()>
where H: ChannelHandler;
```

**实现要点**：
- 用 `tokio::io::stdin()` + `BufReader::new(...).lines()` 读取；每行 `serde_json::from_str::<H::Req>`。
- 写端：`tokio::io::stdout()`，一个后台 task 消费 `mpsc`，序列化后 `write_all` + 追加 `\n` + `flush`。
- `PeerId` 固定为 `"stdio"`。
- **日志必须重定向到 stderr**（否则会污染协议流）。在 server-stdio bin 里统一初始化 logger 输出到 stderr。
- 支持 EOF：stdin 关闭时调 `on_disconnect` 并退出。
- 支持父进程监控：和现有 `nova_gateway` 一样的 `parent_pid` / stdin-EOF 逻辑，但由于本身读 stdin，EOF 已自动触发退出。

**消息成帧选择**：NDJSON（`\n` 分隔）。优点简单、通用、和 JSON-RPC over stdio 及 LSP 风格兼容；不用 Content-Length 头保持实现简洁。如果未来要兼容 LSP 习惯，再加一个 feature 切换。

**依赖**：`channel-core`, tokio (features = io-util, io-std, sync, rt), async-trait, serde, serde_json, anyhow, log。

---

### 3.8 `nova-gateway-core`（协议 ↔ 应用 路由层）

**职责**：把 `GatewayMessage` 分发到 `AgentApplication` 的具体方法上，并把 `AppEvent` 转回 `GatewayMessage`。WS 和 stdio 两侧共享。

**搬入**：`src/gateway/{bridge,router,handlers}` 整体迁入此 crate。

**文件结构**：
```
crates/nova-gateway-core/
└── src/
    ├── lib.rs
    ├── bridge.rs        # AppEvent ↔ GatewayMessage
    ├── router.rs        # GatewayMessage → AgentApplication 调用分发
    └── handlers/        # agents / chat / config / sessions / system
```

**对外 API**：
```rust
pub use bridge::{app_event_to_gateway, app_session_to_protocol, app_agent_to_protocol, app_message_to_protocol};
pub use router::dispatch;
```

**依赖**：`nova-app`, `nova-protocol`, `channel-core`（取 `ResponseSink`）, async-trait, tokio, serde_json, anyhow。

---

### 3.9 `nova-server-ws`（WebSocket 适配器 + 二进制）

**职责**：把 `AgentApplication` 包成 `ChannelHandler<Req=GatewayMessage, Resp=GatewayMessage>`，启动 WS server。

**文件结构**：
```
crates/nova-server-ws/
├── Cargo.toml
└── src/
    ├── lib.rs           # 可复用的 WsGatewayHandler + run()
    └── bin/
        └── nova_gateway_ws.rs   # = 原 nova_gateway.rs
```

**核心胶水**：
```rust
pub struct WsGatewayHandler { app: Arc<dyn AgentApplication> }

#[async_trait]
impl ChannelHandler for WsGatewayHandler {
    type Req = GatewayMessage;
    type Resp = GatewayMessage;
    async fn on_connect(&self, _peer) -> Result<Vec<GatewayMessage>> {
        // app.on_connect().await 返回 Vec<AppEvent>，转成 GatewayMessage
    }
    async fn on_message(&self, _peer, req, sink) -> Result<()> {
        nova_gateway_core::dispatch(req, &*self.app, sink).await
    }
    async fn on_disconnect(&self, peer) { self.app.on_disconnect(&peer).await }
}
```

**Bin `nova_gateway_ws`**：
- 解析 `Args`（保持现有 clap 结构）。
- 加载 config，构造 `OpenAiCompatClient`。
- `nova_app::bootstrap::build_application(...)` 得到 `Arc<dyn AgentApplication>`。
- `nova_server_ws::run(addr, app).await`。
- 保留 `tokio::select!` 对 parent_pid 与 stdin EOF 的监控。

**依赖**：`nova-app`, `nova-protocol`, `nova-gateway-core`, `channel-websocket`, `channel-core`, clap, sysinfo, tokio, custom-utils。

---

### 3.10 `nova-server-stdio`（stdio 适配器 + 二进制）

**职责**：与 `nova-server-ws` 对称，用同一套 `nova-protocol` 和 `AgentApplication`，只是底层走 stdio。

**文件结构**：
```
crates/nova-server-stdio/
├── Cargo.toml
└── src/
    ├── lib.rs           # StdioGatewayHandler
    └── bin/
        └── nova_gateway_stdio.rs
```

**关键设计**：`StdioGatewayHandler` 的 `Req/Resp` 仍然是 `GatewayMessage`，协议层与 WS 完全一致。路由分发器（`nova_gateway_core::dispatch`）两侧复用。

**Bin `nova_gateway_stdio`**：
- 初始化 logger → **stderr**。
- 加载 config（沿用同一 `config.toml`）。
- 构建 `Arc<dyn AgentApplication>`。
- `channel_stdio::run_stdio(Arc::new(StdioGatewayHandler::new(app))).await`。
- 不需要监听端口；退出条件是 stdin EOF。
- 不需要 `parent_pid`（进程由父进程通过 stdin 关闭来控制），但为一致性仍可保留。

**依赖**：`nova-app`, `nova-protocol`, `nova-gateway-core`, `channel-stdio`, `channel-core`, clap, tokio, custom-utils。

---

### 3.11 `nova-cli`（REPL 二进制）

**职责**：现有 `src/bin/nova_cli.rs` 的 REPL 体验，直接使用 `nova-core`，**不走 gateway 协议**。

**搬入**：`src/bin/nova_cli.rs` → `crates/nova-cli/src/main.rs`。

**依赖**：`nova-core`, clap, rustyline, chrono, colored, tokio, custom-utils, anyhow, serde_json。

不依赖 `nova-app` / `nova-protocol` / channel 任何一个——保持它作为 "agent 裸用法" 的示例与调试工具。

---

## 4. 配置（config.toml）处理

现有 `config.rs` 里 `AppConfig` 含 `llm` / `gateway` / `agents`。拆分后：

- `nova-core::config`：
  - `ModelConfig`（已在 provider）
  - `LlmConfig`（base_url, api_key, model_config）
  - `AgentSpec`（id, display_name, description, aliases, system_prompt_template, tool_whitelist, model_config）
  - 合并在 `CoreConfig` 里。
- `nova-app::config` 额外定义 `GatewayConfig { host, port, max_iterations, tool_timeout_secs, agents: Vec<AgentSpec> }` 并聚合为 `AppConfig { llm, gateway }`。
- 每个 server bin 自己加载 `config.toml` → `AppConfig::from_origin(...)`，下发给 `build_application`。

兼容性：`config.toml` 文件结构**保持不变**，只是解析发生位置变了。

---

## 5. Cargo.toml 要点

### 5.1 根 `Cargo.toml`（workspace-only）
```toml
[workspace]
resolver = "2"
members = [
  "crates/nova-core",
  "crates/nova-conversation",
  "crates/nova-app",
  "crates/nova-protocol",
  "crates/channel-core",
  "crates/channel-websocket",
  "crates/channel-stdio",
  "crates/nova-gateway-core",
  "crates/nova-server-ws",
  "crates/nova-server-stdio",
  "crates/nova-cli",
  "deskapp/src-tauri",
]

[workspace.dependencies]
# 保持现有 workspace.dependencies，新增内部 crate：
nova-core         = { path = "crates/nova-core" }
nova-conversation = { path = "crates/nova-conversation" }
nova-app          = { path = "crates/nova-app" }
nova-protocol     = { path = "crates/nova-protocol" }
channel-core      = { path = "crates/channel-core" }
channel-websocket = { path = "crates/channel-websocket" }
channel-stdio     = { path = "crates/channel-stdio" }
nova-gateway-core = { path = "crates/nova-gateway-core" }
```

根 `[package]` 与 `[[bin]]` 全部删除。

### 5.2 Feature 简化
- 原 `default = ["cli", "gateway"]` 取消；每个 bin 是独立 crate，想要哪个就 `cargo build -p nova-server-ws`。
- `prod` feature 留在各 bin 的 crate 里（`custom-utils/prod`）。

### 5.3 `[[bin]]`
- `nova-cli` / `nova-server-ws` / `nova-server-stdio` 三个 bin crate，每个只产一个二进制，入口在 `src/main.rs` 或 `src/bin/<name>.rs`。

---

## 6. 协议与行为对比

| 维度 | WS 版 | stdio 版 |
|------|-------|----------|
| 成帧 | WebSocket text frame | NDJSON，`\n` 分隔 |
| 消息类型 | `GatewayMessage` | `GatewayMessage`（同一份 DTO） |
| 连接数 | 多（服务端监听端口） | 1（进程生命周期内一个连接） |
| 心跳 | Ping/Pong by WS | 无（依赖父进程 EOF） |
| 日志 | stdout/stderr 皆可 | **必须 stderr** |
| 鉴权 | 预留 `require_auth` | 默认不启用；如需可走 env/config |
| 退出 | ctrl-C / parent_pid | stdin EOF / ctrl-C |

核心：**协议层完全一致**，这意味着 deskapp 前端后续若从 tauri-sidecar 切到 stdio-sidecar，业务层零改动。

---

## 7. 迁移步骤（建议阶段化执行）

按从下往上拆，每一步结束都保证 `cargo build --workspace` 和现有 `nova_gateway`/`nova_cli` 能跑。

### Phase 1：抽 `nova-core`
1. 新建 `crates/nova-core`，把 agent/tool/provider/mcp/skill/prompt/message/event/agent_catalog 搬过去，`config.rs` 拆出 LLM/model/AgentSpec 子集。
2. `zero-nova` 在 `lib.rs` 里 `pub use nova_core::*` 做临时 re-export，保证下游（gateway/cli/deskapp）不用改 use path。
3. 跑编译 & 现有 bin 手测。

### Phase 2：抽 `nova-conversation`
搬 `src/conversation/*`，依赖 `nova-core`。同样通过 re-export 维持兼容。

### Phase 3：抽 `nova-protocol`
搬 `src/gateway/protocol/*`。因为 `AppEvent → GatewayMessage` 转换还在 `src/gateway/bridge.rs`，这步只挪 DTO。

### Phase 4：抽 `nova-app` + 清理 trait
1. 搬 `src/app/*` 到 `nova-app`。
2. **删除 `AgentApplication` trait 里的 `connect/handle/disconnect`**，把它们迁入 server-ws。
3. `AppEvent` 增加 `Welcome` 变体，替换 `connect()` 返回 `Vec<GatewayMessage>` 为 `Vec<AppEvent>`。
4. 新增 `AgentApplication::on_connect/on_disconnect`（返回 AppEvent / 无返回值）。

### Phase 5：拆 channel 层
1. 新建 `channel-core`，把 `ChannelHandler` / `ResponseSink` / `ResponseSinkError` 从 `channel-websocket` 搬过去。
2. `channel-websocket` 改为依赖 `channel-core`，re-export 这些符号。
3. 新建 `channel-stdio`，实现 `run_stdio`。

### Phase 6：`nova-gateway-core` + 两个 server
1. 新建 `nova-gateway-core`，搬 `src/gateway/{bridge,router,handlers}`。
2. 新建 `nova-server-ws`，搬 `src/gateway/server.rs` + `src/bin/nova_gateway.rs`。
3. 新建 `nova-server-stdio`，写 handler + `nova_gateway_stdio` bin。

### Phase 7：`nova-cli`
搬 `src/bin/nova_cli.rs`。

### Phase 8：收尾
1. 根 `Cargo.toml` 删 `[package]`，只剩 workspace。
2. 删 `src/` 根目录。
3. 更新 `Makefile.toml`、`scripts/`、`AGENTS.md`、`docs/` 的 crate 路径引用。
4. `deskapp/src-tauri/Cargo.toml` 改为依赖 `nova-app` / `nova-core`（看它实际用了哪些）。
5. `cargo fmt && cargo clippy --workspace --all-targets && cargo test --workspace`。
6. 跑一遍 CI。

---

## 8. 风险与权衡

| 风险 | 说明 | 缓解 |
|------|------|------|
| 构建时间变长 | workspace 成员增多 | 大多数共享依赖通过 `[workspace.dependencies]` 复用，实际增量可控；受益：增量改动只重编涉及的 crate |
| 状态 | 已完成 (Completed) | - |
| 目标 | 将项目拆分为多个 transport-agnostic 的 crate，实现核心逻辑与传输层的彻底解耦。 | - |
| 成果 | 项目已重构为 11+ 个 workspace 成员，支持 WebSocket 和 Stdio 两种 Gateway 模式。 | - |
| 循环依赖风险 | 尤其是 `nova-app` ↔ `nova-gateway-core` | 严格依赖方向（见 §2.1），`nova-gateway-core` 单向依赖 `nova-app`；`nova-app` 不引用任何 protocol / channel |
| `channel-core` 过度抽象 | 只服务两个实现 | 一开始只暴露必要符号，先做 minimal；真的嫌碎可以让 `channel-stdio` 直接依赖 `channel-websocket`（不推荐，但能省一个 crate） |
| stdio 日志污染协议流 | `println!` / 默认 logger 可能输出到 stdout | server-stdio 的 bin 里**强制** logger 指向 stderr；代码评审时 grep `println!` |
| NDJSON 对"单条巨大 JSON" 的处理 | 若单条消息非常大，`lines()` 会把它全读进内存 | 默认 `BufReader` 足够；极端情况可加 `read_to_end` + `LengthDelimitedCodec` 替代，后续扩展 |
| deskapp 现有 Rust 侧代码改动 | 现在可能 `use zero_nova::...` | Phase 1 的 `pub use` re-export 作为过渡；最终切到 `nova-app` / `nova-core`，放到 Phase 8 |
| 测试覆盖 | 现有 `src/mcp/tests.rs` 等会跟着迁 | 每个 crate 自带单测；新增一个 workspace 集成测试（启一个 stdio server 用 NDJSON 对话一轮）作为 smoke |

---

## 9. 成功标准（Definition of Done）

1. `cargo build --workspace` 全绿，`cargo clippy --workspace --all-targets -- -D warnings` 全绿。
2. `nova_gateway_ws` 行为、协议、日志与原 `nova_gateway` 等价（deskapp 连接正常）。
3. `nova_gateway_stdio` 能够通过 NDJSON 完成一次完整对话：connect → list_agents → create_session → chat → receive progress → turn_complete。
4. `nova_cli` 的 REPL、`run`、`tools`、`mcp-test` 子命令全部可用。
5. `nova-core` 单独依赖构建成功，且不引入 sqlx/tungstenite/clap。
6. `config.toml` 零迁移即可跑新二进制。

---

## 10. 未来扩展点（本次不做）

- `SessionRepository` trait 化，支持内存/PostgreSQL 后端。
- `channel-grpc` / `channel-unix-socket`。
- `nova-protocol` 的 schema 导出（TS 类型 / JSON Schema），给 deskapp 前端自动生成 binding。
- 把 `nova-gateway-core` 再拆出一层 "应用调用 trait"，使 protocol 与 `AgentApplication` 之间通过 codec 解耦。

---

## 11. 讨论中的开放问题

两个可能的分歧点，等落地前最终拍板：

1. **`channel-core` 是否值得独立一个 crate**：独立可以让 `channel-stdio` 和 `channel-websocket` 彻底平级；否则 `channel-stdio` 依赖 `channel-websocket` 只是为了复用 trait，语义上别扭。
2. **`nova-gateway-core` 要不要并入 `nova-app`**：并入会让 `nova-app` 沾上 `nova-protocol` 依赖，破坏"应用门面 transport 无关"的不变式。当前方案选择独立 crate 保持清洁。
