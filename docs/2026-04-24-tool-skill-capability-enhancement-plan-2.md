# 2026-04-24 tool-skill-capability-enhancement-plan-2

| 章节 | 说明 |
|------|------|
| Plan 编号与标题 | Plan 2：Skill 路由、激活态与 Prompt 组装 |
| 前置依赖 | Plan 1 |
| 本次目标 | 把 skill 从"静态提示词集合"升级为"会话级可激活能力"，建立路由、切换、sticky、退出和历史切片机制，并在 `run_turn` 前生成真正的 `TurnContext`。 |
| 涉及文件 | `crates/nova-core/src/agent.rs`、`crates/nova-core/src/prompt.rs`、`crates/nova-core/src/skill.rs`、`crates/nova-core/src/event.rs`、`crates/nova-core/src/conversation.rs`（或等价 session 层）、`crates/nova-app/src/bootstrap.rs`、`crates/nova-cli/src/main.rs`、`.nova/prompts/turn-router.md`、`.nova/prompts/workflow-stages.md` |
| 代码验证状态 | 已部分确认 (2026-04-24) |

---

## 详细设计

### 1. Session 级 Active Skill 状态

#### 1.1 状态所有权确认

**关键决策**：`ActiveSkillState` 必须放在 **session 层**（`nova-conversation`），而非 `AgentRuntime`。

**原因**：
- `AgentRuntime` 可能在**同一个进程**中跨多个会话复用（特别是在 gateway 模式下）
- 当前 `AgentRuntime::new()` 创建后的生命周期可能覆盖多次 `chat` 调用
- 如果 `ActiveSkillState` 放在 runtime 中，skill 数据会在会话间泄漏

#### 1.2 `ActiveSkillState` 定义

```rust
pub struct ActiveSkillState {
    pub skill_id: String,      // 当前 active skill 的 id
    pub entered_at: Instant,   // 激活时间（用于 debug）
    pub last_routed_at: Instant, // 最近一次路由评估时间
    pub history_token_count: usize, // 追踪当前 session token 使用量
}
```

#### 1.3 End-User 事件

新增以下 `AgentEvent` 变体（在现有 `event.rs` 中追加）：

```rust
pub enum AgentEvent {
    // ...existing events...
    SkillActivated {
        skill_id: String,
        skill_name: String,
        sticky: bool,
        reason: String,     // "auto" | "explicit" | "fallback"
    },
    SkillSwitched {
        from_skill: String,
        to_skill: String,
        reason: String,
    },
    SkillExited {
        skill_id: String,
        reason: String,
    },
    SkillRouteEvaluated {
        result: SkillRouteDecision,
        confidence: f64,    // 0.0 - 1.0
        reasoning: String,
    },
}
```

以及辅助类型：

```rust
pub enum SkillRouteDecision {
    KeepCurrent,
    Activate(String),    // skill_id
    Deactivate,
    NoSkill,
}
```

**使用场景：**
- CLI 能打印当前 skill 变化（通过事件监听）
- Gateway 能透出给桌面端（通过事件转换）
- 评测工具能断言路由结果是否符合预期

---

### 2. Skill 路由流程

#### 2.1 路由决策机制

每轮请求前执行 `SkillRouter::route()`，返回 `SkillRouteDecision`：

```rust
pub async fn route(
    &self,
    current_message: &Message,
    active_state: Option<&ActiveSkillState>,
    candidates: &[&SkillPackage],
    config: &RoutingConfig,
) -> SkillRouteDecision {
    // 优先检查 sticky
    if let Some(state) = active_state {
        if let Some(skill) = self.find_skill_by_id(&state.skill_id, candidates) {
            if skill.sticky {
                return SkillRouteDecision::KeepCurrent;
            }
        }
    }

    // 检查显式退出
    if self.is_exit_signal(current_message.content.as_str()) {
        return SkillRouteDecision::Deactivate;
    }

    // 执行 LLM 路由评估
    let result = self.evaluate_with_llm(current_message, candidates).await?;

    // 根据置信度决定
    match result {
        LlmRoutingResult::HighConfidence(id, _) => SkillRouteDecision::Activate(id),
        LlmRoutingResult::KeepPreferred(id, _) => SkillRouteDecision::KeepCurrent,
        LlmRoutingResult::LowConfidence => SkillRouteDecision::NoSkill,
        LlmRoutingResult::Error => SkillRouteDecision::NoSkill,
    }
}
```

#### 2.2 路由配置枚举

```rust
pub enum SkillRouteAction {
    KeepActive,     // 保持当前活跃
    ActivateNew,    // 激活新 skill
    Deactivate,     // 退出当前 skill
    NoSkill,        // 不激活任何 skill
}
```

#### 2.3 路由优先级（已验证模型）

1. **Sticky 保持**：若 active skill 且 `sticky=true`，则默认 `KeepCurrent`
2. **用户显式退出**：用户输入 `/exit-skill`、`/reset-skill` 时，执行 `Deactivate`
3. **LLM 路由评估**：根据用户消息和可选候选 skill 描述做路由
4. **无匹配回退**：无高置信结果则返回 `NoSkill`

#### 2.4 阶段一实现细节

第一阶段不强制引入新模型，可复用现有主模型配置：

- 使用 `config.rs` 中已有的 `llm.*` 配置
- 路由 prompt 放在 `.nova/prompts/turn-router.md` 文件中
- 单次低 token 路由调用（prompt 仅包含 `skill_name + description` 列表 + 用户当前消息）

---

### 3. Skill 切换与历史切片

#### 3.1 当前历史管理

**当前状态（已验证）**：
- `AgentRuntime::run_turn` 接收 `history: &[Message]`
- 所有消息平铺在一个 `Vec<Message>` 中，不分 skill 或轮次

```rust
// 当前结构
pub struct AgentState {
    pub history: Arc<ContactHistory>, // 扁平 Vec<Message>
    // ...
}
```

#### 3.2 目标历史结构

**Skill-aware 历史段划分**：

```rust
// 三者结构：
// 1. 全局摘要（跨所有 skill 的共识）
pub struct GlobalHistorySummary {
    pub user_goals: Vec<String>,      // 用户核心目标
    pub key_decisions: Vec<String>,   // 关键决策记录
    pub unfinished_items: Vec<String>, // 未完成事项
}

// 2. 每个 skill 的独立摘要
pub struct SkillHistorySegment {
    pub skill_id: String,
    pub goals: Vec<String>,
    pub decisions: Vec<String>,
    pub tasks: Vec<String>,           // 该 skill 相关的任务
}

// 3. 当前 active segment（原始消息）
pub struct ActiveHistorySegment {
    pub messages: Arc<Vec<Message>>,
    pub token_estimate: usize,
}
```

#### 3.3 切换逻辑（4 步）

当 skill 从 A 切换到 B 时：

```
1. 结束当前 active segment
2. 将旧 segment 归约为摘要对象：
   - 用户目标
   - 已做决策
   - 未完成事项
   - 关键路径引用
3. 新建新的 active segment
4. 后续 prompt 中只保留：
   - 全局摘要（Always included）
   - 所有 skill 摘要（作为 context reference）
   - 当前 skill 的 active segment（完整消息）
```

**Token 预算计算**（重要）：

```rust
// 建议的 token 预算公式
const GLOBAL_SUMMARY_MAX_TOKENS: usize = 1000;
const SKILL_SUMMARY_MAX_TOKENS: usize = 500;
const ACTIVE_SEGMENT_MAX_TOKENS: usize = 4000; // 其余留给对话
// 默认上限：5500 tokens
```

#### 3.4 第一种实现策略

第一阶段先用**规则摘要**：
- 从 `ToolEnd` 事件提取关键信息（如涉及文件名的 Edit 操作）
- 根据 message role 分类（User/AI 消息各有不同提取规则）
- 丢弃 `LogDelta` 等详细日志消息

第二阶段（如有 LLM 路由能力）再扩展为 LLM 摘要。

---

### 4. TurnContext 构建

#### 4.1 接口定义

在 `AgentRuntime::run_turn` 调用前，引入显式的 turn preparation：

```rust
pub struct TurnContext {
    // 主动构造
    pub system_prompt: String,
    pub tool_definitions: Vec<ToolDefinition>,
    pub history: Arc<Vec<Message>>,
    pub active_skill: Option<ActiveSkillState>,
    pub capability_policy: CapabilityPolicy,

    // 构造后只读
    pub max_tokens: usize,
    pub iteration_budget: usize, // 当前轮剩余最大迭代次数
}

// AgentRuntime 新增接口
impl AgentRuntime {
    pub fn prepare_turn(
        &self,
        message: &Message,
        capability_policy: CapabilityPolicy,
    ) -> Result<TurnContext> {
        // 1. 决定 active skill
        // 2. 根据 active skill 生成 capability policy
        // 3. 生成 system prompt sections
        // 4. 过滤工具定义
        // 5. 裁剪历史
        // 6. 构造最终 TurnContext
    }

    pub async fn run_turn_with_context(
        &self,
        ctx: TurnContext,
        message: Message,
    ) -> Result<TurnResult> {
        run_turn(ctx, message).await
    }
}
```

**运行时接入方式：**

- 新增 `prepare_turn()` 方法
- `run_turn()` 只消费已经准备好的上下文
- CLI / app / gateway 共用同一套准备逻辑

---

### 4.2 SkillTool 三层模型（新增 - 基于 v1_messages 分析）

**背景**：原始会话分析发现，Skills 暴露但未调用（`/skill-name` 模式只支持用户显式输入）。需三层模型区分调用来源：

```rust
pub enum SkillInvocationLevel {
    SessionLevel,   // 会话级 —— Turn 自动路由决定
    ToolLevel,      // 工具级 —— 模型自动调用 SkillTool（需 prompt 明确触发条件）
    UserLevel,      // 用户级 —— 用户显式输入 /skill-name
}
```

**SkillTool 自动调用触发条件**（需写入 SystemPrompt `# Using your tools` 部分）：

```
当以下场景出现时，自动调用 Skill 工具：
1. 当前上下文需要参考外部 skill 说明文档，但不在 active skill 范围内
2. 用户提及的工具/能力不在当前工具集中，且存在同名 skill
3. 多步骤任务需要分阶段执行，分阶段边界对应不同 skill

调用格式：Skill(skill="<slug>")，返回结构化输出而非纯文本
```

**SkillTool 输出格式**（不再只返回拼接文本）：

```rust
pub struct SkillToolOutput {
    pub skill_name: String,
    pub description: String,
    pub instructions: String,  // 主指令
    pub tools: Vec<String>,    // 该 skill 支持的工具列表
    pub source: String,        // 源头文件路径
}
```

**三层可控性保证**：

| 层级 | 触发方式 | 频率 | 影响范围 |
|------|----------|------|----------|
| 会话级 Skill | Turn 自动路由 | 中 | 整个 session 的 active skill |
| 工具级 SkillTool | 模型自动调用 | 低 | 仅补充说明，不覆盖 active_skill |
| 用户级 /skill-name | 用户文本输入 | 低 | 可替代或叠加 active skill |

---

### 5. CLI 接入 (初步支持)

#### 5.1 CLI 端使用 `TurnContext`

CLI 或其他入口变更：

```rust
// 当前
let result = agent.run_turn(history, user_input).await;

// 目标
let ctx = agent.prepare_turn(&user_input, policy).await?;
let result = agent.run_turn_with_context(ctx, user_input).await;
```

#### 5.2 事件桥接

CLI 需要在进用户输入前，发射 `SkillRouteEvaluated` 事件，以便：
- CLI 能打印当前 skill 变化
- Gateway 能透出给桌面端
- 评测工具能断言路由结果是否符合预期

---

## 测试案例

1. **正常路径**：无 active skill 时，用户输入匹配某个 skill，路由结果为 `Activate`，system prompt 含对应 skill section。
2. **正常路径**：active sticky skill 存在时，普通后续消息保持 `KeepCurrent`。
3. **正常路径**：用户显式退出后，skill 被清空，后续回到默认模式。
4. **边界条件**：路由无匹配时，不激活 skill，但会话继续运行。
5. **边界条件**：skill 从 A 切到 B 时，A 的历史被摘要，B 的 active segment 重新开始。
6. **异常场景**：路由调用失败时，回退到 `NoSkill`，不能阻塞整轮对话。
7. **异常场景**：skill prompt 缺失时，返回带路径信息的错误，而不是 silently fallback。
8. **新测试**：验证 `TurnContext` 构造后的 system prompt 包含所有 expected sections（验证顺序和内容存在性）。
9. **新测试**：验证 `HistoryManager` 的 token 预算约束 — 超过 `ACTIVE_SEGMENT_MAX_TOKENS` 时触发摘要。
10. **新测试**：验证 skill 切换时 `AgentEvent::SkillSwitched` 被正确发射，CLI/gateway 能接收到。
11. **新测试**：验证 Session 压缩阶段 — 当 `history_token_count` 超过 `WINDOW_MAX_TOKENS` 时触发压缩。
12. **新测试**：验证 SkillTool 三层模型 — 工具级调用不覆盖 `active_skill`，只有用户级 `/skill` 才能替代。
