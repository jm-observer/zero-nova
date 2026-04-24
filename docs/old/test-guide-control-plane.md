# Zero-Nova 会话控制层测试指南

本文档旨在为开发人员、测试人员及最终用户提供针对 **Conversation Control Plane (会话控制层)** 功能的详细配置参考、测试案例与验证方法。

---

## 1. 测试准备与环境配置

*   **后端服务**：确保 `nova-gateway` 已启动。
*   **前端连接**：使用 OpenFlux 客户端连接到 `ws://127.0.0.1:9090`。
*   **配置加载**：控制层的行为主要由 `config.toml` 和 `src/gateway/mod.rs` 中的注册信息决定。

---

## 2. 配置参考手册

### 2.1 Agent & Skill 默认配置示例

在 `config.toml` 或系统内部逻辑中，Agent 的详细配置如下：

| 属性 | 配置示例 (OpenClaw) | 说明 |
| :--- | :--- | :--- |
| **ID** | `openclaw` | 内部唯一标识符，用于协议通信 |
| **显示名称** | `OpenClaw` | 前端界面显示的名称 |
| **描述** | `擅长编写代码与系统部署的资深架构师` | Agent 的职能描述 |
| **别名 (Aliases)** | `["oc", "claw", "架构师"]` | 支持通过这些词汇进行自然语言点名 |
| **模型依赖** | `gpt-4o` 或 `claude-3-5-sonnet` | 建议使用推理能力强的模型 |
| **温度值 (Temp)** | `0.1` | 保持回复的确定性，尤其是执行任务时 |
| **超时策略** | `tool_timeout = 120s` | 耗时任务（如部署）的单次工具调用阈值 |
| **重试策略** | `max_iterations = 10` | 单个 Turn 内允许的最大思维循环次数 |

### 2.2 提示词模板 (System Prompts)

#### 开发者层级 (固定 Prompt)
```markdown
# Role: {{agent_name}}
# Context: You are part of the Zero-Nova Control Plane.
# Constraints:
1. Always check if there is a pending workflow before starting a new task.
2. For high-risk actions (file write, service restart), you MUST produce a tool call that triggers a 'PendingInteraction'.
3. Current Workflow Stage: {{workflow_stage}}
```

#### 变量占位符说明
*   `{{agent_name}}`: 当前活跃 Agent 的显示名称。
*   `{{workflow_stage}}`: `WorkflowEngine` 注入的当前状态（如 `Discover`, `AwaitSelection`）。

---

## 3. TurnRouter 意图分类测试

`TurnRouter` 是每轮输入的第一入口，负责判断用户输入属于哪类会话事件。意图分类的正确性直接决定控制层的核心行为。

### 3.1 意图分类优先级

系统按以下固定优先级解释每轮输入：

1. `ResolvePendingInteraction` — 当前是否存在挂起交互
2. `AddressAgent` — 当前输入是否在点名/切换 agent
3. `ContinueWorkflow` — 当前是否存在活跃 workflow 且输入在继续该流程
4. `StartNewTask` — 视为新任务，交给 skill / workflow 路由
5. `FallbackChat` — 无法匹配时进入普通对话

### 3.2 优先级验证用例

以下用例用于验证同一输入在不同会话状态下被正确归类：

| 用户输入 | 会话状态 | 期望 TurnIntent | 说明 |
| :--- | :--- | :--- | :--- |
| `继续` | 有 PendingInteraction | `ResolvePendingInteraction` | 优先解析为挂起交互回应 |
| `继续` | 无挂起，有活跃 Workflow | `ContinueWorkflow` | 降级为 workflow 延续 |
| `继续` | 无挂起，无 Workflow | `FallbackChat` | 无上下文可匹配，进入普通对话 |
| `OK` | 有 PendingInteraction | `ResolvePendingInteraction` | 模糊确认词优先匹配挂起交互 |
| `OK` | 无挂起 | `FallbackChat` | 无挂起状态时不应误触发动作 |
| `OpenClaw 在不在` | 有 PendingInteraction | `ResolvePendingInteraction` | 挂起交互优先级高于 agent 点名 |
| `OpenClaw 在不在` | 无挂起 | `AddressAgent` | 识别为 agent presence check |
| `我想部署一个 TTS` | 无挂起，无 Workflow | `StartNewTask` | 识别为新任务入口 |
| `今天天气怎么样` | 无挂起，无 Workflow | `FallbackChat` | 非任务类输入进入普通对话 |

### 3.3 日志验证

每次意图分类应在 `nova-gateway` 日志中输出以下格式：

```
[TurnRouter] input="继续" intent=ResolvePendingInteraction confidence=0.95
```

测试时应检查日志中 `intent` 字段与期望值一致。

---

## 4. Agent 点名与切换测试

### 4.1 Agent Addressing 意图类型

| 意图类型 | 用户输入示例 | 期望行为 |
| :--- | :--- | :--- |
| **Presence Check** | `OpenClaw 在不在` | 回复在线状态，**不切换** active_agent |
| **Handoff** | `让 OpenClaw 来处理` | 切换 active_agent 为 `openclaw` |
| **Direct Ask** | `OpenClaw，帮我看看这个错误` | 切换 active_agent 并立即将消息交给 openclaw 处理 |

### 4.2 别名识别测试

| 用户输入 | 匹配别名 | 期望识别 Agent |
| :--- | :--- | :--- |
| `架构师来一下` | `架构师` | `openclaw` |
| `oc 帮我看看` | `oc` | `openclaw` |
| `让 claw 处理这个问题` | `claw` | `openclaw` |

### 4.3 切换中状态保留测试

Agent 切换时，会话级状态不应丢失：

| 场景 | 操作步骤 | 验证要点 |
| :--- | :--- | :--- |
| Workflow 进行中切换 | 1. 进入 SolutionWorkflow (stage=Compare) 2. 用户说"让 OpenClaw 来处理" | WorkflowState 保留，stage 仍为 Compare |
| 有挂起交互时切换 | 1. 产生 PendingInteraction (SelectOption) 2. 用户说"让 OpenClaw 来" | PendingInteraction 保留，切换后仍可解析 |
| 切换后回切 | 1. 从 AgentA 切换到 AgentB 2. 用户说"切回刚才那个" | 恢复 AgentA 为 active_agent |

---

## 5. 典型输入输出样例与挂起交互

### 5.1 方案搜索与选择 (SelectOption)

*   **用户输入**：`我想部署一个本地运行的 Llama3 方案。`
*   **系统处理**：进入 `SolutionWorkflow` -> `Discover` 阶段。
*   **系统输出**：
    > "为您找到以下两种方案：
    > 1. **Ollama**: 简单易用，支持多种模型。
    > 2. **vLLM**: 高性能推理框架，适合生产环境。
    > 请问您选择哪一个？"
*   **挂起交互配置 (InteractionRequest)**：
    ```json
    {
      "kind": "select",
      "subject": "llm_solution_selection",
      "prompt": "请选择您希望部署的 Llama3 方案",
      "options": [
        { "id": "ollama", "label": "Ollama (推荐入门)", "aliases": ["第一个", "1", "ollama"] },
        { "id": "vllm", "label": "vLLM (生产级)", "aliases": ["第二个", "2", "vllm"] }
      ]
    }
    ```

### 5.2 约束条件收集 (FillInput)

*   **场景**：Workflow 处于 `Discover` 阶段，需要收集用户的环境信息和偏好。
*   **系统输出**：
    > "为了推荐合适的方案，请问：
    > 1. 本地机器是否有 GPU？
    > 2. 更关注中文效果、易部署，还是推理速度？"
*   **挂起交互配置 (InteractionRequest)**：
    ```json
    {
      "kind": "fill_input",
      "subject": "tts_constraints",
      "prompt": "请提供您的环境信息与偏好",
      "options": []
    }
    ```
*   **用户回复**：`本地部署，有 GPU，优先中文效果`
*   **解析结果**：`ResolutionIntent::ProvideInput`，更新 `WorkflowState.constraints`。

### 5.3 高风险动作确认 (ApproveAction)

*   **场景**：用户已选择 Ollama，Agent 准备执行部署脚本。
*   **当前阶段**：`AwaitExecutionConfirmation`。
*   **系统回复**：`即将执行脚本：docker run ... 占用端口 11434。是否确认继续？`
*   **挂起交互配置 (InteractionRequest)**：
    ```json
    {
      "kind": "approve",
      "subject": "deploy_ollama",
      "prompt": "即将执行 docker run 命令，占用端口 11434",
      "risk_level": "high",
      "options": [
        { "id": "approve", "label": "确认执行", "aliases": ["好的", "继续", "开始吧"] },
        { "id": "reject", "label": "取消", "aliases": ["不要", "先不要", "取消"] }
      ]
    }
    ```
*   **自然语言确认与状态迁移**：

| 用户输入 | 识别意图 | 状态迁移 | 系统后续回复示例 |
| :--- | :--- | :--- | :--- |
| `好的，开始吧` | **Approve** | `AwaitExecutionConfirmation` → `Executing` | "收到，正在拉取镜像并启动容器..." |
| `先不要动，我再想想` | **Reject** | `AwaitExecutionConfirmation` → `AwaitSelection` | "好的，已取消执行。您可以重新选择方案。" |

### 5.4 Agent 切换确认 (ConfirmSwitch)

*   **场景**：用户在 Workflow 进行中要求切换 agent。
*   **系统回复**：`当前有正在进行的部署流程，确定要切换到 OpenClaw 吗？`
*   **挂起交互配置 (InteractionRequest)**：
    ```json
    {
      "kind": "confirm_switch",
      "subject": "agent_switch_openclaw",
      "prompt": "当前有正在进行的部署流程，确定要切换到 OpenClaw 吗？",
      "risk_level": "low",
      "options": [
        { "id": "confirm", "label": "确认切换", "aliases": ["是", "对", "切换"] },
        { "id": "cancel", "label": "取消", "aliases": ["不", "算了"] }
      ]
    }
    ```

### 5.5 失败后重试或回滚 (RetryOrRollback)

*   **场景**：部署执行失败（如 Docker 拉取超时）。
*   **当前阶段**：`Executing` → 检测到失败。
*   **系统回复**：`部署失败：镜像拉取超时。是否重试，还是回滚到方案选择阶段？`
*   **挂起交互配置 (InteractionRequest)**：
    ```json
    {
      "kind": "retry_or_rollback",
      "subject": "deploy_failure",
      "prompt": "部署失败：镜像拉取超时",
      "risk_level": "medium",
      "options": [
        { "id": "retry", "label": "重试", "aliases": ["再试一次", "重新执行"] },
        { "id": "rollback", "label": "回滚到方案选择", "aliases": ["换一个", "回退", "重新选"] }
      ]
    }
    ```

---

## 6. Workflow 阶段迁移端到端测试

### 6.1 完整生命周期

以 TTS 方案搜索部署为例，验证 `WorkflowStage` 的完整迁移路径：

```
Discover → Compare → AwaitSelection → AwaitExecutionConfirmation → Executing → AwaitTestInput → Testing → Completed
```

### 6.2 逐阶段验证

| 步骤 | 用户输入 | 当前阶段 | 迁移目标 | 验证要点 |
| :--- | :--- | :--- | :--- | :--- |
| 1 | `我想要一个 TTS 方案` | — | `Discover` | WorkflowState 创建，topic="tts" |
| 2 | `本地部署，有 GPU，优先中文效果` | `Discover` | `Compare` | constraints 更新，触发候选搜索 |
| 3 | *(系统输出对比表)* | `Compare` | `AwaitSelection` | 产生 SelectOption 类型的 PendingInteraction |
| 4 | `第二个` | `AwaitSelection` | `AwaitExecutionConfirmation` | selected_candidate 写入，PendingInteraction 清除后创建新的 ApproveAction |
| 5 | `可以，继续` | `AwaitExecutionConfirmation` | `Executing` | PendingInteraction 清除，开始执行部署 |
| 6 | *(系统完成部署)* | `Executing` | `AwaitTestInput` | 产生 FillInput 类型的 PendingInteraction |
| 7 | `用这段文字测试：你好世界` | `AwaitTestInput` | `Testing` | PendingInteraction 清除，开始执行测试 |
| 8 | *(系统输出测试结果)* | `Testing` | `Completed` | WorkflowState 标记完成 |

### 6.3 阶段迁移日志验证

每次阶段迁移应在日志中输出：

```
[WorkflowEngine] stage_transition: AwaitExecutionConfirmation -> Executing
```

### 6.4 PendingInteraction 与阶段的对应关系

每个 `Await*` 阶段都应伴随一个 PendingInteraction 的创建，迁移后应清除：

| WorkflowStage | 对应 InteractionKind | 迁移时 PendingInteraction |
| :--- | :--- | :--- |
| `Discover` (收集约束时) | `FillInput` | 解析后清除 |
| `AwaitSelection` | `SelectOption` | 解析后清除 |
| `AwaitExecutionConfirmation` | `ApproveAction` | 解析后清除 |
| `AwaitTestInput` | `FillInput` | 解析后清除 |

---

## 7. 风险等级与确认策略测试

### 7.1 三级风险策略

| RiskLevel | 典型场景 | 确认策略 |
| :--- | :--- | :--- |
| **Low** | 方案选择、普通补参 | 接受自然语言确认，支持自动接受 |
| **Medium** | 拉取镜像、下载大模型 | 要求明确正向确认，模糊回复应追问 |
| **High** | 覆盖写文件、删除资源、重启服务 | 附带摘要回显，低置信度时要求二次确认 |

### 7.2 置信度边界测试

| RiskLevel | 用户输入 | 置信度 | 期望行为 |
| :--- | :--- | :--- | :--- |
| Low | `默认就行` | 高 | 直接通过 |
| Medium | `嗯` | 低 | 追问：`请明确确认是否继续拉取镜像` |
| Medium | `好的，拉取吧` | 高 | 通过 |
| High | `可以` | 中 | 回显摘要后通过 |
| High | `行吧` | 低 | 追问：`此操作将覆盖现有配置文件，请确认输入"确认执行"` |
| High | `确认执行` | 高 | 通过 |

### 7.3 自动接受行为

*   `risk_level: low` + 用户开启"自动信任基础操作" → 前端跳过确认弹窗
*   `risk_level: medium` 或 `high` → 无论设置如何，都必须显示确认弹窗
*   **测试要点**：在测试 high 风险动作时，务必关闭自动接受，验证弹窗阻塞行为

---

## 8. 异常与边界场景测试

### 8.1 无挂起时收到确认语

| 用户输入 | 会话状态 | 期望行为 |
| :--- | :--- | :--- |
| `好的` | 无 PendingInteraction | 走 `FallbackChat`，不触发任何动作 |
| `第二个` | 无 PendingInteraction | 走 `FallbackChat`，不误选任何选项 |
| `确认执行` | 无 PendingInteraction | 走 `FallbackChat` |

### 8.2 重复确认

| 场景 | 操作步骤 | 期望行为 |
| :--- | :--- | :--- |
| 已解析的交互被再次确认 | 1. 产生 PendingInteraction 2. 用户回复"确认" → 已 resolve 3. 用户再次说"确认" | 忽略或提示"该操作已处理" |

### 8.3 Workflow 中途退出

| 用户输入 | 当前状态 | 期望行为 |
| :--- | :--- | :--- |
| `我不想要了，换个话题` | AwaitSelection | 清除 WorkflowState 和 PendingInteraction，回到空闲状态 |
| `取消当前流程` | Executing | 尝试中止执行，清除 WorkflowState |
| `算了，帮我看个别的问题` | Discover | 清除 WorkflowState，将新消息作为 `StartNewTask` 处理 |

### 8.4 InteractionResolver 日志验证

每次挂起交互解析应输出：

```
[InteractionResolver] pending_id="abc123" resolution=Approve selected_option=None confidence=0.92
```

测试时应检查 `resolution` 和 `confidence` 字段是否符合预期。

---

## 9. 测试方法：命令行 vs 前端集成

### 9.1 命令行 (CLI) 测试
CLI 模式主要用于快速验证底层的 `AgentRuntime`（Prompt 效果与工具逻辑）。

*   **运行命令**：
    ```powershell
    # 交互模式
    cargo run --bin nova-cli -- chat --model gpt-4o
    ```
*   **参数示例**：
    *   `--verbose`: 显示工具调用的原始 JSON 输入/输出。
    *   `--base-url`: 指向中转 API 或本地 Mock 服务。
*   **局限性**：CLI 不支持可视化 `PendingInteraction` 面板，不适合测试完整控制层流程。

### 9.2 前端 (OpenFlux) 集成测试
这是测试 **会话控制层** 的唯一完整路径。

*   **依赖页面**：OpenFlux Chat Interface。
*   **开关项**：在设置中确保 `Workflow Auto-Sync` 已开启。
*   **测试覆盖范围**：TurnRouter 意图分类、Agent 切换、Workflow 阶段迁移、PendingInteraction 全流程。

---

## 10. 集成要点与配置片段

### 10.1 与 nova-gateway 集成
在 `src/gateway/mod.rs` 中，确保 `AgentRegistry` 包含了您的自定义 Agent：

```rust
let agent_registry = AgentRegistry::new(AgentDescriptor {
    id: "openclaw".to_string(),
    display_name: "OpenClaw".to_string(),
    aliases: vec!["oc".to_string(), "架构师".to_string()],
    system_prompt_template: "/prompts/openclaw.md".into(),
    ..Default::default()
});
```

### 10.2 状态同步注意点
1. **消息序列**：前端接收到 `interaction.request` 事件后，会锁定输入框或显示悬浮按钮。
2. **意图路由优先级**：`ResolvePendingInteraction` > `AddressAgent` > `ContinueWorkflow`。如果挂起状态存在，任何输入都会先交给 `InteractionResolver` 解析。
3. **阶段迁移同步**：每次 `WorkflowStage` 变更时，后端应通过 WebSocket 推送 `workflow.stage_changed` 事件，前端据此更新 UI 状态。

---

## 11. 反馈与调试指南

### 11.1 关键日志格式

测试过程中应关注以下三类日志，可通过关键字过滤定位问题：

| 组件 | 日志前缀 | 格式示例 |
| :--- | :--- | :--- |
| TurnRouter | `[TurnRouter]` | `[TurnRouter] input="继续" intent=ResolvePendingInteraction confidence=0.95` |
| InteractionResolver | `[InteractionResolver]` | `[InteractionResolver] pending_id="abc123" resolution=Approve selected_option=None confidence=0.92` |
| WorkflowEngine | `[WorkflowEngine]` | `[WorkflowEngine] stage_transition: AwaitExecutionConfirmation -> Executing` |
| AgentContext | `[AgentContext]` | `[AgentContext] switch: nova -> openclaw trigger=handoff` |

### 11.2 WebSocket 事件抓包

在浏览器 F12 Network 面板中观察以下 WebSocket 消息：

| 事件名 | 触发时机 | 关键字段 |
| :--- | :--- | :--- |
| `interaction.request` | 产生新的 PendingInteraction | `kind`, `subject`, `risk_level`, `options` |
| `interaction.resolved` | 用户回复被解析后 | `result`, `resolution`, `confidence` |
| `workflow.stage_changed` | WorkflowStage 迁移 | `from`, `to`, `topic` |
| `agent.switched` | Agent 切换 | `from`, `to`, `trigger` |
