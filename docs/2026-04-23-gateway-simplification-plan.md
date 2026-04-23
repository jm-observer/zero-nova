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
├─ crates/channel-websocket      # 纯传输层
├─ src/gateway                   # WebSocket 协议、handler、server
├─ src/app                       # 应用门面，组织用例
├─ src/conversation              # session/history/repository
├─ src/agent.rs                  # agent runtime
├─ src/agent_catalog.rs          # agent registry
└─ src/bin/nova_gateway.rs       # 启动入口
```

约束说明：

1. `conversation` 只保留会话状态与持久化，不再维护工作流
2. `app` 只编排用例，不做启发式意图识别
3. `gateway` 只解析协议并调用应用服务，不承担领域决策
4. Agent 切换通过显式 API 或显式消息完成，不再通过自然语言猜测

### 2. 建议删除的能力

#### 2.1 删除 `Workflow` 全链路

删除范围：

1. `src/conversation/workflow.rs`
2. `WorkflowCandidate`
3. `WorkflowStage`
4. `WorkflowState`
5. `WorkflowEngine`
6. `ConversationService::advance_workflow`
7. `ConversationService::start_workflow`
8. `TurnIntent::ContinueWorkflow`
9. `TurnIntent::StartNewTask`

删除原因：

1. 当前候选方案是硬编码，和 Agent 能力、工具系统、真实执行路径没有闭环
2. 工作流状态没有可靠持久化语义，重载会话后状态并不可信
3. 该模块增加了主链路分支复杂度，但没有提供稳定收益
4. 如果未来需要“方案对比/执行编排”，应该以单独能力重新设计，而不是挂在当前会话控制层里

#### 2.2 删除 `TurnRouter`

删除范围：

1. `src/conversation/control.rs` 中 `TurnIntent`
2. `src/conversation/control.rs` 中 `TurnRouter`
3. `ConversationService::start_turn` 中基于 `TurnRouter::classify` 的分流逻辑

替代方式：

1. 普通聊天：直接进入 `execute_agent_turn`
2. Agent 切换：仅保留显式入口
   - 方案 A：继续使用现有 `AgentsSwitch` WebSocket 消息
   - 方案 B：新增应用服务方法 `switch_agent(session_id, agent_id)`

推荐采用方案 B 的实现方式，但协议层仍可继续沿用现有 `AgentsSwitch` 消息。

原因：

1. 通过自然语言猜测“你是不是想切 Agent / 开工作流”会制造不稳定行为
2. 系统已经有明确的网关命令面，不需要再在会话文本里做一轮路由器判断
3. 删除后 `start_turn` 会退化成更稳定的单路径执行

### 3. 收缩 `ControlState`

建议把 `ControlState` 收缩为仅保留会话稳定状态：

```rust
pub struct ControlState {
    pub active_agent: String,
}
```

处理原则：

1. `pending_interaction` 一并删除
2. `workflow` 一并删除
3. `InteractionResolver`、`PendingInteraction`、`ResolutionIntent`、`RiskLevel` 等配套结构一并删除

原因：

1. `active_agent` 是真正稳定且持久化需要关心的状态
2. `pending_interaction` 只服务于“切换前确认”这类 UI 交互，不值得占用会话领域模型
3. 交互确认如果仍有需要，应放到更接近网关或前端协议的层面，而不是塞进 Session 核心模型

### 4. 会话服务简化方案

简化后的 `ConversationService` 应收敛为下面几类能力：

1. `start_turn`
   - 直接写入用户消息
   - 调用 `AgentRuntime::run_turn`
   - 写回 Agent 输出
2. `stop_turn`
   - 取消当前运行 token
3. `switch_agent`
   - 校验目标 agent 是否存在
   - 更新 `session.control.active_agent`
   - 可选发送 `AgentSwitched` 事件

简化后的伪代码：

```rust
pub async fn start_turn(...) -> Result<()> {
    self.execute_agent_turn(session_id, input, event_tx).await
}

pub async fn switch_agent(...) -> Result<()> {
    // validate agent
    // update session active_agent
    // emit AgentSwitched
}
```

收益：

1. 会话服务从“路由器 + 交互状态机 + workflow 引擎入口”收缩为真正的应用服务
2. 主执行路径更容易测试
3. 停止逻辑、消息持久化逻辑不再被无关分支干扰

### 5. Agent 切换改为显式操作

当前 `request_agent_switch` / `resolve_interaction` 是围绕“先生成待确认交互，再等用户回复”设计的。该流程建议直接删除，改成显式切换：

1. 网关收到 `AgentsSwitch`
2. `GatewayApplication` 调用 `ConversationService::switch_agent`
3. 直接更新 session 的 `active_agent`
4. 返回成功响应并广播 `AgentSwitched`

如果产品上确实需要“切换前确认”，建议由前端处理：

1. 前端点击切换
2. 前端自行弹确认框
3. 确认后再发送 `AgentsSwitch`

这比把确认状态塞进后端 Session 更简单，也更符合当前系统规模。

### 6. 清理边界耦合

本轮不做彻底分层重构，但至少要先完成两项收口：

#### 6.1 `conversation` 不再依赖 `gateway::protocol`

当前问题：

1. `Session::get_messages_dto` 返回 `gateway::protocol::MessageDTO`
2. `SessionStore::list_sorted` 返回 `gateway::protocol::Session`

建议调整：

1. `conversation` 只暴露领域对象或内部 DTO
2. `gateway::protocol` 转换放到 `app` 或 `gateway` 层

这样做的目的不是“为了抽象而抽象”，而是避免底层存储模块反向依赖最外层传输协议。

#### 6.2 `GatewayApplication` 逐步变成真正的用例门面

当前 `GatewayApplication` 已接近门面，但仍然偏向“帮 Gateway 透传数据”。优化方向：

1. 把“切换 agent”“读取 session message”“列出 session”等能力显式建模为应用服务接口
2. 尽量减少 `gateway` handler 直接知道内部 session 结构
3. 保持 `gateway` 只关心协议收发

### 7. 建议实施顺序

按风险从低到高，分三步做：

#### Step 1：删除 Workflow

变更点：

1. 删除 `src/conversation/workflow.rs`
2. 删除 `conversation/mod.rs` 对 `workflow` 的导出
3. 删除 `ConversationService` 中 workflow 相关分支
4. 删除 `control.rs` 中 workflow 相关字段和枚举分支

预期结果：

1. 主链路只剩“聊天 / 停止 / session 管理 / agent 切换”
2. 移除最大块的原型性复杂度

#### Step 2：删除交互确认状态机

变更点：

1. 删除 `PendingInteraction`、`InteractionResolver` 等结构
2. 删除 `resolve_interaction`
3. 删除 `request_agent_switch`
4. 新增或改造 `switch_agent`

预期结果：

1. `ControlState` 只剩 `active_agent`
2. Agent 切换路径改为显式命令，不再依赖会话里的隐式待确认状态

#### Step 3：收口模块边界

变更点：

1. 把 `gateway::protocol` DTO 转换移出 `conversation`
2. 减少 `GatewayApplication` 对内部结构的透传
3. 视情况把 `ControlState` 重命名为更贴近含义的 `SessionState` 或直接内联

预期结果：

1. `conversation`、`app`、`gateway` 的职责更明确
2. 后续再决定是否需要更进一步拆分，而不是先做大架构承诺

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
2. 再删 `TurnRouter + pending_interaction`
3. 最后收口 `conversation/app/gateway` 边界

这样做可以用最小成本把系统从“半成品多分支状态机”收回到“稳定的单主路径服务”，后续如果真要做更强的多 Agent 协作或任务编排，再以独立能力重新设计，而不是继续在当前壳子上叠加。
