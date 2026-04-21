# configs/ 目录说明

本目录存放会话控制层（Conversation Control Plane）的提示词模板、测试样例与配置示例。

## 目录结构

```
configs/
├── README.md                              # 本文件
├── prompts/                               # 提示词模板
│   ├── agent-nova.md                      # Nova（默认助手）的 system prompt
│   ├── agent-openclaw.md                  # OpenClaw（架构师）的 system prompt
│   ├── turn-router.md                     # TurnRouter 意图分类提示词（LLM 模式）
│   ├── interaction-resolver.md            # InteractionResolver 自然语言解析提示词
│   └── workflow-stages.md                 # Workflow 各阶段的动态注入 prompt 片段
└── examples/                              # 配置与测试示例
    ├── agents.toml                        # Agent 注册配置示例（含字段说明）
    ├── interaction-samples.json           # PendingInteraction 测试样例集
    └── workflow-e2e.json                  # SolutionWorkflow 端到端测试场景
```

## 各文件用途

### prompts/

| 文件 | 加载方式 | 说明 |
|------|---------|------|
| `agent-nova.md` | `SystemPromptBuilder::with_agent()` | Nova 的角色定义与行为准则 |
| `agent-openclaw.md` | `SystemPromptBuilder::with_agent()` | OpenClaw 的角色定义与行为准则 |
| `turn-router.md` | `gateway.router.use_llm_classification = true` 时使用 | TurnRouter 在 LLM 分类模式下的 system prompt |
| `interaction-resolver.md` | 后续 LLM 辅助解析模式时使用 | InteractionResolver 的 system prompt |
| `workflow-stages.md` | `SystemPromptBuilder` 根据 `WorkflowStage` 动态注入 | 每个 Workflow 阶段的行为指导片段 |

### examples/

| 文件 | 用途 |
|------|------|
| `agents.toml` | 展示 `config.toml` 中 `[[gateway.agents]]` 的完整配置写法，可直接复制到 `config.toml` |
| `interaction-samples.json` | 提供 `SelectOption`、`ApproveAction`（高/低风险）、`FillInput` 四类挂起交互的测试用例 |
| `workflow-e2e.json` | TTS 方案搜索部署的完整 7 步端到端测试场景，含边界用例 |

## 占位符说明

提示词模板中使用 `{{variable}}` 格式的占位符，由 runtime 在构建 system prompt 时替换：

| 占位符 | 注入来源 | 示例值 |
|--------|---------|--------|
| `{{workflow_stage}}` | `WorkflowState.stage` | `AwaitSelection` |
| `{{pending_interaction}}` | `ControlState.pending_interaction` | `SelectOption: tts_solution_selection` |
| `{{topic}}` | `WorkflowState.topic` | `tts` |
| `{{constraints}}` | `WorkflowState.constraints` | `{"gpu": true, "os": "ubuntu"}` |
| `{{candidates}}` | `WorkflowState.candidates` | 候选方案列表 |
| `{{selected_candidate}}` | `WorkflowState.selected_candidate` | `fish-speech` |
| `{{active_agent}}` | `ControlState.active_agent` | `openclaw` |
| `{{available_agents}}` | `AgentRegistry.list()` | Agent 描述列表 |
| `{{interaction_kind}}` | `PendingInteraction.kind` | `Select` |
| `{{subject}}` | `PendingInteraction.subject` | `tts_solution_selection` |
| `{{prompt}}` | `PendingInteraction.prompt` | 挂起交互的提示文本 |
| `{{risk_level}}` | `PendingInteraction.risk_level` | `High` |
| `{{options}}` | `PendingInteraction.options` | 选项列表 |

## 如何添加新 Agent

1. 在 `configs/prompts/` 下创建 `agent-{id}.md`，参照 `agent-nova.md` 的结构编写
2. 在 `config.toml` 中添加 `[[gateway.agents]]` 条目（参照 `examples/agents.toml`）
3. 重启 `nova-gateway`，新 Agent 会自动注册到 `AgentRegistry`
