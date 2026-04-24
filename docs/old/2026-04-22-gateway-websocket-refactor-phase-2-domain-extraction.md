# Gateway / WebSocket 重构 Phase 2 文档

## 时间
- 创建时间：2026-04-22
- 最后更新：2026-04-22

## 项目现状
- `src/gateway/handlers/chat.rs` 同时处理会话串行、交互恢复、Agent 切换确认、Workflow 推进、Agent 执行和事件桥接。
- `src/gateway/session.rs`、`control.rs`、`workflow.rs`、`agents.rs` 均位于 `gateway` 目录，导致“渠道层”和“会话 / Agent 领域层”职责混杂。
- `src/gateway/router.rs` 的 `AppState` 直接持有 `AgentRuntime`、`AgentRegistry` 和 `SessionStore`，WebSocket handler 与底层业务对象直接耦合。

## 本次目标
- 将 Agent 目录、会话控制、Workflow、Session 持久化相关能力从 `gateway` 命名空间中拆出。
- 新增应用服务层，把当前散落在 handler 中的业务流程收拢成可测试的 service。
- 在不拆 WebSocket server 的前提下，让 `gateway` 退化为协议适配和路由层。

## 详细设计

### 目标目录
- 新增或调整为以下结构：
  - `src/app/`
  - `src/conversation/`
  - `src/agent_catalog/` 或 `src/agent_catalog.rs`
- `src/gateway` 在 Phase 2 结束时仅保留：
  - `protocol.rs`
  - `router.rs`
  - `server.rs`
  - 少量渠道适配 handler

### 模块迁移设计

#### 1. Agent Catalog
- 将 `src/gateway/agents.rs` 移至 `src/agent_catalog.rs` 或 `src/agent_catalog/mod.rs`。
- `AgentRegistry` 职责收缩为：
  - Agent descriptor 注册与查询
  - 主 Agent 标识管理
  - 点名解析
- `AgentRegistry` 不再出现在 `gateway::*` 命名空间中。

#### 2. Conversation Domain
- 将以下模块迁移到 `src/conversation/`：
  - `session.rs`
  - `control.rs`
  - `workflow.rs`
  - `sqlite_session_repository.rs`
  - `sqlite_manager.rs` 如果其唯一使用方是会话存储
- 迁移后的边界：
  - `Session` 是会话领域对象
  - `SessionStore` 后续可继续拆成 `SessionRepository + SessionCache + SessionService`
  - `WorkflowState`、`WorkflowEngine` 属于会话编排，不属于渠道层

#### 3. Application Service
- 新增 `src/app/conversation_service.rs`，承接当前 `handlers/chat.rs` 的流程编排。
- 建议对外提供：
  - `handle_chat`
  - `handle_chat_stop`
  - `handle_session_create`
  - `handle_session_copy`
  - `handle_session_delete`
  - `list_agents`
  - `switch_agent`
- `ConversationService` 依赖：
  - `AgentRuntime`
  - `AgentRegistry`
  - `SessionService` 或当前过渡期 `SessionStore`

#### 4. Gateway Handler 退化
- `src/gateway/handlers/chat.rs` 变为薄适配层：
  - 解析协议 DTO
  - 调用 `ConversationService`
  - 将返回的应用事件写入 outbound sink
- handler 中不再直接读取 `session.control`、不再直接操作 workflow、也不再直接持有事件桥接细节。

### 事件映射设计
- `src/gateway/bridge.rs` 建议迁移为 `src/app/event_mapper.rs`。
- 分两步做映射：
  - `AgentEvent -> AppEvent`
  - `AppEvent -> GatewayMessage`
- Phase 2 不要求一次性设计为多渠道通用协议，但必须把“业务事件”和“WebSocket DTO”从语义上分开。

## 实施步骤

### Step 1: 迁移纯领域模块
- 先移动 `agents.rs`、`control.rs`、`workflow.rs`、`session.rs` 到新目录。
- 仅调整 `mod` 和引用路径，不改行为。
- 这一小步完成后先跑完整编译与测试，保证迁移只是换边界，不改变逻辑。

### Step 2: 提炼服务接口
- 从 `handlers/chat.rs` 抽取 `ConversationService`。
- 把会话锁、取消、交互恢复、Workflow 推进、普通 Agent turn 统一收口到 service。
- 让 service 以领域对象和应用事件作为输入输出，而不是直接依赖 WebSocket 发包。

### Step 3: 收口 sessions / agents / config handler
- `handlers/sessions.rs` 和 `handlers/agents.rs` 改为调用 service。
- `router.rs` 只负责协议分发，不再知道底层业务实现细节。

### Step 4: 调整 `AppState`
- 将 `AppState` 从“对象仓库”改为“服务仓库”。
- 示例：
  - `conversation_service`
  - `config_service`
  - `gateway_presenter` 或事件编码器

## 阶段完成后的功能完整性要求
- Phase 2 完成后，外部可见功能必须与 Phase 1 保持一致，不允许出现“重构后能力减少”。
- 完成后程序必须继续支持：
  - 聊天主流程
  - 聊天取消
  - Session 生命周期操作
  - Agent 列表与切换
  - 交互请求和恢复
  - 配置读取与更新
- 完成后必须满足内部约束：
  - `gateway` 命名空间不再包含 AgentRegistry、WorkflowState、SessionStore 的主定义
  - `handlers/chat.rs` 不再直接操作会话内部状态
  - 业务测试可以不经过 WebSocket server 直接验证 `ConversationService`

## 测试案例
- 单元测试：
  - `ConversationService::handle_chat` 正常发送 start / progress / complete
  - `handle_chat_stop` 可取消运行中 turn
  - pending interaction 存在时，输入先进入 interaction resolution 分支
  - 切换 Agent 只更新控制状态，不破坏历史消息
- 回归测试：
  - 原有协议测试全部继续通过
  - Session 创建、删除、复制的行为不变
- 结构性验证：
  - `cargo test --workspace` 中存在不经过 `gateway::server` 的 service 级测试

## 风险与待定项
- 风险：
  - `SessionStore` 当前同时承担 repository、cache 和 domain service，直接迁移时容易把旧耦合原样带过去。
  - `chat.rs` 的逻辑较重，抽 service 时如果把发包逻辑与业务逻辑拆得不彻底，会形成新的伪分层。
- 待定项：
  - `workflow.rs` 是否继续保留完整语义。如果近期不作为主线，建议在 Phase 2 中同步继续收缩其职责，避免后续继续影响应用层边界。

