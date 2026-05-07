# Provider 心跳与 Project 状态可视化设计

| 章节 | 说明 |
|-----------|------|
| 时间 | 创建：2026-05-04<br>最后更新：2026-05-04 |
| 项目现状 | 1. 后端当前只有 DeskApp 到 Gateway 的 WebSocket 连接态，没有 Provider 级别的存活探测与状态广播。<br>2. `deskapp/src/ui/titlebar.ts` 只消费 `gateway:status`，显示的是“网关连接状态”，不是“模型 Provider 可用状态”。<br>3. `SessionRuntimeSnapshot` 尚未暴露 `project_dir`，前端无法稳定获知当前会话绑定的 Project 目录。<br>4. `deskapp/src/ui/chat-view.ts` 已实现 `@` 项目路径选择器，但输入区没有独立的 Project 下拉入口，也没有显式展示当前 Project 目录。<br>5. `nova-agent` 已具备 `set_project_dir` / `get_project_dir` / `ProjectManagerTool`，并支持新会话继承同 Agent 最近一次 `project_dir`。 |
| 整体目标 | 1. 在后端增加 Provider 心跳机制，持续判断当前配置下可用 Provider 的健康状态。<br>2. 让 DeskApp 同时区分“网关连接状态”和“Provider 可用状态”，避免 WebSocket 已连通但 LLM 实际不可用时误报绿色。<br>3. 在聊天输入区增加 Project 下拉菜单，稳定展示当前会话 Project 目录，并提供围绕该目录的轻量操作入口。<br>4. 让当前 Project 目录通过共享协议进入前端状态层，避免桌面端依赖本地推断或重复调用。 |
| 非目标 | 1. 本次不实现 Provider 自动故障切换。<br>2. 本次不引入多项目工作区管理。<br>3. 本次不把 Project 下拉菜单扩展为完整目录切换器；目录切换仍以 `ProjectManager` 工具链为主。 |

## Plan 拆分

| 状态 | Plan | 说明 | 依赖 | 执行顺序 |
|------|------|------|------|----------|
| 待开始 | Plan 1: Provider 心跳模型与协议扩展 | 定义后端健康状态模型、事件载荷、前端共享 schema，以及 `SessionRuntimeSnapshot.projectDir` 扩展。 | 无 | 1 |
| 待开始 | Plan 2: Gateway 后端心跳调度与事件广播 | 在 Rust 后端实现 Provider 心跳任务、探测策略、状态缓存和广播触发点。 | Plan 1 | 2 |
| 待开始 | Plan 3: DeskApp 连接状态整合展示 | 在桌面端状态层区分网关状态与 Provider 状态，并更新标题栏/控制台文案。 | Plan 1, Plan 2 | 3 |
| 待开始 | Plan 4: 聊天输入区 Project 下拉菜单 | 在输入框旁新增 Project 菜单，展示当前目录并接入 runtime 数据、快捷动作和测试。 | Plan 1 | 4 |

## 现状分析

### 1. Provider 可用性没有独立观测面
- `nova-agent::provider::LlmClient` 只有 `stream()` 能力，没有 `health_check()` 或探针抽象。
- `deskapp/src-tauri` 中 `GatewayStateManager` 仅跟踪 sidecar 生命周期状态，无法表达“Gateway 进程活着，但 Provider 鉴权失败 / 上游不可达”的场景。
- `deskapp/src/gateway-client.ts` 的连接事件只描述 WebSocket 是否连通，也无法覆盖模型层异常。

### 2. Project 目录能力已存在但没有前端主展示位
- `ProjectManagerTool` 已经可以在会话内读写 `project_dir`。
- `ConversationService` 和相关测试已经保证 `project_dir` 会参与 prompt 构造、工具相对路径解析和会话继承。
- 但 `SessionRuntimeSnapshot` 当前只有模型、token、last turn 信息，缺少 `project_dir` 字段，DeskApp 无法在会话选中后直接知道当前目录。

### 3. `@` 路径选择器与“Project 显示入口”是两层能力
- `@` 选择器解决的是“在输入内容中插入某个相对路径”。
- 本次新增的 Project 下拉菜单解决的是“显式告诉用户当前会话的工作根目录是什么”。
- 两者不应互相替代：前者仍保留键盘流式输入效率，后者提供状态透明度与快捷操作。

## 关键设计决策

### 1. 区分两类连接状态
桌面端最终展示两个维度：

1. `gatewayConnectionStatus`
   - 来源：现有 WebSocket / sidecar 生命周期。
   - 含义：DeskApp 是否能连到 Gateway。
2. `providerHealthStatus`
   - 来源：后端心跳。
   - 含义：Gateway 当前用于推理的 Provider 是否可达、可鉴权、响应是否及时。

标题栏默认显示聚合结果，但内部状态必须保留两个维度，避免 UI 文案和诊断能力退化。

### 2. Provider 心跳只探测“当前配置下会实际使用的 Provider”
本次不对所有潜在 Provider 做全量扫描，而是探测以下集合：

- 全局默认 orchestration provider
- 全局默认 execution provider
- 当前 agent 配置显式绑定的 provider（若与全局不同）
- 会话级 override provider 仅在该会话活跃且 override 生效时纳入即时探测

原因：
- 降低不必要请求与噪音。
- 桌面端展示重点是“当前能不能聊”，不是“配置文件里所有 provider 都是否完美”。

### 3. 心跳结果使用统一健康模型
定义统一状态：

- `unknown`：尚未探测
- `checking`：正在探测
- `healthy`：探测成功
- `degraded`：可达但响应超慢或结果不完整
- `auth_failed`：鉴权失败 / token 无效
- `unreachable`：网络不可达、DNS、TLS、连接拒绝、超时
- `misconfigured`：缺少 base url / api key / 必要模型配置

同时记录：

- `checked_at`
- `latency_ms`
- `provider`
- `scope`（`orchestration` / `execution`）
- `message`

### 4. 心跳不复用真实对话链路
Provider 心跳不调用 `stream()`，单独使用轻量 HTTP 探针：

- OpenAI / OpenAI-compatible：优先 `GET /models`
- Anthropic：优先 `GET /v1/models`

原因：
- 避免消耗真实推理额度与消息上下文。
- 能明确区分“鉴权/网络失败”和“对话调用失败”。
- 保持探针逻辑简单，易于超时控制和告警去重。

### 5. 当前 Project 目录进入共享协议
`SessionRuntimeSnapshot` 新增：

- `project_dir: Option<String>`

如需保留来源语义，可预留：

- `project_dir_source: Option<String>`，候选值 `inherited` / `session_override` / `default`

本次 UI 只强依赖 `project_dir`，`source` 作为可选增强项，不阻塞落地。

### 6. Project 下拉菜单以“展示 + 快捷动作”为主
聊天输入区新增 `Project` 菜单触发器，默认展示当前目录 basename，展开后包含：

- 当前绝对路径（只读展示）
- `复制路径`
- `在系统文件管理器中打开`
- `刷新会话运行态`

本期不直接在菜单内实现“切换到别的目录”，因为：
- 目录切换已可由 Agent 工具链完成。
- 未经确认就把下拉菜单升级为目录浏览器，会把本次 UI 需求放大成新的工作流设计。

### 7. `@` 选择器继续以当前会话 `project_dir` 为根
Project 下拉菜单只负责状态显式化，不改变 `project_dir_list` 的根目录语义。`@` 选择器仍然：

- 相对当前会话 `project_dir`
- 限制越界访问
- 展示统一使用 `/` 分隔

这样可以避免两个入口对“项目根目录”的理解不一致。

## 事件与数据流

### 1. Provider 心跳主链路
1. Gateway 启动或配置变更后，构建 `ProviderHeartbeatManager`。
2. 管理器按固定间隔调度探测，并在状态变化时写入内存缓存。
3. 缓存变化后通过 Gateway 事件总线广播 `provider.health.updated`。
4. 新客户端连接时，除现有 `welcome` 外，再主动推送一次 `provider.health.snapshot`。
5. DeskApp 状态层收到后更新 Provider 健康缓存，并驱动标题栏/控制台刷新。

### 2. Project 目录展示链路
1. 会话创建、切换 Project、继承 Project 后，`SessionRuntimeSnapshot` 带出 `project_dir`。
2. `session.runtime.response` 与 `session.runtime.updated` 同步包含该字段。
3. DeskApp `AppState.sessionRuntimeStates` 继续作为单一数据源。
4. `ChatView` 在当前会话切换或 runtime 更新后刷新 Project 菜单文案。

## 风险与待定项

### 已知风险
- 兼容 Provider 的 `/models` 端点可能实现不一致，OpenAI-compatible 适配层需要允许“路径可配置或可降级”。
- 若心跳频率过高，某些上游会触发限流，必须把间隔、超时、退避统一抽成具名常量。
- 如果 DeskApp 只显示聚合状态、不保留细粒度错误，会丢失“网关已连接但 Provider 鉴权失败”的诊断价值。
- `SessionRuntimeSnapshot` 加字段后，需要同步 `nova-protocol` schema 导出和 DeskApp 生成类型，避免手写类型漂移。

### 待确认项
- Project 下拉菜单是否需要在后续版本支持“最近项目目录”切换。如果需要，建议单开后续设计，不混入本次。
- 标题栏是否只显示一个聚合灯，还是主灯 + tooltip / hover 明细。推荐本期先保留单灯、补 tooltip。
