# Plan 1: Prompt 与协议收敛

| 章节 | 内容 |
|---|---|
| Plan 编号与标题 | Plan 1: Prompt 与协议收敛 |
| 前置依赖 | 无 |
| 本次目标 | 让 orchestrator 在生成计划前稳定看到可用 agent catalog，并把“只能从 catalog 中选 agent”的约束写入提示词和协议说明；同时停止要求模型自由填写 `subagent_type`。 |
| 涉及文件 | `crates/nova-agent/src/prompt.rs`、`crates/nova-agent/src/app/bootstrap.rs`、`crates/nova-agent/src/tool/builtin/orchestrate_task.rs`、`crates/nova-agent/src/orchestrator/planner.rs`、编排相关 prompt/skill 文档（如存在） |

## 详细设计

### 1.1 新增 agent catalog prompt section

当前 `SystemPromptBuilder::from_config()` 的 section 顺序是：

`Base → BehaviorGuards → Skill → DeveloperProjectPrompt → ProjectContext → Environment → Workflow`

Plan 1 新增 `AvailableAgents` section，建议插入到：

`Base → BehaviorGuards → Skill → AvailableAgents → DeveloperProjectPrompt → ProjectContext → Environment → Workflow`

原因：

1. agent catalog 属于运行时能力边界，优先级高于项目上下文，但低于身份与行为约束。
2. orchestrator 决策 agent 分配时，需要先看到系统允许的 agent，再读取项目细节。
3. 该 section 不应放进 `Environment`，否则语义过宽，后续难以单独测试和复用。

### 1.2 PromptConfig 增加 agent catalog 输入

为避免在 `SystemPromptBuilder` 内直接依赖 `AppConfig`，建议在 `PromptConfig` 中加入受控字段，例如：

```rust
pub struct AgentCatalogEntry {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub is_default: bool,
}
```

```rust
pub struct PromptConfig {
    pub available_agents: Vec<AgentCatalogEntry>,
}
```

该字段由 bootstrap、ConversationService、AgentTool 的子 agent 路径统一填充，避免 prompt builder 直接读取全局配置。

### 1.3 agent catalog 的 prompt 文本格式

catalog 文本应面向模型决策，而不是原样暴露底层结构。建议格式：

```text
Available execution agents:
- nova (default): general-purpose agent for coordination, exploration, and fallback tasks
- developer: implementation-focused agent for code changes, debugging, and tests

Rules:
- You must select only from the listed agent ids.
- Do not invent new agent ids such as Reviewer, Coder, or Researcher unless they appear above.
- If no specialist clearly fits, use the default agent.
```

格式要求：

1. 固定顺序与 `gateway.agents` 顺序一致。
2. 明确默认 agent。
3. 附带禁止编造 agent 的硬约束。
4. 不拼接 prompt file 正文，避免把 catalog section膨胀成 prompt 镜像。

### 1.4 协议说明从 `subagent_type` 转为 catalog selection

`OrchestrationPlan.AgentRequest` 当前形态：

```rust
pub struct AgentRequest {
    pub agent_id: String,
    pub subagent_type: String,
    pub description: String,
    pub prompt: String,
}
```

Plan 1 的协议方向：

1. 将 `subagent_type` 标记为 deprecated。
2. 新增语义更清晰的字段，例如 `agent` 或 `executor`, 值必须来自 catalog 中的 agent id。
3. 计划文档、tool schema、prompt 示例统一改用新字段。

示例：

```rust
pub struct AgentRequest {
    pub agent_id: String,
    pub executor: String,
    pub description: String,
    pub prompt: String,
}
```

这里的 `agent_id` 继续表示编排实例 ID；`executor` 表示实际执行的注册 agent。

### 1.5 OrchestrateTask prompt / skill 文档更新

只在 schema 改字段还不够，必须同步更新 orchestrator 使用的提示词或 skill 指令：

1. 明确“实例标识”和“执行 agent”是两回事。
2. 给出正例和反例。
3. 明确禁止输出 catalog 外的值。

示例约束：

```text
For each planned agent:
- `agentId` is a unique plan-local instance id such as `agent-1`.
- `executor` must be one of the available execution agent ids shown above.
- Never use free-form role labels like `Reviewer` or `Coder` unless they are actual execution agent ids.
```

## 测试案例

1. `SystemPromptBuilder::from_config()` 在提供 `available_agents` 时，会生成 `Available Agents` section，且顺序与配置一致。
2. 当 `available_agents` 为空时，不注入该 section，避免污染非 orchestrator prompt。
3. orchestrator prompt 示例和 schema 文本不再出现“请填写 `subagent_type`”之类说明。
4. 新协议示例 JSON 只使用新字段，旧字段仅在兼容说明中出现。
