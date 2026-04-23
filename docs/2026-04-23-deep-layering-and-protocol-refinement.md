# Phase 5: 深度分层、协议模块化与服务精细化设计文档

## 时间
- 创建时间：2026-04-23
- 最后更新：2026-04-23

## 项目现状

经过 Phase 4 的网关收口与简化，项目已达到“单 Agent 会话”的最小闭环，结构趋于稳定。但从长期演进看，当前仍存在以下结构性问题：

1. **协议单体化**：`src/gateway/protocol.rs` 过大，所有 DTO 聚合在单文件中，阅读、维护与增量修改成本持续上升。
2. **网关与应用直连具体实现**：`GatewayApplication<C>` 目前是具体类型，网关处理器直接依赖其实现细节，不利于 Mock、替换实现或未来扩展新入口。
3. **传输 DTO 反向渗透应用层**：`app/application.rs` 直接返回 `gateway::protocol::*` 下的类型，导致应用层向上依赖渠道层。
4. **事件边界不清晰**：当前 `AgentEvent` 同时被 CLI 与 gateway 消费，但它既承载运行时事件，又被直接映射为某个具体协议，不利于后续多渠道复用。
5. **会话状态职责混叠**：`SessionStore` 同时承担内存缓存、持久化、懒加载、并发控制和取消控制，演进空间有限。

## 本次目标

通过深度分层和契约化设计，实现“渠道适配”和“业务执行”真正分离：

1. **协议模块化**：在物理上拆分网关协议定义，提高结构清晰度与可维护性。
2. **应用接口契约化**：让 gateway 仅依赖应用层 trait，不再依赖具体实现类型。
3. **统一内部事件边界**：明确 `AgentEvent` 与 `AppEvent` 的角色，避免双事件体系无序并存。
4. **存储职责结构化**：将 `SessionStore` 演进为会话服务、缓存、仓储三层，但保持现有行为语义不变。
5. **保证兼容迁移**：本轮属于分层重构，不改变现有网关协议 JSON 结构，也不改变 CLI 的用户可见行为。

## 详细设计

### 1. 协议物理拆分

将 `src/gateway/protocol.rs` 拆分为目录结构，通过 `mod.rs` 聚合导出：

```text
src/gateway/protocol/
├── mod.rs
├── envelope.rs
├── chat.rs
├── session.rs
├── agent.rs
├── config.rs
└── system.rs
```

建议按以下边界拆分：

| 文件 | 内容 |
|------|------|
| `envelope.rs` | `GatewayMessage`、`MessageEnvelope` 等统一消息骨架 |
| `chat.rs` | `ChatPayload`、`ProgressEvent`、`ChatCompletePayload` 等 |
| `session.rs` | `Session`、`MessageDTO`、`SessionCreateRequest` 等 |
| `agent.rs` | `Agent`、`AgentsListResponse`、`AgentsSwitchResponse` |
| `config.rs` | `config.get` / `config.update` 相关 payload |
| `system.rs` | `ErrorPayload`、`WelcomePayload`、`SuccessResponse` |

关键约束：

1. 各子模块只定义自身 DTO，不互相 `use` 对方的具体实现。
2. 交叉引用类型统一由 `mod.rs` 重导出后使用，避免环依赖。
3. 本轮只做物理拆分，不改字段名、不改 `serde` tag、不改 JSON 结构。

### 2. 应用层契约与事件边界

本轮需要先明确：**应用层暴露的是业务契约，不是传输契约；运行时核心事件源仍然是 `AgentEvent`，`AppEvent` 是应用层对外暴露的稳定事件模型。**

#### 2.1 `AgentEvent` 与 `AppEvent` 的关系

为避免双事件模型失控，采用以下分层规则：

1. `AgentEvent` 继续作为 agent runtime 的内部/底层事件，定义在 `src/event.rs`。
2. `AppEvent` 定义在 `src/app/types.rs`，作为应用层对 gateway 等上游入口暴露的稳定事件契约。
3. `ConversationService` 内部可以继续消费和产生 `AgentEvent`，但在穿过应用层边界前，统一映射为 `AppEvent`。
4. CLI 暂不在本轮切换到 `AppEvent`。CLI 继续直接消费 `AgentEvent`，避免把“分层重构”混入“终端输出重构”。
5. gateway 只消费 `AppEvent`，再由 gateway adapter 映射为 `GatewayMessage`。

这样处理后：

- agent runtime 不感知 gateway 协议；
- gateway 不感知 agent runtime 的底层事件细节；
- CLI 与 gateway 可以在不同节奏上逐步收敛，而不是一次性强推统一出口。

#### 2.2 应用层纯对象

应用层对外返回的对象只表达业务状态，不带 `gateway::protocol` 依赖。

```rust
// src/app/types.rs
use crate::message::ContentBlock;
use crate::provider::types::Usage;
use serde_json::Value;

pub struct AppSession {
    pub id: String,
    pub title: Option<String>,
    pub agent_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: usize,
}

pub struct AppAgent {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

pub struct AppMessage {
    pub role: String,
    pub content: Vec<ContentBlock>,
    pub timestamp: i64,
}

pub enum AppEvent {
    TextDelta(String),
    ThinkingDelta(String),
    ToolStart { id: String, name: String, input: Value },
    ToolEnd { id: String, name: String, output: String, is_error: bool },
    ToolLog { id: String, name: String, log: String, stream: String },
    Iteration { current: usize, total: usize },
    IterationLimitReached { iterations: usize },
    AssistantMessage { content: Vec<ContentBlock> },
    TurnComplete { usage: Usage },
    Error(String),
    SystemLog(String),
    AgentSwitched { agent: AppAgent },
}
```

说明：

1. `AppEvent` 不是“完全抽象到脱离当前运行时”的理想化事件，而是“对应用层稳定且不依赖 gateway 协议”的事件。
2. `Welcome`、`connect`、`disconnect` 这类连接生命周期事件不进入 `AppEvent`，它们属于渠道层职责。
3. `AppMessage.role` 本轮保持 `String`，避免重构范围扩大到消息角色枚举统一。

#### 2.3 `GatewayApplication` Trait 的完整契约面

应用层契约必须覆盖当前 gateway 的完整调用面，否则网关仍会回退到依赖具体实现。建议定义为：

```rust
#[async_trait]
pub trait GatewayApplication: Send + Sync {
    async fn session_exists(&self, session_id: &str) -> anyhow::Result<bool>;
    async fn start_turn(
        &self,
        session_id: &str,
        input: &str,
        sender: tokio::sync::mpsc::Sender<AppEvent>,
    ) -> anyhow::Result<()>;
    async fn stop_turn(&self, session_id: &str) -> anyhow::Result<()>;

    async fn list_sessions(&self) -> anyhow::Result<Vec<AppSession>>;
    async fn session_messages(&self, session_id: &str) -> anyhow::Result<Vec<AppMessage>>;
    async fn create_session(&self, title: Option<String>, agent_id: String) -> anyhow::Result<AppSession>;
    async fn delete_session(&self, session_id: &str) -> anyhow::Result<bool>;
    async fn copy_session(&self, session_id: &str, truncate_index: Option<usize>) -> anyhow::Result<AppSession>;

    async fn switch_agent(&self, session_id: &str, agent_id: &str) -> anyhow::Result<AppAgent>;
    fn list_agents(&self) -> Vec<AppAgent>;
    fn get_agent(&self, agent_id: &str) -> Option<AppAgent>;

    fn config_snapshot(&self) -> anyhow::Result<serde_json::Value>;
    async fn update_config(&self, payload: serde_json::Value) -> anyhow::Result<()>;
}
```

边界说明：

1. 移除 `connect()` / `disconnect()`：它们不是应用业务能力，而是 gateway server 生命周期能力。
2. gateway handler 只持有 `Arc<dyn GatewayApplication>`。
3. `GatewayApplication<C>` 具体实现仍可保留泛型，但通过 bootstrap 阶段完成具体类型封装后，以 trait object 形式交给 gateway。

### 3. 渠道层适配原则

gateway 层承担“协议翻译官”职责，但只负责翻译，不编排业务。

#### 3.1 入站

gateway handler 负责：

1. 解析 `GatewayMessage`。
2. 做基础字段校验。
3. 将协议 payload 转成简单参数传给 `GatewayApplication`。
4. 将应用层错误统一映射为网关协议错误码。

#### 3.2 出站

gateway adapter 负责：

1. `AppSession` -> `gateway::protocol::Session`
2. `AppAgent` -> `gateway::protocol::Agent`
3. `AppMessage` -> `gateway::protocol::MessageDTO`
4. `AppEvent` -> `gateway::protocol::GatewayMessage`

这里必须坚持一条规则：**gateway adapter 可以依赖 app types 和 gateway protocol；app 不得依赖 gateway protocol。**

#### 3.3 生命周期事件归属

以下事件继续由 gateway 直接生成，不进入应用层：

1. `welcome`
2. 连接建立/断开日志
3. request-id 相关的协议信封包装
4. websocket 发送失败、连接关闭等传输错误

这样做的原因是这些事件对 CLI、批处理、未来 HTTP 接口都不具备统一业务意义。

### 4. 会话服务深度分层

将当前 `SessionStore` 拆解为三部分，但保持现有外部行为：

| 组件 | 职责 | 存放位置 |
|------|------|----------|
| `SessionRepository` | 纯数据库读写、SQL 映射，不持有运行时状态 | `conversation/repository.rs` |
| `SessionCache` | 管理 `Arc<SessionRuntime>`，承载内存态、并发锁、取消 token | `conversation/cache.rs` |
| `SessionService` | 组合 repository 和 cache，对外提供创建、查询、复制、切换 agent、消息追加等业务动作 | `app/session_service.rs` 或 `conversation/service.rs` |

其中：

- `SessionRuntime` 表示会话运行时状态，等价于当前 `Arc<Session>` 的职责集合。
- `SessionRepository` 返回持久化记录结构，例如 `StoredSession`、`StoredMessage`，不直接暴露运行时锁对象。
- `SessionCache` 不直接做数据库写入。

#### 4.1 一致性策略

本轮必须显式保留当前行为语义，采用如下策略：

1. **读取策略**：优先查 `SessionCache`；cache miss 时由 `SessionService` 从 `SessionRepository` 加载并回填 cache。
2. **并发去重**：同一个 `session_id` 的 cache miss 加载需要串行化，避免并发请求把同一会话重复装入 cache。
3. **写入策略**：以 repository 为持久化真源，业务操作先更新内存运行态，再持久化；若持久化失败，本次操作返回错误，不将半成功状态静默吞掉。
4. **创建策略**：新建会话时，先构造完整运行态，再写入 repository，最后写入 cache，确保 cache 中不会出现未持久化成功的脏会话。
5. **删除策略**：先删 repository，再删 cache；如果 repository 删除失败，不移除 cache。
6. **复制策略**：复制会话时，先在内存中组装新会话快照，再完整写库，成功后再放入 cache。
7. **切换 agent 策略**：更新运行态中的 active agent 后，必须立刻持久化 `sessions` 表中的 `agent_id`。
8. **取消控制归属**：`chat_lock` 与 `cancellation_token` 只存在于 `SessionCache` / `SessionRuntime`，不进入 repository。

#### 4.2 `chat_lock` 与取消语义

当前 `SessionStore` 不只是缓存，还承担并发保护。本轮需要把这部分语义明确保留下来：

1. `start_turn` 必须按 `session_id` 串行执行。
2. `stop_turn` 只负责触发取消 token，不负责等待后台任务完全退出。
3. turn 结束后必须清理 cancellation token，并刷新 `updated_at`。
4. 即使未来增加多入口，同一会话仍共享同一把 `chat_lock`，否则会破坏历史消息顺序。

### 5. 数据流向

重构后的典型消息流如下：

1. gateway 收到 `chat` 请求。
2. handler 解析 payload，提取 `session_id` 与 `input`。
3. handler 调用 `GatewayApplication::start_turn(...)`。
4. `GatewayApplication` 内部调用 `ConversationService`。
5. `ConversationService` 驱动 agent runtime，接收 `AgentEvent`。
6. 应用层把 `AgentEvent` 映射为 `AppEvent`，通过 channel 向上游发送。
7. gateway adapter 把 `AppEvent` 映射为 `GatewayMessage` 并发送给 websocket 客户端。
8. turn 完成后，gateway 发送 `chat.complete` 等协议消息。

这个流向的关键收益是：未来新增 HTTP、Telegram 等入口时，只需替换第 1、2、7、8 步的适配器，而第 3 到 6 步保持不变。

## 实施顺序

### Step 1：协议物理拆分
- 创建 `src/gateway/protocol/` 目录与 `mod.rs`。
- 原样搬迁 DTO 和测试，先保证编译与 JSON 兼容。
- 不在这一步引入字段变更或语义调整。

### Step 2：抽取 app types 与 adapter
- 新增 `src/app/types.rs`。
- 新增 `AgentEvent -> AppEvent` 的映射函数。
- 新增 `AppSession/AppAgent/AppMessage/AppEvent -> gateway::protocol::*` 的映射函数。

### Step 3：收敛 gateway contract
- 将 `GatewayApplication` 提取为 trait。
- 让 `GatewayHandler` 改为依赖 `Arc<dyn GatewayApplication>`。
- 移除 `connect()` / `disconnect()` 这类不属于应用层的接口设计。

### Step 4：拆分 SessionStore
- 提取 `SessionCache`。
- 提取 repository 返回的持久化结构体。
- 以 `SessionService` 统一封装缓存与持久化编排。

### Step 5：清理耦合
- 将 `app/application.rs` 中所有 `gateway::protocol::*` 返回值替换为 app types。
- 确认 gateway 侧不再直接依赖 `ConversationService` 或 `SessionStore` 内部细节。

## 测试案例

### 1. 协议兼容性回归
- 对拆分后的 `GatewayMessage`、`MessageEnvelope`、关键 payload 保留现有序列化/反序列化测试。
- 使用固定 JSON 样例验证拆分前后结构完全一致。

### 2. 应用层契约测试
- 基于内存 fake 或 mock 实现 `GatewayApplication`，验证 gateway handler 不依赖具体实现类型。
- 验证 `GatewayApplication` 的接口覆盖现有 gateway 路由所需能力，不再回退到具体类。

### 3. 事件映射测试
- 测试 `AgentEvent -> AppEvent` 的逐项映射。
- 测试 `AppEvent -> GatewayMessage` 的逐项映射。
- 验证 `TurnComplete`、`ToolLog`、`AgentSwitched` 等复杂事件无信息丢失。

### 4. 会话一致性测试
- 验证 cache miss 时会从 repository 加载并回填 cache。
- 验证并发读取同一缺失会话时，不会重复创建多个运行态实例。
- 验证 `append_message`、`switch_agent`、`copy_session`、`delete_session` 的内存态和持久态保持一致。

### 5. 并发与取消测试
- 验证同一 `session_id` 上的两个 `start_turn` 会被串行化。
- 验证 `stop_turn` 会触发取消 token。
- 验证 turn 正常结束和取消结束时都会清理 cancellation token。

### 6. 回归测试
- 运行现有 CLI 路径，确认 CLI 继续消费 `AgentEvent` 时行为不变。
- 运行 gateway 集成测试，确认 `chat.start`、`chat.progress`、`chat.complete`、错误响应行为不变。

## 风险与待定项

1. **Trait object 与泛型收口**：`GatewayApplication<C>` 当前带有 `LlmClient` 泛型。实现上建议在 bootstrap 阶段完成具体类型实例化，再以 `Arc<dyn GatewayApplication>` 向 gateway 暴露，避免把泛型继续泄漏到 handler 层。
2. **事件模型短期双轨**：本轮会暂时保留 `AgentEvent` 与 `AppEvent` 并存，这是受控设计，不是重复设计。必须通过明确映射边界防止两套事件各自演化。
3. **会话缓存一致性复杂度上升**：`SessionStore` 拆分后，状态边界更清晰，但并发场景下的正确性要求更高，必须依赖测试锁定行为。
4. **迁移范围较广**：涉及 `app`、`gateway`、`conversation` 三层，实施时必须严格按步骤推进，每一步完成后都执行完整检查，避免大爆炸式重构。
