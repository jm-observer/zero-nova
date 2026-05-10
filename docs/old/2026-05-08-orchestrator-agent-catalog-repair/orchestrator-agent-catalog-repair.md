# Orchestrator Agent Catalog Repair

| 章节 | 内容 |
|---|---|
| 时间 | 创建：2026-05-08；最后更新：2026-05-08 |
| 项目现状 | 当前多 Agent 编排已经复用 `[[gateway.agents]]` 作为子 Agent 来源，但运行协议仍要求 LLM 在 `OrchestrationPlan.AgentRequest.subagent_type` 和 `Agent` 工具输入中显式给出执行 Agent ID。与此同时，系统提示词没有把可用 agent 列表和各自职责稳定注入给 orchestrator。结果是模型既要“猜”有哪些 agent，又要输出一个自由字符串字段，容易出现无效 agent、大小写漂移、虚构 reviewer/coder persona 等问题。 |
| 整体目标 | 将子 Agent 选择从“模型自由填写 `subagent_type`”收敛为“运行时基于显式 agent catalog 决策”。编排 prompt 必须稳定暴露可用 agent 列表、职责、默认项和选择规则；编排协议不再要求模型发明 `subagent_type`；运行时只接受来自受控 catalog 的 agent 选择，避免 prompt 与协议互相打架。 |
| Plan 拆分 | 1. **Plan 1: Prompt 与协议收敛** - 为 orchestrator 注入 agent catalog，并从计划协议中移除自由 `subagent_type` 输入。依赖：无。状态：待开始。<br>2. **Plan 2: 运行时选路与兼容迁移** - 让 `OrchestratorEngine`/`AgentTool` 依据 catalog 解析和执行选中的 agent，同时兼容旧字段输入。依赖：Plan 1。状态：待开始。<br>3. **Plan 3: 测试与观测补齐** - 补充解析、prompt、事件和回退路径测试，确保迁移期可观测。依赖：Plan 1、Plan 2。状态：待开始。 |
| 风险与待定项 | 1. 现有事件与前端若仍展示 `subagentType`，需要确认是保留该字段作为“已解析的执行 agent”还是同步重命名。<br>2. 旧 prompt、旧 fixture、旧文档均默认 `subagent_type` 可由模型填写，迁移阶段需要兼容反序列化。<br>3. 如果后续需要 reviewer/researcher 等专门 agent，catalog 需要能表达“用途/标签”而不把 prompt 中的自由文本重新引回协议层。 |

## 项目现状

当前实现存在三处结构性不一致：

1. `PromptConfig` / `SystemPromptBuilder` 没有 agent catalog section。系统提示词可以注入项目上下文、developer prompt、environment、workflow，但不会注入 `gateway.agents` 列表，因此 orchestrator 不知道当前会话到底注册了哪些 agent。
2. `OrchestrationPlan.AgentRequest` 仍把 `subagent_type: String` 暴露给模型填写，并在缺失时默认补成 `"nova"`。这让协议把“执行 agent 选择”错误地下沉到 LLM 的自由文本输出。
3. `AgentTool` 的输入 schema 和运行时实现都依赖 `subagent_type` 解析已注册 agent，未知值再 fallback 到 primary agent。由于 prompt 里没有 catalog，这个 fallback 经常被动触发。

这套设计在 2026-05-07 的“先跑通 developer/nova 双 agent 路由”阶段是可接受的，但继续沿用会带来以下问题：

1. prompt 和协议边界不清。模型既要拆任务，又要记住 agent ID，还要猜哪些 ID 合法。
2. reviewer/coder/researcher 之类自然语言角色名会和真正注册的 agent ID 混用。
3. 兼容回退掩盖真实问题。模型输出无效 `subagent_type` 时任务还能跑，但落到错误 agent，调试成本高。
4. 新增 agent 后没有自动暴露给 orchestrator，必须靠 prompt 手写或模型记忆，不可维护。

## 整体目标

本次修复不只是“再补一段提示词”，而是明确三条边界：

1. `gateway.agents` 是子 Agent 的唯一事实来源。
2. orchestrator prompt 必须显式展示 agent catalog，让模型只在受控列表内做选择。
3. 计划协议和执行工具不再要求模型提供自由 `subagent_type` 字符串；运行时只接收已解析的 agent 选择结果。

目标状态如下：

```text
AppConfig.gateway.agents
    ↓
Agent catalog builder
    ↓
System prompt / reviewer prompt / plan schema hint 中统一暴露
    ↓
LLM 只输出受控 agent selection
    ↓
OrchestratorEngine / AgentTool 按 catalog 执行
```

## Plan 拆分

### Plan 1: Prompt 与协议收敛

- 在 prompt 组装链路中新增 “Available Agents” section。
- section 内容来自 `gateway.agents`，至少包含：`id`、`display_name`、`description`、是否默认、可用场景说明。
- 更新 orchestrator 相关提示词，明确：
  - 只能从 catalog 中选择 agent
  - 不允许编造新 agent 名称
  - 无法判断时使用默认 agent
- 调整计划协议，移除或废弃由模型直接填写的 `subagent_type`。

### Plan 2: 运行时选路与兼容迁移

- 引入受控的 agent selection 字段或结构，而不是任意字符串。
- `OrchestratorEngine` 在执行前统一解析所选 agent，并把最终 agent ID 传给 `AgentTool`。
- `AgentTool` 内部保留兼容层：旧输入若仍带 `subagent_type`，先映射到 catalog，再记录 warning。
- review 阶段不再硬编码 `"Reviewer"` 这类未注册值，而是使用 catalog 中的默认/指定 agent。

### Plan 3: 测试与观测补齐

- 补充 prompt 测试，验证系统提示词确实包含 agent catalog。
- 补充 plan 解析测试，验证非法 agent 选择会被拒绝或规范回退。
- 补充运行时日志和事件测试，验证外发事件中的 agent 标识与实际执行 agent 一致。
- 补充迁移测试，验证旧 `subagent_type` 输入仍可被兼容解析，但会产生明确 warning。

## 风险与待定项

1. 如果直接删除 `subagent_type`，会影响现有 schema fixture、事件展示和前端调试 UI；因此建议先进入“协议新增 + 旧字段兼容”的迁移期。
2. agent catalog 若只暴露 `id` 与 `description`，模型仍可能难以分辨使用场景；需要为 orchestrator 单独生成简洁用途说明，而不是原样拼接长 prompt。
3. 运行时需要决定“无效 agent 选择”是 hard error 还是自动回退。建议对新协议使用 hard error，对旧兼容字段保留 warning + fallback。
