# Gateway 削减与优化计划

## 时间
- 创建时间：2026-04-23
- 最后更新：2026-04-23

## 项目现状

当前代码的真实主链路是：

`nova_gateway` -> `app::bootstrap` -> `GatewayApplication` -> `ConversationService` -> `AgentRuntime`

这条链路已经可以支撑“单 Agent 会话 + 会话持久化 + WebSocket 网关”的核心能力，但代码中还叠加了几组没有形成闭环的中间概念：

1. `conversation/control.rs`
   - `ControlState` 同时承载 `active_agent`、`pending_interaction`、`workflow`
   - `TurnRouter` 在真正进入 Agent 前，做一层启发式意图分类
2. `conversation/workflow.rs`
   - `WorkflowCandidate`、`WorkflowStage`、`WorkflowEngine`
   - 当前实现仍是原型性质，候选方案为硬编码，未接入真实搜索、执行、测试链路
3. 旧设计文档 [2026-04-22-gateway-websocket-refactor-design.md](/D:/git/zero-nova/docs/2026-04-22-gateway-websocket-refactor-design.md)
   - 第 134-167 行描述了 `app / conversation / agent_runtime / agent_catalog` 的更重分层
   - 但当前代码并未达到该结构，实际仍是 Gateway 主导的集成式实现

基于现状，可以确认几个问题：

1. `Workflow` 不是实际业务闭环，而是嵌在对话链路中的半成品分支
2. `TurnRouter` 不是稳定的领域边界，只是把“正常聊天”前插入了一层规则分流
3. `ControlState` 承担了过多职责，且其中 `pending_interaction`、`workflow` 都不是可靠持久态
4. `app`、`conversation`、`gateway` 的边界不干净
   - `GatewayApplication` 直接返回 `gateway::protocol` DTO
   - `SessionStore` 也直接依赖 `gateway::protocol`
5. 当前最稳定、最值得保留的能力，其实只有：
   - Session 生命周期管理
   - Agent 注册与切换
   - Agent 单轮执行
   - Gateway WebSocket 收发

结论：接下来不应继续补齐 `Workflow` 体系，也不应继续追逐 2026-04-22 文档中的重型目标架构；更合适的方向是先回到“单 Agent 会话系统”的最小闭环，再逐步把边界清理干净。

## 本次目标

本计划的目标是通过一轮削减和整理，把项目收敛到一个更小、更稳的可维护结构：

1. 删除与当前 Agent 主链路无关、且未形成闭环的伪领域能力
2. 把会话主路径收敛成“显式指令 + 直接执行”，去掉隐式启发式路由
3. 让 `conversation` 回归“Session 与历史存储”职责，不再承担工作流编排
4. 让后续演进以真实落地能力为前提，而不是继续堆叠设计中的抽象层

验收标准：

1. 会话仍可创建、列出、复制、删除、读取消息
2. 聊天请求仍可正常驱动 `AgentRuntime`
3. Agent 切换能力仍可用，但不依赖 `TurnRouter` 或 `Workflow`
4. `conversation/workflow.rs`、`WorkflowCandidate`、`TurnRouter`、大部分 `ControlState` 扩展字段被删除或收缩
5. 网关、应用服务、会话存储的职责比当前更清晰

## 详细设计

### 1. 目标收敛架构

本轮不追求旧文档中的多层重构，先收敛到下面这个更现实的结构：

```text
zero-nova
├─ crates/channel-websocket      # 纯传输层 (ChannelHandler trait / ResponseSink / run_server)
├─ src/gateway                   # WebSocket 协议、handler、server、bridge、router
│   ├─ protocol.rs               # GatewayMessage / MessageEnvelope / 传输 DTO
│   ├─ router.rs                 # handle_message() — 按 type 分发
│   ├─ server.rs                 # GatewayHandler<C> + run_server()
│   ├─ bridge.rs                 # agent_event_to_gateway() — 事件→协议转换
│   └─ handlers/
│       ├─ agents.rs             # handle_agents_list / handle_agents_switch
│       ├─ chat.rs               # handle_chat / handle_chat_stop
│       ├─ sessions.rs           # session CRUD
│       └─ config.rs             # config 读写
├─ src/app                       # 应用门面，组织用例
│   ├─ application.rs            # GatewayApplication<C> — 对外门面
│   ├─ conversation_service.rs   # ConversationService<C> — 轮次编排
│   └─ bootstrap.rs              # 初始化 + 启动
├─ src/conversation              # session/history/repository
│   ├─ mod.rs                    # 导出 SqliteSessionRepository + SessionStore
│   ├─ session.rs                # Session / SessionStore
│   ├─ control.rs                # ControlState（收缩后）
│   ├─ repository.rs             # SqliteSessionRepository
│   └─ sqlite_manager.rs         # 建库建表 + migration
├─ src/agent.rs                  # AgentRuntime<C>
├─ src/agent_catalog.rs          # AgentDescriptor / AgentRegistry
├─ src/event.rs                  # AgentEvent
├─ src/message.rs                # Role / ContentBlock / Message
├─ src/tool.rs                   # Tool trait / ToolRegistry
├─ src/provider/                 # LlmClient trait + provider 实现
└─ src/bin/nova_gateway.rs       # 启动入口
```

流转方向：

```
WebSocket 客户端
    → channel-websocket (run_server / ChannelHandler)
    → gateway/server.rs (GatewayHandler::on_message)
    → gateway/router.rs (handle_message → handlers)
    → app/application.rs (GatewayApplication 门面)
    → app/conversation_service.rs (ConversationService)
    ├→ session.rs (SessionStore — 持久化)
    ├─ → control.rs (ControlState — 当前活跃 agent)
    └→ agent.rs (AgentRuntime — LLM 调用)
```

约束说明：

1. `conversation` 只保留会话状态与持久化，不再维护工作流
2. `app` 只编排用例，不做启发式意图识别
3. `gateway` 只解析协议并调用应用服务，不承担领域决策
4. Agent 切换通过显式 API 或显式消息完成，不再通过自然语言猜测
5. `conversation` 模块内部不依赖 `gateway::protocol`
6. `gateway::bridge` 不再产出 `InteractionRequest` / `InteractionResolved` 消息（随交互状态机删除）

### 2. 建议删除的能力

#### 2.1 删除 `Workflow` 全链路

删除范围：

| # | 文件 / 符号 | 说明 |
|---|------------|------|
| 1 | `src/conversation/workflow.rs` | 整个文件 |
| 2 | `ConversationService::advance_workflow` | 私有方法 |
| 3 | `ConversationService::start_workflow` | 私有方法 |
| 4 | `control.rs` 中 `WorkflowState` 字段 | `ControlState.workflow: Option<WorkflowState>` |
| 5 | `mod.rs` 中 `pub mod workflow` 及导出 | |

删除原因：

1. 当前候选方案是硬编码，和 Agent 能力、工具系统、真实执行路径没有闭环
2. 工作流状态没有可靠持久化语义，重载会话后状态并不可信
3. 该模块增加了主链路分支复杂度，但没有提供稳定收益
4. 如果未来需要"方案对比/执行编排"，应该以单独能力重新设计，而不是挂在当前会话控制层里

#### 2.2 删除 `TurnRouter` 及其全部相关符号

删除范围：

| # | 文件 / 符号 | 说明 |
|---|------------|------|
| 1 | `src/conversation/control.rs` 中 `TurnIntent` | 整个 enum（含 5 个变体） |
| 2 | `src/conversation/control.rs` 中 `TurnRouter` | 整个 impl（含 `classify` + 私有 `detect_new_task`） |
| 3 | `ConversationService::start_turn` 中 `TurnRouter::classify` 调用 + match | |
| 4 | `control.rs` 中对 `TurnRouter` 的 `use` 导入（如被其他模块引用） | |

删除后 `start_turn` 简化为：

```rust
// 简化前 (conversation_service.rs:29-46)
pub async fn start_turn(...) -> Result<()> {
    let intent = TurnRouter::classify(input, &control, Some(&self.agent_registry));
    match intent {
        TurnIntent::ResolvePendingInteraction => self.resolve_interaction(...).await,
        TurnIntent::AddressAgent { agent_id } => self.request_agent_switch(...).await,
        TurnIntent::ContinueWorkflow => self.advance_workflow(...).await,
        TurnIntent::StartNewTask { topic } => self.start_workflow(...).await,
        TurnIntent::ExecuteChat => self.execute_agent_turn(...).await,
    }
}

// 简化后
pub async fn start_turn(&self, session_id: &str, input: &str, event_tx: mpsc::Sender<AgentEvent>) -> Result<()> {
    self.execute_agent_turn(session_id, input, event_tx).await
}
```

替代方式：

1. 普通聊天：直接进入 `execute_agent_turn`
2. Agent 切换：仅保留显式入口
   - 方案 A：继续使用现有 `AgentsSwitch` WebSocket 消息
   - 方案 B：新增应用服务方法 `switch_agent(session_id, agent_id)`

推荐采用方案 B 的实现方式，但协议层仍可继续沿用现有 `AgentsSwitch` 消息。

原因：

1. 通过自然语言猜测"你是不是想切 Agent / 开工作流"会制造不稳定行为
2. 系统已经有明确的网关命令面，不需要再在会话文本里做一轮路由器判断
3. 删除后 `start_turn` 会退化成更稳定的单路径执行

#### 2.3 删除交互确认状态机

删除范围：

| # | 文件 / 符号 | 说明 |
|---|------------|------|
| 1 | `src/conversation/control.rs` 中 `PendingInteraction` | 整个 struct |
| 2 | `src/conversation/control.rs` 中 `InteractionKind` | enum: Approve / Select / Input |
| 3 | `src/conversation/control.rs` 中 `InteractionOption` | struct |
| 4 | `src/conversation/control.rs` 中 `RiskLevel` | enum: Low / Medium / High |
| 5 | `src/conversation/control.rs` 中 `ResolutionIntent` | enum: Approve / Reject / Select / ProvideInput / Unclear |
| 6 | `src/conversation/control.rs` 中 `ResolutionResult` | struct |
| 7 | `src/conversation/control.rs` 中 `InteractionResolver::resolve` | 整个 impl |
| 8 | `ConversationService::resolve_interaction` | 私有方法 |
| 9 | `ConversationService::request_agent_switch` | 私有方法 |
| 10 | `control.rs` 中 `ControlState.pending_interaction: Option<PendingInteraction>` | 字段 |
| 11 | `bridge.rs` 中 `AgentEvent::InteractionRequest` 转换分支 | |
| 12 | `bridge.rs` 中 `AgentEvent::InteractionResolved` 转换分支 | |
| 13 | `protocol.rs` 中 `InteractionRequestPayload` | struct |
| 14 | `protocol.rs` 中 `InteractionResolvedPayload` | struct |
| 15 | `protocol.rs` 中 `InteractionOptionDTO` | struct |
| 16 | `protocol.rs` 中 `MessageEnvelope::InteractionRequest` | 变体 |
| 17 | `protocol.rs` 中 `MessageEnvelope::InteractionResolved` | 变体 |
| 18 | `event.rs` 中 `AgentEvent::InteractionRequest` | 变体 |
| 19 | `event.rs` 中 `AgentEvent::InteractionResolved` | 变体 |

#### 2.4 精简 `MessageEnvelope` 枚举

`MessageEnvelope` 当前有 25+ 个变体，删除 Interaction 相关后可精简：

| 移除的变体 | 说明 |
|-----------|------|
| `InteractionRequest(InteractionRequestPayload)` | 交互确认 |
| `InteractionResolved(InteractionResolvedPayload)` | 交互已解决 |

同时 `protocol/mod.rs`（如存在）或 `mod.rs` 中对 `InteractionRequestPayload`、`InteractionResolvedPayload`、`InteractionOptionDTO` 的 `use` / 导出也应一并移除。

### 3. 收缩 `ControlState`

收缩后：

```rust
pub struct ControlState {
    pub active_agent: String,
}
```

当前 `control.rs` 删除后的最终内容应仅为：

| 保留 | 删除（见 §2.2、§2.3） |
|------|----------------------|
| `ControlState` struct（仅 `active_agent`） | `TurnIntent` enum |
| `ControlState::new(default_agent: &str)` | `TurnRouter` impl |
| | `PendingInteraction` struct |
| | `InteractionKind` enum |
| | `InteractionOption` struct |
| | `RiskLevel` enum |
| | `ResolutionIntent` enum |
| | `ResolutionResult` struct |
| | `InteractionResolver::resolve` |

**是否保留 `ControlState` 命名待定**：
- 若最终只剩 `active_agent` 一个字段，可考虑直接内联到 `Session` 中
- 但这属于第二轮小优化，不必和第一轮删除动作混做
- 本轮先收缩字段，不改动 struct 名称

### 4. 新增 `ConversationService::switch_agent`

当前 `handle_agents_switch`（`gateway/handlers/agents.rs:19-46`）只校验 agent 是否存在，**并未真正执行切换**。简化后应将其改为显式切换：

这里不应该让 `ConversationService` 直接访问 `SessionStore` 的私有字段；更合理的做法是先在 `SessionStore` 补一个显式持久化入口。

**SessionStore 新增方法**：

```rust
impl SessionStore {
    pub async fn set_active_agent(&self, session_id: &str, agent_id: &str) -> Result<Arc<Session>> {
        let session = self.get(session_id).await?.context("Session not found")?;

        {
            let mut control = session.control.write().unwrap();
            control.active_agent = agent_id.to_string();
        }

        self.repository
            .save_session(
                &session.id,
                &session.name,
                agent_id,
                session.created_at,
                session.updated_at.load(Ordering::SeqCst),
            )
            .await?;

        Ok(session)
    }
}
```

**GatewayApplication 新增方法**：

```rust
impl<C: LlmClient + 'static> GatewayApplication<C> {
    // ... 现有方法 ...

    pub async fn switch_agent(&self, session_id: &str, agent_id: &str) -> Result<crate::gateway::protocol::Agent> {
        let agent = self.conversation_service.switch_agent(session_id, agent_id).await?;
        Ok(agent_to_protocol(&agent))
    }
}
```

**ConversationService 新增方法**：

```rust
impl<C: LlmClient + 'static> ConversationService<C> {
    // ... 简化后的 start_turn / stop_turn ...

    pub async fn switch_agent(&self, session_id: &str, agent_id: &str) -> Result<AgentDescriptor> {
        let agent = self.agent_registry.get(agent_id)
            .cloned()
            .context(format!("Agent '{}' not found", agent_id))?;

        self.sessions.set_active_agent(session_id, agent_id).await?;

        Ok(agent)
    }
}
```

**handlers/agents.rs 变更**：

```rust
// 简化前 (agents.rs:19-46) — 只做校验，不切换
pub async fn handle_agents_switch(...) {
    match app.get_agent(&payload.agent_id) {
        Some(agent) => send response {}
        None => send error {}
    }
}

// 简化后
pub async fn handle_agents_switch(payload, app, outbound_tx, request_id) {
    match app.switch_agent(&payload.session_id, &payload.agent_id).await {
        Ok(agent) => send response(agent)
        Err(e) => send error(e)
    }
}
```

注意：这要求 `AgentIdPayload` 或 `AgentsSwitch` 消息中携带 `session_id`。如当前 payload 中不包含 `session_id`，需要在 `protocol.rs` 中确认或新增一个携带 `session_id` 的请求体。

### 5. `ConversationService` 最终形态

简化后 `ConversationService` 仅有 3 个公开异步方法：

| 方法 | 职责 | 返回值 |
|------|------|--------|
| `start_turn(session_id, input, event_tx)` | 写入用户消息 → 调用 AgentRuntime → 写回输出 | `Result<()>` |
| `stop_turn(session_id)` | 取消当前运行 token | `Result<()>` |
| `switch_agent(session_id, agent_id)` | 校验 agent → 更新 `active_agent` → 持久化 | `Result<AgentDescriptor>` |

**简化后 `start_turn`**：

```rust
pub async fn start_turn(&self, session_id: &str, input: &str, event_tx: mpsc::Sender<AgentEvent>) -> Result<()> {
    self.execute_agent_turn(session_id, input, event_tx).await
}
```

原来 5 分支 match 退化为单路径调用，消除了启发式路由和原型性分支。

**`execute_agent_turn` 的逻辑不变**（`conversation_service.rs:56-92`），仍然：
1. 获取 session + 获取 chat_lock
2. 写入用户消息到 Session + 持久化
3. 创建 cancellation token 并设置
4. 获取历史（排除刚写入的用户消息，因为会进入 LLM 上下文）
5. 调用 `AgentRuntime::run_turn`
6. 将 agent 输出逐条写入 session + 持久化
7. 清理 cancellation token

**`switch_agent` 已在 §4 完整定义**，此处不再重复。

### 6. Agent 切换改为显式操作

当前 `request_agent_switch` / `resolve_interaction` 围绕"先生成待确认交互，再等用户回复"设计。该流程全部删除，改成显式切换：

1. 网关收到 `AgentsSwitch(session_id, agent_id)`
2. `GatewayApplication` 调用 `ConversationService::switch_agent`
3. 直接更新 session 的 `active_agent`
4. 返回成功响应

如果产品上确实需要"切换前确认"，建议由前端处理（前端弹确认框后发送 `AgentsSwitch`），而不是把确认状态塞进后端 Session。

**关键问题：`AgentIdPayload` 需要携带 `session_id`**

当前 `AgentIdPayload`（`protocol.rs:219-221`）只有 `agent_id` 字段，缺少 `session_id`：

```rust
// 修改前
pub struct AgentIdPayload {
    pub agent_id: String,
}

// 修改后（建议重命名为 SessionAgentSwitch）
pub struct SessionAgentSwitchPayload {
    pub session_id: String,
    pub agent_id: String,
}
```

`MessageEnvelope` 中 `AgentsSwitch` 的关联 payload 也需要相应调整。

### 7. 清理边界耦合

本轮不做彻底分层重构，但至少完成两项收口：

#### 7.1 `conversation` 不再依赖 `gateway::protocol`

当前依赖路径（session.rs 第 3 行）：

```rust
use crate::gateway::protocol::{ContentBlockDTO, MessageDTO, Session as SessionProtocol};
```

需要修改的位置：

| 文件 | 方法 | 当前 | 修改后 |
|------|------|------|--------|
| `session.rs` | `Session::get_messages_dto()` | 返回 `Vec<MessageDTO>`（来自 `gateway::protocol`） | 拆出内部方法 `get_internal_messages() -> Vec<Message>`，`gateway::protocol::MessageDTO` 转换放到 `app::application` |
| `session.rs` | `SessionStore::list_sorted()` | 返回 `Vec<SessionProtocol>`（来自 `gateway::protocol`） | 改为 `Vec<SessionSummary>`（内部结构：id, name, agent_id, created_at, updated_at, message_count），转换放到 `app::application` |

`app/application.rs` 中已有的转换函数 `session_to_protocol`（第 158 行）可以复用：

```rust
// app/application.rs — 新增
fn messages_to_protocol(messages: &[Message]) -> Vec<crate::gateway::protocol::MessageDTO> {
    messages.iter().map(|m| {
        /* 将 ContentBlock::Text/Thinking/ToolUse/ToolResult
           映射为 ContentBlockDTO */
    }).collect()
}

// list_sessions() 修改前：
pub async fn list_sessions(&self) -> Vec<crate::gateway::protocol::Session> {
    self.conversation_service.sessions.list_sorted().await  // 直接返回 protocol
}

// list_sessions() 修改后：
pub async fn list_sessions(&self) -> Vec<crate::gateway::protocol::Session> {
    let summaries = self.conversation_service.sessions.list_sorted().await;
    summaries.into_iter().map(|s| session_to_protocol_internal(&s)).collect()
}
```

这样做的目的是避免底层存储模块反向依赖最外层传输协议。

#### 7.2 保留 `GatewayApplication` 中的协议转换职责

当前 `GatewayApplication` 中：

- `list_sessions()` 直接返回 `Vec<gateway::protocol::Session>`
- `session_messages()` 返回 `Vec<gateway::protocol::MessageDTO>`
- `create_session()` / `copy_session()` 调用 `session_to_protocol()`
- `list_agents()` / `get_agent()` 调用 `agent_to_protocol()`
- `connect()` / `handle()` / `disconnect()` 使用 `gateway::protocol::GatewayMessage`

修改方向：

1. `session_to_protocol`、`agent_to_protocol`、`messages_to_protocol` 保留在 `app/application.rs` 中（作为内部转换函数），不放到 gateway 层
2. 这些转换仍由 `GatewayApplication` 承载，因为 `app` 是唯一知道领域对象和传输 DTO 的层
3. 不要为了"纯"而把转换函数搬到 `gateway` 模块 — 那样反而会导致 `app` 依赖 `gateway`（如果 `gateway` 中的 handler 需要转换的话）
4. 核心约束是：**`conversation` 不能知道 `gateway::protocol`**，`app` 知道 `gateway::protocol` 是完全合理的（因为它是门面，负责适配不同消费者）

**结论**：§7.1 的解耦边界是 `conversation` vs `gateway`，而不是 `app` vs `gateway`。`app` 作为门面持有协议转换是正常且合理的。

### 8. 建议实施顺序

按风险从低到高，分三步做。注意：涉及 interaction/event/protocol/bridge 的删除必须放在同一步里完成，中间态不能拆开。

#### Step 1：删除 Workflow

变更清单：

| # | 文件 | 变更操作 |
|---|------|---------|
| 1 | `src/conversation/workflow.rs` | 删除整个文件 |
| 2 | `src/conversation/mod.rs` | 删除 `pub mod workflow` 行 |
| 3 | `src/conversation/control.rs` | 删除 `ControlState.workflow` 字段 |
| 4 | `src/conversation/control.rs` | 删除 `use crate::conversation::workflow::` 导入 |
| 5 | `src/app/conversation_service.rs` | 删除 `use crate::conversation::workflow::` 导入 |
| 6 | `src/app/conversation_service.rs` | 删除 `advance_workflow()` 方法 |
| 7 | `src/app/conversation_service.rs` | 删除 `start_workflow()` 方法 |
| 8 | `src/app/conversation_service.rs` | `start_turn` 中删除 `ContinueWorkflow` / `StartNewTask` 分支 |

编译验证：`cargo check --all-targets` 应通过。

#### Step 2：删除 TurnRouter + 交互确认状态机 + 相关协议/事件桥接

变更清单：

| # | 文件 | 变更操作 |
|---|------|---------|
| 1 | `src/conversation/control.rs` | 删除 `TurnIntent` enum |
| 2 | `src/conversation/control.rs` | 删除 `TurnRouter` impl |
| 3 | `src/conversation/control.rs` | 删除 `PendingInteraction` struct |
| 4 | `src/conversation/control.rs` | 删除 `InteractionKind` enum |
| 5 | `src/conversation/control.rs` | 删除 `InteractionOption` struct |
| 6 | `src/conversation/control.rs` | 删除 `RiskLevel` enum |
| 7 | `src/conversation/control.rs` | 删除 `ResolutionIntent` enum |
| 8 | `src/conversation/control.rs` | 删除 `ResolutionResult` struct |
| 9 | `src/conversation/control.rs` | 删除 `InteractionResolver` impl |
| 10 | `src/conversation/control.rs` | 删除 `ControlState.pending_interaction` 字段 |
| 11 | `src/app/conversation_service.rs` | 删除 `use` 中的 `TurnIntent, TurnRouter, InteractionKind, InteractionResolver, ResolutionIntent` |
| 12 | `src/app/conversation_service.rs` | 删除 `resolve_interaction()` 方法 |
| 13 | `src/app/conversation_service.rs` | 删除 `request_agent_switch()` 方法 |
| 14 | `src/app/conversation_service.rs` | 删除 `map_resolution_result()` 辅助函数 |
| 15 | `src/app/conversation_service.rs` | `start_turn` 删除 `TurnRouter::classify` + match，改为单行调用 `execute_agent_turn` |
| 16 | `src/event.rs` | 删除 `AgentEvent::InteractionRequest` 变体 |
| 17 | `src/event.rs` | 删除 `AgentEvent::InteractionResolved` 变体 |
| 18 | `src/event.rs` | 删除 `InteractionOptionEvent` struct |
| 19 | `src/gateway/bridge.rs` | 删除 `AgentEvent::InteractionRequest` 分支与相关 import |
| 20 | `src/gateway/bridge.rs` | 删除 `AgentEvent::InteractionResolved` 分支与相关 import |
| 21 | `src/gateway/protocol.rs` | 删除 `MessageEnvelope::InteractionRequest` 变体 |
| 22 | `src/gateway/protocol.rs` | 删除 `MessageEnvelope::InteractionResolved` 变体 |
| 23 | `src/gateway/protocol.rs` | 删除 `InteractionOptionDTO` / `InteractionRequestPayload` / `InteractionResolvedPayload` |
| 24 | `src/bin/nova_cli.rs` | 删除 `InteractionRequest` / `InteractionResolved` 的事件展示分支 |

编译验证：`cargo check --all-targets` 应通过。

#### Step 3：更新 Agent 切换链路 + 收口 `conversation` → `gateway::protocol` 依赖

变更清单：

| # | 文件 | 变更操作 |
|---|------|---------|
| 1 | `src/conversation/session.rs` | 新增 `set_active_agent(session_id, agent_id)`，负责更新内存与持久化 |
| 2 | `src/app/conversation_service.rs` | 新增 `switch_agent()` 公开方法 |
| 3 | `src/app/application.rs` | 新增 `switch_agent()` 公开方法，并把 `AgentDescriptor` 转为 protocol `Agent` |
| 4 | `src/gateway/handlers/agents.rs` | `handle_agents_switch` 改为调用 `app.switch_agent(session_id, agent_id)` |
| 5 | `src/gateway/router.rs` | `AgentsSwitch` 路由调用处适配新的 payload |
| 6 | `src/gateway/protocol.rs` | 新增 `SessionAgentSwitchPayload` struct（含 `session_id` + `agent_id`） |
| 7 | `src/gateway/protocol.rs` | 将 `AgentsSwitch(AgentIdPayload)` 改为 `AgentsSwitch(SessionAgentSwitchPayload)` |
| 8 | `src/gateway/protocol.rs` | 更新 `agents.switch` 反序列化测试 |
| 9 | `src/conversation/session.rs` | 删除 `use crate::gateway::protocol::{ContentBlockDTO, MessageDTO, Session as SessionProtocol}` |
| 10 | `src/conversation/session.rs` | `Session::get_messages_dto()` 拆分为 `get_internal_messages() -> Vec<Message>` |
| 11 | `src/conversation/session.rs` | `SessionStore::list_sorted()` 改为返回 `Vec<SessionSummary>`（新增内部 struct） |
| 12 | `src/app/application.rs` | `list_sessions()` 中新增从内部结构 → `gateway::protocol::Session` 的转换 |
| 13 | `src/app/application.rs` | `session_messages()` 中使用新的转换逻辑 |

**`AgentIdPayload` vs `SessionAgentSwitchPayload` 的取舍**：

考虑到当前已有 `ChatPayload`（含 `session_id`），而 Agent 切换需要作用到特定 session，建议：
- 删除 `AgentIdPayload`
- 新增 `SessionAgentSwitchPayload { session_id: String, agent_id: String }`
- 这样前端发送切换消息时明确指定目标 session

编译验证：`cargo check --all-targets` 应通过。

#### Step 4：收口 `conversation` → `gateway::protocol` 依赖

变更清单：

| # | 文件 | 变更操作 |
|---|------|---------|
| 1 | `src/conversation/session.rs` | 删除 `use crate::gateway::protocol::{ContentBlockDTO, MessageDTO, Session as SessionProtocol}` |
| 2 | `src/conversation/session.rs` | `Session::get_messages_dto()` 拆分为 `get_internal_messages() -> Vec<Message>` |
| 3 | `src/conversation/session.rs` | `SessionStore::list_sorted()` 改为返回 `Vec<SessionSummary>`（新增内部 struct） |
| 4 | `src/app/application.rs` | `list_sessions()` 中新增从内部结构 → `gateway::protocol::Session` 的转换 |
| 5 | `src/app/application.rs` | `session_messages()` 中使用新的转换逻辑 |

新增内部结构：

```rust
// src/conversation/session.rs
pub struct SessionSummary {
    pub id: String,
    pub name: String,
    pub agent_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: usize,
}
```

```rust
// src/app/application.rs — 新增转换
fn session_summary_to_protocol(summary: &crate::conversation::session::SessionSummary)
    -> crate::gateway::protocol::Session
{
    crate::gateway::protocol::Session { ... }
}

fn messages_to_gateway_protocol(messages: &[crate::message::Message])
    -> Vec<crate::gateway::protocol::MessageDTO>
{
    messages.iter().map(|m| { ... }).collect()
}
```

编译验证：`cargo check --all-targets` 应通过。之后运行 `cargo clippy --workspace -- -D warnings` + `cargo fmt --check --all` + `cargo test --workspace`。

## 测试案例

### 1. 正常路径

1. 创建 session，指定 agent，确认 session 成功持久化
2. 发送一条 chat 消息，确认用户消息与 assistant 消息均正确落库
3. 调用 stop，确认正在执行的 turn 能被取消
4. 调用显式切换 agent，确认 `active_agent` 更新成功
5. 列出 sessions，确认最新 agent_id、message_count、updated_at 正确

### 2. 边界条件

1. 对不存在的 session 发起 chat，返回明确错误
2. 对不存在的 session 发起 switch，返回明确错误
3. 切换到不存在的 agent，返回明确错误
4. 在空历史 session 上首次聊天，确认 system prompt 与首轮消息行为正确
5. 复制 session 后继续聊天，确认新旧 session 相互隔离

### 3. 回归场景

1. 删除 workflow 后，原有 chat handler 仍能正常工作
2. 删除 interaction 状态机后，`AgentsSwitch` handler 仍可完成切换
3. 重启服务并重新加载 session，确认不再依赖无法恢复的 `workflow/pending_interaction` 状态
4. `cargo clippy --workspace -- -D warnings`
5. `cargo fmt --check --all`
6. `cargo test --workspace`

## 风险与待定项

1. 如果桌面端或前端已经依赖“后端发起确认交互再切换 agent”的行为，需要同步调整 UI 流程
2. 删除 `workflow` 后，部分“方案推荐/部署引导”的产品设想会暂时下线；这属于有意收缩，不是回归
3. `conversation` 与 `gateway::protocol` 的解耦可能会波及多个 handler，建议放在删除 workflow 之后单独做
4. 是否保留 `ControlState` 这个命名仍待定
   - 若最终只剩 `active_agent`
   - 可考虑直接并入 `Session`
   - 但这属于第二轮小优化，不必和第一轮删除动作混做

## 最终建议

本轮优化的核心不是“重构成更漂亮的架构图”，而是先承认当前系统的真实重心：它本质上是一个带 Session 的 Agent Gateway。

因此推荐执行策略是：

1. 先删 `Workflow`
2. 再一口气删掉 `TurnRouter + pending_interaction + interaction protocol/event/bridge`
3. 最后补显式 `switch_agent`，并收口 `conversation/app/gateway` 边界

这样做可以用最小成本把系统从“半成品多分支状态机”收回到“稳定的单主路径服务”，后续如果真要做更强的多 Agent 协作或任务编排，再以独立能力重新设计，而不是继续在当前壳子上叠加。
