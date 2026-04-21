# 会话控制层设计文档

> 版本: v1.0 | 日期: 2026-04-17

## 1. 背景与目标

现有 `skill-system-design` 解决的是 **prompt 按需加载**、**工具按 skill 过滤**、**历史按 skill 管理**。这套思路适合短任务和静态能力切换，但不足以覆盖以下场景：

1. **长流程任务**：如 TTS / 图片生成 / 其他技术方案的搜索、选型、部署、测试。
2. **多 agent 对话**：用户可通过自然语言随时点名某个 agent，例如“OpenClaw 在不在”。
3. **自然语言确认**：用户用“同意”“OK”“继续”等自然语言回复写操作、部署动作、方案选择，而非输入固定字符串。

这些场景的共同点不是“需要更多 skill”，而是需要一个比 skill 更高层的 **会话控制层**：

- 先判断用户这一轮输入属于什么控制意图。
- 再决定是否切换 agent、继续 workflow、解析挂起交互，或进入 skill 路由。
- 对高风险动作设置 runtime 级别的确认边界，而不是只靠 prompt 约束。

### 1.1 设计目标

引入 **Conversation Control Plane（会话控制层）**，实现：

1. **统一解释每轮输入**：先做控制意图判断，再做 skill / tool 执行。
2. **支持多 agent 自然切换**：用户可自然点名 agent，并切换当前活跃 agent。
3. **支持显式 workflow 状态**：长流程任务由 runtime 维护阶段状态，而非只靠 LLM 记忆。
4. **支持通用挂起交互**：统一处理确认、选型、补参、风险提示等等待用户回应的场景。
5. **保留 skill 系统**：skill 继续承担 prompt 模板与工具约束，但降级为执行层能力，而非顶层控制入口。

## 2. 设计结论

### 2.1 Skill 不再是顶层抽象

skill 仍然保留，但职责收缩为：

- 提供特定行为模板
- 提供工具白名单
- 提供局部 prompt 片段

skill **不再负责**：

- 决定当前对话是发起新任务、继续流程还是回应确认
- 决定当前用户是在和哪个 agent 说话
- 决定一条自然语言是否可以授权危险动作

### 2.2 新的顶层抽象

系统顶层新增四个核心对象：

| 对象 | 职责 |
|------|------|
| `TurnRouter` | 解释当前用户输入属于哪类会话事件 |
| `AgentContext` | 管理活跃 agent、被点名 agent、agent 切换 |
| `WorkflowRuntime` | 管理长流程任务的显式阶段状态 |
| `PendingInteraction` | 管理所有等待用户自然语言回应的挂起交互 |

skill 与工具执行处于下层：

```
用户输入
  ↓
TurnRouter
  ├─ PendingInteractionResolver
  ├─ AgentAddressResolver
  ├─ WorkflowRuntime
  └─ SkillRouter
       ↓
  SystemPromptBuilder + ToolRegistry
       ↓
    AgentRuntime
```

## 3. 核心概念

### 3.1 TurnRouter

`TurnRouter` 是每轮输入的第一入口。它不直接回答问题，而是判断这条输入属于哪一类事件。

建议事件类型：

```rust
pub enum TurnIntent {
    ResolvePendingInteraction,
    AddressAgent,
    ContinueWorkflow,
    StartNewTask,
    FallbackChat,
}
```

### 3.2 AgentContext

`AgentContext` 负责多 agent 会话控制。它不关心某个 agent 的具体 prompt，只关心当前由谁处理。

```rust
pub struct AgentContext {
    pub active_agent: String,
    pub addressed_agent: Option<String>,
    pub available_agents: Vec<AgentDescriptor>,
}

pub struct AgentDescriptor {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub aliases: Vec<String>,
}
```

需要支持的能力：

- 判断“OpenClaw 在不在”是否在点名 agent
- 判断“让 OpenClaw 来处理”是否在切换 agent
- 判断“继续刚才那个 agent”是否沿用上下文

### 3.3 WorkflowRuntime

`WorkflowRuntime` 负责长流程任务，不再把“流程状态”隐含在 LLM 历史里。

```rust
pub struct WorkflowState {
    pub workflow_type: WorkflowType,
    pub topic: Option<String>,
    pub stage: WorkflowStage,
    pub constraints: serde_json::Value,
    pub candidates: Vec<WorkflowCandidate>,
    pub selected_candidate: Option<String>,
}

pub enum WorkflowType {
    SolutionWorkflow,
    CodingWorkflow,
}

pub enum WorkflowStage {
    Discover,
    Compare,
    AwaitSelection,
    AwaitExecutionConfirmation,
    Executing,
    AwaitTestInput,
    Testing,
    Completed,
}
```

这里的关键点是：

- `TTS`、`文生图`、`OCR` 这类主题不是 skill 名，而是 `topic`
- workflow 是通用框架，主题是 workflow 的参数
- runtime 知道当前确切阶段，而不是靠 prompt 暗示

### 3.4 PendingInteraction

`PendingInteraction` 是对“自然语言审批”的上位抽象。它统一描述“系统暂停并等待用户回应”的交互。

```rust
pub struct PendingInteraction {
    pub id: String,
    pub kind: InteractionKind,
    pub subject: String,
    pub prompt: String,
    pub options: Vec<InteractionOption>,
    pub risk_level: RiskLevel,
    pub metadata: serde_json::Value,
}

pub enum InteractionKind {
    ApproveAction,
    SelectOption,
    FillInput,
    ConfirmSwitch,
    RetryOrRollback,
}

pub struct InteractionOption {
    pub id: String,
    pub label: String,
    pub aliases: Vec<String>,
}

pub enum RiskLevel {
    Low,
    Medium,
    High,
}
```

它覆盖的场景包括：

- “是否写入配置并重启服务？”
- “你选哪个方案？”
- “使用默认端口还是自定义端口？”
- “是否切换到 OpenClaw agent？”

### 3.5 InteractionResolver

`InteractionResolver` 负责把用户自然语言回应转成结构化决议。

```rust
pub struct ResolutionResult {
    pub resolved: bool,
    pub intent: ResolutionIntent,
    pub confidence: f32,
    pub selected_option_id: Option<String>,
    pub free_text: Option<String>,
    pub requires_clarification: bool,
}

pub enum ResolutionIntent {
    Approve,
    Reject,
    Select,
    ProvideInput,
    Cancel,
    Unknown,
}
```

这里必须强调：

- LLM 只负责 **语义归一化**
- runtime 负责 **状态匹配与动作授权**

也就是说，即便模型判断“OK，继续”表示同意，runtime 仍需检查当前是否真的存在一个待批准动作。

## 4. 事件优先级

每轮用户输入必须按固定优先级解释，否则会产生歧义。

推荐顺序：

1. **挂起交互解析**：当前是否存在 `PendingInteraction`
2. **agent 点名 / 切换**：当前输入是否在点名 agent
3. **workflow 延续**：当前是否存在 `WorkflowState`，且输入显然在继续该流程
4. **新任务路由**：否则视为新任务，再交给 skill / workflow 路由
5. **兜底聊天**：无法可靠匹配时进入普通对话

伪代码：

```rust
fn route_turn(session: &SessionState, user_message: &str) -> TurnIntent {
    if session.pending_interaction.is_some() {
        return TurnIntent::ResolvePendingInteraction;
    }

    if looks_like_agent_addressing(user_message, &session.agent_context) {
        return TurnIntent::AddressAgent;
    }

    if session.workflow.is_some() && looks_like_workflow_reply(user_message) {
        return TurnIntent::ContinueWorkflow;
    }

    if looks_like_new_task(user_message) {
        return TurnIntent::StartNewTask;
    }

    TurnIntent::FallbackChat
}
```

这个优先级是必要的，因为：

- “继续”在有挂起确认时，通常应先解释为确认回复，而不是新任务。
- “OpenClaw 在不在”应先解释为点名 agent，而不是普通问答。

## 5. 长流程任务设计

### 5.1 通用 workflow，而不是特定领域 skill

对于 TTS、文生图、OCR、代码方案这类需求，更合适的不是设计多个垂直 skill，而是一个通用 workflow：

- `solution-workflow`

它的职责不是回答某个具体领域，而是执行统一流程：

1. 收集目标与约束
2. 搜索候选方案
3. 输出对比
4. 等待用户选型
5. 等待部署/执行确认
6. 执行部署或安装
7. 引导测试

### 5.2 示例流程

以 TTS 为例：

```text
用户: 我想要一个 TTS 方案
  → StartNewTask
  → 进入 SolutionWorkflow(topic = "tts", stage = Discover)

系统: 你更关注中文效果、易部署，还是推理速度？本地机器是否有 GPU？
  → 挂起 FillInput

用户: 本地部署，有 GPU，优先中文效果
  → ResolvePendingInteraction
  → 更新 constraints
  → 继续 Discover

系统: 我找到 3 个候选：A / B / C，对比如下...
  → stage = AwaitSelection
  → 挂起 SelectOption

用户: 第二个
  → ResolvePendingInteraction
  → selected_candidate = B
  → stage = AwaitExecutionConfirmation

系统: Fish Speech 需要下载约 12GB 模型，并启动 Docker 容器，占用 7860 端口。是否继续部署？
  → 挂起 ApproveAction(risk = High)

用户: 可以，继续
  → ResolvePendingInteraction
  → runtime 执行部署
  → stage = Executing

系统: 已完成部署。是否现在做一次测试？
  → stage = AwaitTestInput
```

这个流程同样适用于：

- 图片生成方案搜索与部署
- 向量数据库方案搜索与部署
- 代码框架选型与示例项目初始化

## 6. 多 Agent 设计

### 6.1 设计原则

多 agent 的重点不是“每个 agent 拥有不同 skill”，而是：

- 用户能自然点名 agent
- 系统能识别当前发言对象
- agent 切换不会破坏挂起交互和 workflow 状态

### 6.2 建议规则

1. 每个 agent 必须有稳定 `id`
2. 每个 agent 可配置多个 `aliases`
3. `AgentAddressResolver` 支持以下意图：
   - presence check：`OpenClaw 在不在`
   - handoff：`让 OpenClaw 来处理`
   - direct ask：`OpenClaw，帮我看看这个`
4. 切换 agent 时，默认保留会话级 `PendingInteraction` 和 `WorkflowState`，但允许 agent 级上下文切换

### 6.3 为什么不把 agent 点名做成 skill

因为“OpenClaw 在不在”并不是任务内容，而是 **会话控制意图**。如果把它交给 skill 分类，会造成：

- 被当成普通问答
- 被误判成某个 workflow 入口
- 与确认语句冲突

因此它必须在 skill 之前解析。

## 7. 自然语言确认与挂起交互

### 7.1 不把审批单独特化

“自然语言审批”只是挂起交互的一种，不应单独设计一个审批系统后再为其他场景打补丁。

更稳定的抽象是：

- 系统先声明“当前等待什么回应”
- 再由解释器把用户自然语言映射为结构化结果

### 7.2 为什么不能只靠 LLM 自由理解

如果没有 `PendingInteraction`，以下输入都可能歧义：

- “继续”
- “OK”
- “第二个”
- “默认就行”

这些词本身没有明确对象，只有在挂起状态存在时才有含义。

因此实现上必须满足两个条件：

1. **先有挂起状态**
2. **再做自然语言解析**

### 7.3 风险边界

对高风险动作，建议增加额外约束：

| 风险等级 | 示例 | 建议 |
|------|------|------|
| Low | 方案选择、普通补参 | 可接受自然语言确认 |
| Medium | 拉取镜像、下载大模型 | 要求明确正向确认 |
| High | 覆盖写文件、删除资源、重启服务 | 除自然语言确认外，建议附带摘要回显或二次确认 |

这里的重点是：

- 高风险动作可以继续支持自然语言确认
- 但 runtime 应要求更高的置信度或更严格的确认策略

## 8. 与 Skill 系统的关系

### 8.1 保留 skill，但调整职责

skill 系统仍然有价值，适合处理：

- prompt 模块化
- 工具按需暴露
- 用户扩展自定义行为模板

但 skill 的位置变为：

- `Conversation Control Plane` 之下的执行层

### 8.2 推荐关系

| 层级 | 职责 |
|------|------|
| 会话控制层 | 解释输入、切换 agent、续接 workflow、解析挂起交互 |
| workflow 层 | 管理长流程任务状态与阶段迁移 |
| skill 层 | 提供 prompt 模板、工具约束、局部行为规则 |
| tool 层 | 执行真实副作用与外部调用 |

### 8.3 对原 `skill-system-design` 的修订建议

如果保留原文档，建议做以下调整：

1. 把 `SkillRouter` 从顶层入口下沉为执行层路由器
2. 把 `sticky` 改造成更明确的 `WorkflowState + PendingInteraction` 机制
3. 把“自然语言确认”从 prompt 约束提升为 runtime 结构化能力
4. 增加 `AgentContext` 与 `TurnRouter`，统一处理多 agent 场景

## 9. Session 数据结构建议

```rust
pub struct SessionState {
    pub agent_context: AgentContext,
    pub workflow: Option<WorkflowState>,
    pub pending_interaction: Option<PendingInteraction>,
    pub active_skill: Option<String>,
    pub history: Vec<Message>,
}
```

说明：

- `active_skill` 仍然保留，但不再是唯一核心状态
- `workflow` 描述当前长流程
- `pending_interaction` 描述当前等待用户回应的局部状态
- `agent_context` 描述当前由谁处理

## 10. 实施建议

建议分三步实施，避免一次性重构过大。

### 10.1 Phase 1：控制入口改造

目标：引入 `TurnRouter` 和 `PendingInteraction`，先解决输入优先级与自然语言确认问题。

范围：

- 新增 `TurnRouter`
- 新增 `PendingInteraction`
- 给 `Session` 增加 `pending_interaction`
- 将高风险动作改为“先挂起，再等待回应”

### 10.2 Phase 2：Workflow 显式化

目标：把长流程任务从 prompt 记忆转为 runtime 状态。

范围：

- 新增 `WorkflowRuntime`
- 定义 `WorkflowState` 与 `WorkflowStage`
- 先落地一个 `solution-workflow`

### 10.3 Phase 3：多 Agent 会话控制

目标：支持用户自然点名 agent 和 agent 切换。

范围：

- 新增 `AgentContext`
- 新增 `AgentAddressResolver`
- 在 `TurnRouter` 中加入 agent addressing 分支

## 11. 不做的事情

当前版本建议明确不做：

- **不做任意 workflow DSL**：先用固定 Rust 数据结构表达流程状态
- **不做多 workflow 并行激活**：同一 session 同时只维护一个主 workflow
- **不做完全自治的高风险写操作**：所有高风险动作都必须经过挂起交互
- **不做 agent 间自动协商**：先只支持用户显式点名与切换
- **不做纯 prompt 驱动的状态迁移**：关键状态必须落在 runtime 中

## 12. 结论

这套设计的关键变化不是“新增更多 skill”，而是把系统顶层从 **skill 路由** 提升为 **会话控制**。

最终结构应当是：

- `TurnRouter` 负责解释每轮输入
- `AgentContext` 负责多 agent 会话控制
- `WorkflowRuntime` 负责长流程状态
- `PendingInteraction` 负责等待用户自然语言决策
- `SkillRouter` 退到执行层，负责 prompt 与工具选择

这个分层能同时覆盖：

- TTS / 文生图 / 其他方案的搜索、部署、测试
- 多 agent 自然点名与切换
- 自然语言确认、选型、补参、重试、回滚
