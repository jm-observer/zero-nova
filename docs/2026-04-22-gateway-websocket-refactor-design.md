# Gateway / Agent 解耦与渠道 WebSocket 库化设计文档

| 时间 | 2026-04-22 |
| :--- | :--- |
| 项目现状 | `src/gateway` 当前同时承担 WebSocket 接入、协议 DTO、消息路由、Session 持久化、控制流、Workflow、Agent 目录与运行时编排。 |
| 本次目标 | 将属于 Agent 的能力从 `gateway` 移出；把“与渠道相关的 WebSocket”抽成独立库；删除未落地或长期 stub 的冗余协议与逻辑；给出可分阶段实施的迁移设计。 |

---

## 1. 背景

当前 `gateway` 名称与实际职责不一致。它不是单纯的网络网关，而是一个把以下能力揉在一起的复合模块：

1. WebSocket server 生命周期管理
2. 前后端协议定义与序列化
3. Session 内存态与 SQLite 持久化
4. 会话控制状态机
5. Workflow 状态推进
6. Agent 注册、点名识别、切换确认
7. `AgentRuntime` 执行结果桥接

这种结构在原型期推进快，但会带来两个直接问题：

1. “渠道适配” 与 “Agent 业务” 无法独立演进
2. 协议与处理器里积累了大量 stub、空分支和一次性设计，长期维护成本高

用户这次的诉求本质上是一次边界重画：

1. `gateway` 不再拥有 Agent 相关业务语义
2. 与渠道相关的 WebSocket 收敛为独立库
3. 冗余设计显式删除，而不是继续保留“以后也许会用”的接口

## 2. 现状评审

### 2.1 当前职责分布

按代码结构看，`src/gateway` 内至少混入了四层职责：

| 层次 | 当前文件 | 现状问题 |
|------|----------|----------|
| 传输层 | `server.rs`、`protocol.rs` | 传输层直接感知业务事件模型 |
| 应用路由层 | `router.rs`、`handlers/*` | 路由层直接依赖 `AgentRuntime`、SessionStore、配置写入 |
| 领域控制层 | `control.rs`、`workflow.rs`、`agents.rs` | 实际属于会话编排或 Agent 领域，不应放在渠道模块 |
| 基础设施层 | `sqlite_manager.rs`、`sqlite_session_repository.rs`、`session.rs` | 与网络无关，却绑在 gateway 目录下 |

### 2.2 关键耦合点

#### A. `start_server` 同时做系统装配与渠道启动

`src/gateway/mod.rs` 当前不仅启动 WebSocket，还负责：

1. 装配 `ToolRegistry`
2. 加载 skills
3. 构造 `AgentRegistry`
4. 构造 `AgentRuntime`
5. 初始化 SQLite
6. 载入所有 Session

这意味着“启动一个渠道 server”必须同时知道 Agent、Tool、Skill、Session 持久化全部细节，导致渠道无法独立复用。

#### B. 路由层直接持有 `AgentRuntime`

`AppState` 里直接存放：

1. `agent: AgentRuntime<C>`
2. `agent_registry: AgentRegistry`
3. `sessions: SessionStore`

这说明当前 WebSocket handler 不是在调用“应用服务接口”，而是在直接操纵底层运行时对象。任何 Agent 侧改动都会穿透到渠道层。

#### C. `handlers/chat.rs` 夹杂会话控制、工作流和 Agent 执行

`handle_chat` 当前理论上应该只处理一个入站命令，但实际上同时负责：

1. 取 session 并串行化会话锁
2. 解析交互恢复
3. Agent 切换确认
4. Workflow 推进
5. 真正的 Agent turn 执行
6. 事件桥接与持久化

这已经不是 handler，而是一个巨大的 application service，只是放错了位置。

#### D. `protocol.rs` 变成“前端所有想过的消息全集”

`MessageEnvelope` 中存在大量问题：

1. 明显长期未实现的变体
2. 多个渠道/功能域消息混在一个枚举中
3. 大量 `Value` 占位，缺少领域类型
4. 空分支吞消息，导致协议表面支持、运行时实际无行为

这会让协议演进非常危险，因为“能反序列化”不代表“系统真正支持”。

### 2.3 明确可删的冗余设计

结合当前代码，以下内容建议在重构中直接删除，而不是继续迁移：

1. `protocol.rs` 中没有后端实现、也没有测试覆盖的消息类型
2. `router.rs` 中直接空处理的消息分支
3. `workflow.rs` 内明显原型性质的假数据流程
4. `bridge.rs` 中只服务单一 UI 协议命名、但没有抽象价值的事件映射细节
5. `agents.create` 这类当前没有真实落库、没有完整生命周期管理的接口
6. `scheduler.*`、`memory.*`、`browser.*`、`openflux.*`、`voice.*` 中未闭环的控制面消息

建议标准很简单：

1. 没有业务 owner
2. 没有后端实现
3. 没有自动化测试
4. 未来 1 到 2 个迭代内不会恢复

满足其中三条就不应继续保留在主协议里。

## 3. 本次设计目标

### 3.1 核心目标

1. 渠道层只负责“连接、协议、收发、会话关联、错误边界”
2. Agent 相关语义下沉到独立的应用层或领域层模块
3. WebSocket 渠道能力可独立复用到其他前端/渠道
4. 协议收缩为“当前真实支持”的最小集合
5. 迁移过程可分阶段推进，不要求一次性重写

### 3.2 非目标

本次设计不追求：

1. 立即支持多渠道统一抽象 DSL
2. 一次性重做所有 Session 持久化模型
3. 让 WebSocket 库直接感知 LLM、Tool、Skill、Workflow
4. 为未来猜测的功能保留预埋消息类型

## 4. 目标架构

建议把现有能力拆成三层，而不是继续围绕 `gateway` 扩张。

```text
zero-nova
├─ crates/
│  └─ channel-websocket/
│     ├─ protocol
│     ├─ server
│     ├─ connection
│     └─ codec
├─ src/
│  ├─ app/
│  │  ├─ conversation_service.rs
│  │  ├─ session_service.rs
│  │  └─ channel_api.rs
│  ├─ agent_runtime/
│  ├─ conversation/
│  │  ├─ control.rs
│  │  ├─ workflow.rs
│  │  └─ session.rs
│  ├─ agent_catalog/
│  └─ bin/
│     └─ nova_gateway.rs
```

说明：

1. `channel-websocket` 是纯渠道库，不包含 Agent 业务
2. `src/app` 作为应用服务层，承接渠道调用
3. `conversation` 承载 Session、控制状态、Workflow 等会话领域
4. `agent_catalog` 或 `agent_runtime` 承载 Agent 注册与执行能力

## 5. 模块边界设计

### 5.1 `channel-websocket` 独立库

建议新建 workspace 成员：

`crates/channel-websocket`

职责仅包含：

1. WebSocket 监听与连接管理
2. 入站消息解码与出站消息编码
3. Ping/Pong、关闭、背压、连接级错误
4. 将协议消息交给上层 `ChannelHandler`

不包含：

1. `AgentRuntime`
2. `SessionStore`
3. `SqliteSessionRepository`
4. Workflow
5. Agent registry
6. 配置文件读写

建议接口如下：

```rust
#[async_trait::async_trait]
pub trait ChannelHandler: Send + Sync + 'static {
    type Request;
    type Response;

    async fn on_connect(&self, peer: SocketAddr) -> anyhow::Result<Vec<Self::Response>>;
    async fn on_message(
        &self,
        peer: SocketAddr,
        message: Self::Request,
        sink: ResponseSink<Self::Response>,
    ) -> anyhow::Result<()>;
    async fn on_disconnect(&self, peer: SocketAddr) -> anyhow::Result<()>;
}
```

这里的关键点是：WebSocket 库只知道“请求”和“响应”是可序列化对象，不知道其业务含义。

### 5.2 协议层拆分

当前 `protocol.rs` 过大，建议按功能域拆分：

```text
crates/channel-websocket/src/protocol/
├─ envelope.rs
├─ chat.rs
├─ session.rs
├─ system.rs
└─ error.rs
```

同时做两件事：

1. 删除未实现消息类型
2. 用显式 DTO 替换 `serde_json::Value` 占位

建议将协议分成两个集合：

1. `transport protocol`
2. `application command/event`

示例：

```rust
pub struct Envelope<T> {
    pub id: Option<String>,
    #[serde(flatten)]
    pub body: T,
}

pub enum ClientCommand {
    Chat(ChatCommand),
    ChatStop(ChatStopCommand),
    SessionsList,
    SessionsCreate(CreateSessionCommand),
    SessionsDelete(DeleteSessionCommand),
    SessionsMessages(SessionMessagesCommand),
    AgentsList,
}

pub enum ServerEvent {
    Welcome(WelcomeEvent),
    ChatStarted(ChatStartedEvent),
    ChatDelta(ChatDeltaEvent),
    ChatCompleted(ChatCompletedEvent),
    SessionsListed(SessionsListedEvent),
    Error(ErrorEvent),
}
```

这样渠道库只依赖协议 crate 或协议模块，不需要依赖业务实现。

### 5.3 应用服务层

新增一个渠道无关的应用接口，例如：

```rust
#[async_trait::async_trait]
pub trait GatewayApplication: Send + Sync + 'static {
    async fn connect(&self, peer: PeerContext) -> anyhow::Result<Vec<AppEvent>>;
    async fn handle(&self, command: AppCommand, emitter: AppEmitter) -> anyhow::Result<()>;
    async fn disconnect(&self, peer: PeerContext) -> anyhow::Result<()>;
}
```

WebSocket 渠道只把消息转换成 `AppCommand`，再把 `AppEvent` 回写给前端。

这层负责：

1. Session 查找
2. 调用 conversation / agent 服务
3. 错误到应用事件的映射
4. request_id 与 session_id 关联

### 5.4 会话领域层

当前 `gateway/session.rs`、`control.rs`、`workflow.rs` 更适合挪到：

```text
src/conversation/
├─ mod.rs
├─ session.rs
├─ control.rs
├─ workflow.rs
└─ repository.rs
```

这样可以明确表达：

1. Session 是会话领域对象，不是网关对象
2. Workflow 是对话编排的一部分，不是渠道能力
3. 持久化仓储是会话基础设施，不是网络基础设施

### 5.5 Agent 相关模块

当前 `gateway/agents.rs` 建议拆成两块：

1. `src/agent_catalog.rs` 或 `src/agent/catalog.rs`
2. `src/app/agent_switch_service.rs` 或集成进 conversation service

`AgentRegistry` 的职责应仅限：

1. Agent descriptor 管理
2. 点名解析
3. 主 Agent 配置

不应再与 WebSocket handler 并列存在于 `gateway` 命名空间。

## 6. 推荐删除后的最小协议集合

第一阶段建议只保留真实闭环的消息：

### 6.1 客户端命令

1. `chat`
2. `chat.stop`
3. `sessions.list`
4. `sessions.messages`
5. `sessions.create`
6. `sessions.delete`
7. `sessions.copy`
8. `agents.list`
9. `agents.switch`
10. `config.get`
11. `config.update`

### 6.2 服务端事件 / 响应

1. `welcome`
2. `error`
3. `chat.start`
4. `chat.progress`
5. `chat.complete`
6. `chat.stop.response`
7. `sessions.list.response`
8. `sessions.messages.response`
9. `sessions.create.response`
10. `sessions.delete.response`
11. `sessions.copy.response`
12. `agents.list.response`
13. `agents.switch.response`
14. `config.get.response`
15. `config.update.response`
16. `interaction.request`
17. `interaction.resolved`

### 6.3 明确下线的协议

建议从主枚举中移除：

1. `agents.create`
2. `scheduler.*`
3. `memory.*`
4. `settings.get`
5. `browser.*`
6. `router.config.*`
7. `weixin.*`
8. `voice.*`
9. `openflux.*`
10. `language.update`

如果未来确实需要，按独立 feature 或独立协议模块重新引入，不在主干里预留空壳。

## 7. 应用层详细设计

### 7.1 会话编排服务

建议新增 `ConversationService`，把 `handlers/chat.rs` 里分散的逻辑收拢。

```rust
pub struct ConversationService<C: LlmClient> {
    agent_runtime: AgentRuntime<C>,
    agent_catalog: AgentCatalog,
    sessions: SessionService,
}
```

对外提供能力：

1. `start_turn`
2. `stop_turn`
3. `resolve_interaction`
4. `switch_agent`
5. `list_agents`

这样 `handlers/chat.rs` 可以退化为简单适配层，后续甚至完全消失。

### 7.2 Session 服务

把当前 `SessionStore` 的职责拆开：

1. `SessionRepository` 负责 SQLite 访问
2. `SessionCache` 负责内存态 session 缓存
3. `SessionService` 负责创建、复制、删除、追加消息

理由：

1. 当前 `SessionStore` 既是 repository 又是 cache 又是 domain service，太重
2. 后续如果需要换存储实现，不应改动渠道层

### 7.3 Agent 事件桥接

`bridge.rs` 不应继续叫 bridge 并放在 gateway 下。

更合理的命名：

1. `src/app/event_mapper.rs`
2. `src/app/agent_event_mapper.rs`

职责：

1. `AgentEvent -> AppEvent`
2. `AppEvent -> ChannelEvent`

如果未来增加桌面端 IPC、HTTP SSE 或其他渠道，这层映射可以复用。

## 8. 迁移步骤

建议分四个阶段执行，每一步都保持系统可运行。

### Phase 1: 先收缩协议，停止继续扩散

目标：

1. 删除空分支与无实现消息
2. 给保留消息补测试
3. 保持目录结构暂时不变

具体动作：

1. 清理 `protocol.rs` 未实现枚举
2. 清理 `router.rs` 的吞消息分支
3. 让所有不支持命令统一返回 `NOT_IMPLEMENTED`
4. 为保留协议补序列化 / 反序列化测试

收益：

1. 先降低噪音
2. 为后续拆库缩小搬迁范围

### Phase 2: 提取会话与 Agent 业务

目标：

1. `gateway` 不再定义 Agent 和 Workflow 领域对象

具体动作：

1. 移动 `agents.rs` 到 `src/agent_catalog.rs`
2. 移动 `control.rs`、`workflow.rs`、`session.rs` 到 `src/conversation/`
3. 提炼 `ConversationService`
4. 将 `handlers/chat.rs` 改成调用 service

收益：

1. 网关开始只剩“路由”而不是“业务实现”

### Phase 3: 提取 `channel-websocket` 库

目标：

1. 让 WebSocket server 脱离当前 crate 内部状态实现

具体动作：

1. 新建 `crates/channel-websocket`
2. 移动 `server.rs` 与精简后的协议编码解码逻辑
3. 定义 `ChannelHandler`
4. 在主 crate 中实现 `WebSocketGatewayHandler`

收益：

1. 以后可复用到桌面端 sidecar、独立服务或测试桩

### Phase 4: 启动装配层重写

目标：

1. `nova_gateway` 只做配置读取与依赖装配

具体动作：

1. 把 `start_server` 从 `gateway/mod.rs` 改为 `app/bootstrap.rs`
2. `bootstrap` 负责构造 `ConversationService` 与 `GatewayApplication`
3. `channel-websocket` 只接收 handler 实例

收益：

1. 启动流程边界清晰
2. 渠道库彻底独立

## 9. 测试案例

### 9.1 协议测试

1. `chat` 命令的序列化 / 反序列化
2. `sessions.*` 命令与响应的序列化 / 反序列化
3. 已删除消息类型不能再被错误接受
4. 未知消息统一落入 `Unknown` 并返回错误

### 9.2 应用服务测试

1. 正常 chat turn 能发送 `chat.start -> chat.progress -> chat.complete`
2. `chat.stop` 能取消正在运行的 turn
3. 有挂起交互时，输入先走 interaction resolution，而不是普通 chat
4. agent 切换只修改会话控制状态，不破坏历史消息

### 9.3 WebSocket 渠道测试

1. 连接成功后收到 `welcome`
2. 文本帧可正确解码为命令
3. 非法 JSON 返回错误，不导致 server 崩溃
4. 客户端断连后写任务能正常退出
5. 背压或下游关闭时不会无限堆积未发送消息

### 9.4 持久化测试

1. 创建 session 后 SQLite 中有记录
2. 追加消息后内存与数据库一致
3. 复制 session 时历史截断逻辑正确
4. 重启后 `load_all` 能恢复会话状态

## 10. 风险与待定项

### 10.1 风险

1. 当前 `protocol.rs` 过大，删除消息可能影响前端兼容
2. `workflow.rs` 属于原型逻辑，迁移时容易出现“保留了不该保留的假流程”
3. `SessionStore` 拆分后，如果并发边界没理清，可能引入一致性问题
4. 当前大量状态使用 `std::sync::RwLock`，未来如果锁持有范围扩大，需要重新审查异步上下文安全性

### 10.2 待确认项

1. “渠道相关的 WebSockets” 是否只指当前 OpenFlux 风格协议，还是后续还要兼容更多渠道协议
2. 前端是否接受一次协议收缩，删除当前未实现消息
3. `agents.switch` 是保留为会话级行为，还是未来升级为应用级路由行为
4. Workflow 是否保留；如果这块也打算简化，建议在 Phase 2 一并收缩

## 11. Review 建议

这是我对这块功能的直接建议，按优先级排序。

### 建议 1：先删协议，再拆库

如果不先把协议瘦身，后面只是把一堆历史包袱原样搬进新库，结果会更难改。

### 建议 2：不要把“WebSocket 库”设计成“新的大网关框架”

独立库要克制，只提供：

1. 连接管理
2. 编解码
3. handler 回调

不要把 Session、Agent、配置热更新再塞进去，否则只是换个目录继续耦合。

### 建议 3：`gateway` 这个名字可以废掉

从职责上看，拆完后主 crate 更适合叫：

1. `conversation`
2. `app`
3. `channel`

保留 `gateway` 这个命名容易继续把各种边缘逻辑塞回来。

### 建议 4：Workflow 如果不是近期主线，建议降级或删除

当前 `workflow.rs` 是明显原型实现：

1. 假数据候选方案
2. 与真实 Agent 执行脱节
3. 缺少持久化与恢复闭环

如果近期没有 owner 持续推进，这块建议不要进入新架构中心。

### 建议 5：尽量减少 `serde_json::Value`

协议里大量 `Value` 会吞掉类型边界，后续前后端联调、重构和排障都很痛苦。渠道协议一旦抽库，更应该尽快类型化。

## 12. 最终结论

这次重构的关键不在“把代码挪个目录”，而在重新定义边界：

1. WebSocket 是渠道层，不是业务层
2. Agent / Workflow / Session 是应用或领域层，不是网关附属物
3. 主协议必须只保留真实支持的能力，不保留空壳

推荐路线是：

1. 先收缩协议
2. 再抽离 conversation 与 agent 业务
3. 最后提取 `channel-websocket` 库

这样拆出来的新库才会干净，也更值得长期维护。
