# Phase 6：Skill 系统与上下文优化

> 前置依赖：Phase 1-5

## 1. 目标

将 Skill 降级为执行层插件，实现 prompt 按需注入和工具过滤。
同时引入 History Trimmer 解决长会话下的上下文膨胀问题。

核心交付：
- Skill 定义、注册、动态加载机制
- 上下文分层组装（Fixed / Agent / Skill / History）
- 工具过滤（按 agent + skill 组合）
- History Trimmer（基于规则的历史裁剪）

## 2. 当前代码现状（Phase 5 完成后预期）

Phase 5 完成后已具备：
- `TurnRouter` 完整优先级链
- `AgentRegistry` 多 agent 注册与切换
- `SolutionWorkflow` 状态机
- `PendingInteraction` 基础设施

但仍缺少：
- prompt 的模块化组装（当前 system prompt 是单一字符串，`prompts/default.md` compile-time 嵌入）
- 工具按场景过滤（当前 `ToolRegistry` 暴露全部工具给 LLM）
- 历史裁剪（长会话时全量历史会超出 context window）
- Skill 概念（特定行为模板 + 工具约束 + prompt 片段的组合）

## 3. 详细设计 (Detailed Design)

### 3.1 Skill 定义

Skill 是执行层的能力模块，不是顶层控制入口。它提供三样东西：

```rust
pub struct SkillDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    /// 补充 prompt 片段，注入到 system prompt 的 skill 层
    pub prompt_fragment: String,
    /// 工具白名单：None = 不限制（使用 agent 级工具集）
    pub tool_whitelist: Option<Vec<String>>,
    /// 哪些 agent 可以使用此 skill：None = 所有 agent
    pub agent_whitelist: Option<Vec<String>>,
}
```

```rust
pub struct SkillRegistry {
    skills: HashMap<String, SkillDefinition>,
}

impl SkillRegistry {
    pub fn register(&mut self, skill: SkillDefinition);
    pub fn get(&self, id: &str) -> Option<&SkillDefinition>;
    pub fn list_for_agent(&self, agent_id: &str) -> Vec<&SkillDefinition>;
}
```

Skill 的选择发生在 `TurnRouter` 返回 `ExecuteChat` 之后、实际调用 `AgentRuntime` 之前。初版 skill 选择使用静态规则（关键词匹配），不引入额外 LLM 调用。

### 3.2 上下文分层架构

System prompt 由四层组成，按以下顺序拼接：

```
┌─────────────────────────────────┐
│  Layer 1: Fixed System Prompt   │  基础世界观、行为准则、安全约束
│  来源: prompts/default.md       │  Token 预算: 固定，不可压缩
├─────────────────────────────────┤
│  Layer 2: Agent Prompt          │  当前 active_agent 的角色描述
│  来源: AgentDescriptor          │  Token 预算: 固定，不可压缩
├─────────────────────────────────┤
│  Layer 3: Skill Prompt          │  当前 active_skill 的行为模板
│  来源: SkillDefinition          │  Token 预算: 固定，不可压缩
├─────────────────────────────────┤
│  Layer 4: History Summary       │  被裁剪历史的摘要（如有）
│  来源: HistoryTrimmer           │  Token 预算: 上限 2000 tokens
└─────────────────────────────────┘
```

**冲突解决规则：**
- 如果 Agent Prompt 和 Skill Prompt 有矛盾，Skill Prompt 优先（更具体的指令覆盖更泛的指令）
- 如果总 system prompt 超出 token 预算，压缩顺序为：History Summary > Skill Prompt > Agent Prompt。Fixed System Prompt 不可压缩

```rust
pub struct SystemPromptBuilder {
    fixed_prompt: String,
    agent_prompt: Option<String>,
    skill_prompt: Option<String>,
    history_summary: Option<String>,
}

impl SystemPromptBuilder {
    pub fn new(fixed: &str) -> Self;
    pub fn with_agent(self, prompt: &str) -> Self;
    pub fn with_skill(self, prompt: &str) -> Self;
    pub fn with_history_summary(self, summary: &str) -> Self;
    pub fn build(self) -> String;
}
```

### 3.3 工具过滤

工具集的确定经过两级过滤：

```
全量工具（ToolRegistry）
  → Agent 白名单过滤（AgentDescriptor.tool_whitelist）
    → Skill 白名单过滤（SkillDefinition.tool_whitelist）
      → 最终暴露给 LLM 的工具列表
```

```rust
impl ToolRegistry {
    /// 两级过滤：先按 agent 白名单，再按 skill 白名单
    pub fn get_filtered_tools(
        &self,
        agent: &AgentDescriptor,
        skill: Option<&SkillDefinition>,
    ) -> Vec<ToolDefinition> {
        let agent_tools = match &agent.tool_whitelist {
            Some(whitelist) => self.tools.iter()
                .filter(|t| whitelist.contains(&t.name))
                .collect(),
            None => self.tools.iter().collect(),
        };

        match skill.and_then(|s| s.tool_whitelist.as_ref()) {
            Some(whitelist) => agent_tools.into_iter()
                .filter(|t| whitelist.contains(&t.name))
                .collect(),
            None => agent_tools,
        }
    }
}
```

### 3.4 History Trimmer

#### 裁剪规则

```rust
pub struct TrimmerConfig {
    /// 历史消息的最大 token 数（估算）
    pub max_history_tokens: usize,    // 默认: 100_000
    /// 保留最近 N 条消息不被裁剪
    pub preserve_recent: usize,       // 默认: 10
    /// 裁剪时是否保留 tool_use/tool_result 对
    pub preserve_tool_pairs: bool,    // 默认: false
}
```

**裁剪算法：**

```rust
pub struct HistoryTrimmer;

impl HistoryTrimmer {
    /// 对历史消息进行裁剪
    /// 返回 (trimmed_history, summary_of_removed)
    pub fn trim(
        history: &[Message],
        config: &TrimmerConfig,
    ) -> (Vec<Message>, Option<String>) {
        let total_tokens = estimate_tokens(history);

        if total_tokens <= config.max_history_tokens {
            return (history.to_vec(), None);
        }

        // 1. 将历史分为 [old_part | recent_part]
        //    recent_part = 最近 preserve_recent 条消息（不裁剪）
        // 2. 从 old_part 的最早消息开始删除，直到总 token 数 <= max_history_tokens
        // 3. 对被删除的消息生成规则摘要（非 LLM）：
        //    - 统计被删除的 user/assistant 消息数
        //    - 提取被删除消息中的 tool_use 名称列表
        //    - 格式: "[历史摘要] 之前的对话中，用户提出了 N 个问题，
        //             使用了以下工具: X, Y, Z。详细内容已省略。"
    }
}
```

**Token 估算：**
初版使用简单的字符数 / 4 估算（中文字符 / 2），不引入 tokenizer 依赖。

```rust
fn estimate_tokens(messages: &[Message]) -> usize {
    messages.iter()
        .map(|m| {
            let text = serialize_message_text(m);
            // 粗估：英文约 4 chars/token，中文约 2 chars/token
            // 使用保守估计 3 chars/token
            text.len() / 3
        })
        .sum()
}
```

**为什么不用 LLM 做摘要（初版）：**
- LLM 摘要会引入额外延迟和成本
- 历史裁剪可能在每轮 chat 都触发，不应依赖额外 API 调用
- 规则摘要虽然信息损失更大，但可预测、无副作用
- 后续可升级为异步 LLM 摘要（在后台生成，不阻塞当前轮）

### 3.5 Skill 执行链

当 `TurnRouter` 返回 `ExecuteChat` 时，执行链如下：

```rust
async fn execute_chat_turn<C: LlmClient>(
    session: Arc<Session>,
    payload: &ChatPayload,
    state: Arc<AppState<C>>,
    outbound_tx: ...,
    msg_id: String,
) {
    // 1. 确定 active agent
    let control = session.control.read();
    let agent_desc = state.agent_registry.get(&control.active_agent);

    // 2. 确定 active skill（初版：规则匹配或 None）
    let skill = resolve_skill(&payload.input, agent_desc, &state.skill_registry);

    // 3. 组装 system prompt
    let prompt = SystemPromptBuilder::new(&state.fixed_prompt)
        .with_agent(&agent_desc.system_prompt_template)
        .with_skill(skill.map(|s| s.prompt_fragment.as_str()).unwrap_or(""))
        .build();

    // 4. 过滤工具集
    let tools = state.tool_registry.get_filtered_tools(agent_desc, skill);

    // 5. 裁剪历史
    let history = session.get_history();
    let (trimmed_history, summary) = HistoryTrimmer::trim(&history, &state.trimmer_config);

    // 如果有摘要，将其作为 system prompt 的 Layer 4
    let final_prompt = if let Some(ref s) = summary {
        SystemPromptBuilder::new(&state.fixed_prompt)
            .with_agent(&agent_desc.system_prompt_template)
            .with_skill(skill.map(|s| s.prompt_fragment.as_str()).unwrap_or(""))
            .with_history_summary(s)
            .build()
    } else {
        prompt
    };

    // 6. 调用 AgentRuntime（使用组装后的 prompt 和过滤后的工具）
    let result = state.default_agent
        .run_turn_with_context(&trimmed_history, &payload.input, event_tx, token, &final_prompt, &tools)
        .await;

    // 7. 后续写入和发送逻辑（与 Phase 3 相同）
}
```

这需要 `AgentRuntime` 新增一个方法：

```rust
impl<C: LlmClient> AgentRuntime<C> {
    /// 使用指定的 prompt 和工具集执行一轮对话
    /// 与 run_turn 的区别：不使用 runtime 自身的 system_prompt 和 tools
    pub async fn run_turn_with_context(
        &self,
        history: &[Message],
        user_input: &str,
        event_tx: mpsc::Sender<AgentEvent>,
        cancellation_token: Option<CancellationToken>,
        system_prompt: &str,
        tools: &[ToolDefinition],
    ) -> Result<TurnResult>;
}
```

### 3.6 协议补全

本 Phase 需要补全以下协议的正式实现（替换 `NOT_IMPLEMENTED` 占位）：

| 消息类型 | 动作 |
|---------|------|
| `sessions.delete` | Phase 3 已实现 |
| `sessions.logs` | 返回 session 的操作日志（可选，如不实现则保持 NOT_IMPLEMENTED） |
| `sessions.artifacts` | 返回 session 产生的文件列表（可选） |
| `agents.create` | 从 API 动态注册 agent 到 registry |
| `chat.stop` | Phase 3 已实现 |

## 4. 本 phase 范围

### 4.1 要做

- 定义 `SkillDefinition` / `SkillRegistry` 数据结构
- 实现 Skill 注册和按 agent 查询
- 实现 `SystemPromptBuilder` 四层组装
- 实现 `ToolRegistry::get_filtered_tools` 两级过滤
- 实现 `HistoryTrimmer::trim` 基于规则的历史裁剪
- 实现 token 估算函数
- 新增 `AgentRuntime::run_turn_with_context`
- 将 `execute_chat_turn` 改为走完整的 skill 执行链
- 补全 `agents.create` 协议实现

### 4.2 不做

- 不做自动学习用户习惯的动态 Skill 生成
- 不做基于向量库的长短期记忆
- 不做 LLM-based 历史摘要（保留为后续升级路径）
- 不做 LLM-based skill 路由（使用规则匹配）
- 不做 `sessions.logs` / `sessions.artifacts`（保持 NOT_IMPLEMENTED，非核心功能）

## 5. 实施步骤

### Step 1：SkillDefinition + SkillRegistry

文件：
- `src/gateway/skill.rs`（新建）

动作：
- 定义 `SkillDefinition` / `SkillRegistry`
- 实现注册、查询、按 agent 过滤

### Step 2：SystemPromptBuilder 重构

文件：
- `src/prompt.rs`

动作：
- 改造 `SystemPromptBuilder` 支持四层组装
- 保持对当前 `with_tools` 的向后兼容

### Step 3：工具过滤

文件：
- `src/tool.rs`

动作：
- 新增 `get_filtered_tools(agent, skill)` 方法

### Step 4：HistoryTrimmer

文件：
- `src/gateway/trimmer.rs`（新建）

动作：
- 实现 `TrimmerConfig` / `HistoryTrimmer::trim`
- 实现 `estimate_tokens`
- 实现规则摘要生成

### Step 5：AgentRuntime 扩展

文件：
- `src/agent.rs`

动作：
- 新增 `run_turn_with_context` 方法

### Step 6：执行链集成

文件：
- `src/gateway/handlers/chat.rs`

动作：
- `execute_chat_turn` 改为走完整的 prompt 组装 + 工具过滤 + 历史裁剪链路

### Step 7：协议补全

文件：
- `src/gateway/router.rs`
- `src/gateway/handlers/agents.rs`

动作：
- 接入 `agents.create`

## 6. 测试方案

### 6.1 SkillRegistry 单元测试

| 测试用例 | 验证点 |
|---------|-------|
| `test_register_and_get` | 注册后能查询到 |
| `test_list_for_agent` | 按 agent_whitelist 过滤正确 |
| `test_list_for_agent_no_whitelist` | agent_whitelist 为 None 时返回全部 |

### 6.2 SystemPromptBuilder 单元测试

| 测试用例 | 验证点 |
|---------|-------|
| `test_build_fixed_only` | 只有 fixed 层时输出正确 |
| `test_build_all_layers` | 四层都有时按顺序拼接 |
| `test_build_skip_empty` | 空字符串层不产生多余分隔符 |

### 6.3 工具过滤单元测试

| 测试用例 | 验证点 |
|---------|-------|
| `test_no_filter` | 两级白名单都是 None 时返回全部 |
| `test_agent_filter` | agent 白名单生效 |
| `test_skill_filter` | skill 白名单在 agent 过滤基础上进一步过滤 |
| `test_skill_filter_subset` | skill 白名单不能引入 agent 白名单之外的工具 |

### 6.4 HistoryTrimmer 单元测试

| 测试用例 | 验证点 |
|---------|-------|
| `test_no_trim_needed` | 历史未超限时原样返回，无摘要 |
| `test_trim_old_messages` | 超限时删除最早的消息 |
| `test_preserve_recent` | 最近 N 条消息不被裁剪 |
| `test_summary_content` | 摘要包含被删除的消息数和工具名 |
| `test_estimate_tokens` | 估算结果在合理范围内 |

### 6.5 回归要求

```powershell
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test --workspace
```

## 7. 完成定义

- [ ] `SkillDefinition` / `SkillRegistry` 可注册和查询
- [ ] `SystemPromptBuilder` 支持四层组装
- [ ] `ToolRegistry::get_filtered_tools` 两级过滤正确
- [ ] `HistoryTrimmer` 能裁剪超限历史并生成规则摘要
- [ ] `execute_chat_turn` 走完整的 skill 执行链
- [ ] `agents.create` 协议已实现
- [ ] 全部测试用例通过

## 8. 后续演进方向

Phase 6 完成后，系统具备完整的分层控制架构。后续可按需演进：

| 方向 | 说明 |
|------|------|
| LLM 历史摘要 | 后台异步调用 LLM 生成高质量摘要，替代规则摘要 |
| LLM Skill 路由 | 当规则匹配无法确定 skill 时 fallback 到 LLM 分类 |
| Skill 热加载 | 从配置文件或 API 动态加载 skill 定义 |
| Workflow 持久化 | 将 WorkflowState 序列化到磁盘，支持服务重启恢复 |
| Session 持久化 | SQLite 或其他存储后端 |
| 向量记忆 | 引入 embedding 做长期记忆检索 |
