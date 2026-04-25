# Phase 3：高级功能（P2）

- 日期：2026-04-25
- 状态：草案
- 优先级：P2 — 依赖 Phase 1 + Phase 2 完成
- 前置条件：Phase 1（统一构建管道）和 Phase 2（环境快照、项目上下文、skill 按需注入）已合并
- 主文档：`docs/design/2026-04-25-prompt-architecture-enhancement.md`
- 涉及文件：
  - `crates/nova-core/src/prompt.rs`
  - `crates/nova-core/src/agent.rs`
  - `crates/nova-core/src/skill.rs`
  - `crates/nova-core/src/config.rs`
  - `crates/nova-conversation/src/service.rs`
  - `crates/nova-app/src/conversation_service.rs`

---

## 一、目标

| 编号 | 目标 | 说明 | 复杂度 |
|------|------|------|--------|
| G9 | 历史管理策略 | Token 预算感知的历史裁剪/摘要 | 高 |
| G10 | 侧信道注入 | 在 tool result 中嵌入动态上下文刷新 | 中 |
| G11 | prepare_turn 接入主流程 | 让 TurnContext 路径成为 ConversationService 的主流程 | 高 |
| B8-fix | 修复 run_turn_with_context | 补全工具执行逻辑和 usage 统计 | 中 |

---

## 二、G9 — 历史管理策略

### 2.1 问题描述

当前 Session 历史**全量传递**给 LLM，没有任何裁剪或摘要机制。

相关代码路径：
- `agent.rs:81`：`let mut all_messages = history.to_vec();` — 历史直接复制
- `agent.rs:576-582`：`trim_history()` 返回 `current_history.clone()` — no-op
- `config.rs:126`：`max_tokens: usize` 配置存在但未用于历史裁剪
- `config.toml`：`[gateway.trimmer]` 配置段存在但 trimmer 逻辑未接入

当对话变长时，历史 token 会超出模型上下文限制，导致 API 报错。

### 2.2 设计方案

#### 2.2.1 Token 预算分配模型

```
模型总上下文 = context_window（如 128K 或 200K token）

┌──────────────────────────────────────────────┐
│                   context_window              │
├──────────────┬───────────────┬───────────────┤
│ system_prompt │  history      │  output_reserve│
│   (固定)      │  (可裁剪)     │  (预留)        │
└──────────────┴───────────────┴───────────────┘

history_budget = context_window - system_prompt_tokens - output_reserve
```

#### 2.2.2 新增 HistoryTrimmer 结构体

```rust
// crates/nova-core/src/prompt.rs — 新增

/// 历史裁剪配置。
#[derive(Debug, Clone)]
pub struct TrimmerConfig {
    /// 模型上下文窗口大小（token 数）
    pub context_window: usize,
    /// 输出预留 token 数
    pub output_reserve: usize,
    /// 最少保留的最近消息数（不被裁剪）
    pub min_recent_messages: usize,
    /// 是否启用历史摘要（替代简单截断）
    pub enable_summary: bool,
}

impl Default for TrimmerConfig {
    fn default() -> Self {
        Self {
            context_window: 128_000,
            output_reserve: 8_192,
            min_recent_messages: 10,
            enable_summary: false,
        }
    }
}

/// 历史裁剪器。
pub struct HistoryTrimmer {
    config: TrimmerConfig,
}

/// 裁剪结果。
pub struct TrimResult {
    /// 裁剪后的消息列表
    pub messages: Vec<crate::message::Message>,
    /// 是否发生了裁剪
    pub was_trimmed: bool,
    /// 被移除的消息数量
    pub removed_count: usize,
    /// 摘要文本（如果启用了摘要）
    pub summary: Option<String>,
}
```

#### 2.2.3 Token 估算

精确的 token 计算需要 tokenizer。Phase 3 先使用字符数估算，后续可替换为精确 tokenizer。

```rust
impl HistoryTrimmer {
    pub fn new(config: TrimmerConfig) -> Self {
        Self { config }
    }

    /// 估算消息列表的 token 数。
    ///
    /// 使用字符数 / 4 的粗略估算（英文约 4 chars/token，中文约 1.5 chars/token）。
    /// 取折中值 3 chars/token。
    fn estimate_tokens(messages: &[crate::message::Message]) -> usize {
        let total_chars: usize = messages.iter().map(|m| {
            m.content.iter().map(|block| {
                match block {
                    crate::message::ContentBlock::Text { text } => text.len(),
                    crate::message::ContentBlock::Thinking { thinking } => thinking.len(),
                    crate::message::ContentBlock::ToolUse { name, input, .. } => {
                        name.len() + input.to_string().len()
                    }
                    crate::message::ContentBlock::ToolResult { output, .. } => output.len(),
                    _ => 0,
                }
            }).sum::<usize>()
        }).sum();

        total_chars / 3
    }

    /// 估算系统提示词的 token 数。
    fn estimate_system_prompt_tokens(system_prompt: &str) -> usize {
        system_prompt.len() / 3
    }
}
```

#### 2.2.4 裁剪策略

```rust
impl HistoryTrimmer {
    /// 对历史消息进行裁剪。
    ///
    /// 策略：
    /// 1. 保留第一条 system 消息（如果存在）
    /// 2. 保留最近 min_recent_messages 条消息
    /// 3. 从最旧的非 system 消息开始移除，直到总 token 在预算内
    /// 4. 确保 tool_use 和对应的 tool_result 成对移除（不留孤立的 tool_result）
    pub fn trim(
        &self,
        messages: &[crate::message::Message],
        system_prompt: &str,
    ) -> TrimResult {
        let system_tokens = Self::estimate_system_prompt_tokens(system_prompt);
        let history_budget = self.config.context_window
            .saturating_sub(system_tokens)
            .saturating_sub(self.config.output_reserve);

        let current_tokens = Self::estimate_tokens(messages);

        // 如果在预算内，不裁剪
        if current_tokens <= history_budget {
            return TrimResult {
                messages: messages.to_vec(),
                was_trimmed: false,
                removed_count: 0,
                summary: None,
            };
        }

        // 分离 system 消息和对话消息
        let (system_msgs, conversation_msgs): (Vec<_>, Vec<_>) = messages.iter()
            .enumerate()
            .partition(|(_, m)| m.role == crate::message::Role::System);

        let system_msgs: Vec<_> = system_msgs.into_iter().map(|(_, m)| m.clone()).collect();
        let conversation_msgs: Vec<_> = conversation_msgs.into_iter().map(|(_, m)| m.clone()).collect();

        // 保护最近 N 条消息
        let protected_count = self.config.min_recent_messages.min(conversation_msgs.len());
        let trimmable = &conversation_msgs[..conversation_msgs.len() - protected_count];
        let protected = &conversation_msgs[conversation_msgs.len() - protected_count..];

        // 从前往后移除消息，直到总 token 在预算内
        let protected_tokens = Self::estimate_tokens(protected)
            + Self::estimate_tokens(&system_msgs);
        let mut remaining_budget = history_budget.saturating_sub(protected_tokens);

        let mut kept_trimmable = Vec::new();
        let mut removed_count = 0;
        let mut keeping = false; // 一旦开始保留，后续都保留（避免中间断开）

        // 从后往前扫描可裁剪消息，保留尽可能多的最近消息
        for msg in trimmable.iter().rev() {
            let msg_tokens = Self::estimate_tokens(&[msg.clone()]);
            if !keeping && msg_tokens <= remaining_budget {
                remaining_budget -= msg_tokens;
                kept_trimmable.push(msg.clone());
            } else if keeping {
                kept_trimmable.push(msg.clone());
            } else {
                removed_count += 1;
                keeping = false;
            }
        }
        kept_trimmable.reverse();

        // 重新组装
        let mut result = system_msgs;
        // 如果有裁剪，插入一条摘要提示
        if removed_count > 0 {
            result.push(crate::message::Message {
                role: crate::message::Role::User,
                content: vec![crate::message::ContentBlock::Text {
                    text: format!(
                        "[System: {} earlier messages were trimmed to fit context window. \
                         The conversation continues from the most recent messages below.]",
                        removed_count
                    ),
                }],
            });
        }
        result.extend(kept_trimmable);
        result.extend(protected.to_vec());

        TrimResult {
            messages: result,
            was_trimmed: removed_count > 0,
            removed_count,
            summary: None,
        }
    }
}
```

#### 2.2.5 配置接入

```rust
// crates/nova-core/src/config.rs — 新增 TrimmerConfig

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GatewayConfig {
    // ... 已有字段 ...

    /// 历史裁剪配置
    #[serde(default)]
    pub trimmer: TrimmerConfigToml,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TrimmerConfigToml {
    /// 是否启用历史裁剪
    #[serde(default = "default_trimmer_enabled")]
    pub enabled: bool,
    /// 模型上下文窗口大小
    #[serde(default = "default_context_window")]
    pub context_window: usize,
    /// 输出预留 token 数
    #[serde(default = "default_output_reserve")]
    pub output_reserve: usize,
    /// 最少保留的最近消息数
    #[serde(default = "default_min_recent")]
    pub min_recent_messages: usize,
}

fn default_trimmer_enabled() -> bool { true }
fn default_context_window() -> usize { 128_000 }
fn default_output_reserve() -> usize { 8_192 }
fn default_min_recent() -> usize { 10 }

impl Default for TrimmerConfigToml {
    fn default() -> Self {
        Self {
            enabled: default_trimmer_enabled(),
            context_window: default_context_window(),
            output_reserve: default_output_reserve(),
            min_recent_messages: default_min_recent(),
        }
    }
}
```

对应的 config.toml 配置：

```toml
[gateway.trimmer]
enabled = true
context_window = 128000
output_reserve = 8192
min_recent_messages = 10
```

#### 2.2.6 接入 agent.rs trim_history()

```rust
// crates/nova-core/src/agent.rs — 修改 trim_history()

fn trim_history(
    &self,
    current_history: &Arc<Vec<Message>>,
    _active_skill: &Option<ActiveSkillState>,
    system_prompt: &str,
    trimmer_config: Option<&TrimmerConfig>,
) -> Result<Arc<Vec<Message>>> {
    match trimmer_config {
        Some(config) => {
            let trimmer = HistoryTrimmer::new(config.clone());
            let result = trimmer.trim(current_history, system_prompt);
            if result.was_trimmed {
                log::info!(
                    "History trimmed: removed {} messages to fit context window",
                    result.removed_count
                );
            }
            Ok(Arc::new(result.messages))
        }
        None => Ok(current_history.clone()),
    }
}
```

---

## 三、G10 — 侧信道注入

### 3.1 问题描述

Claude Code 使用 `<system-reminder>` 标签在 tool result 中嵌入系统级提醒。这些提醒随对话进展自动刷新，包含：

- 可用 skill 列表（每次 tool result 后追加）
- CLAUDE.md 项目说明
- 当前日期等动态信息

当前项目没有类似机制。tool result 的内容仅包含工具输出，不附带任何系统上下文。

### 3.2 设计方案

#### 3.2.1 设计原则

- 侧信道内容作为 tool result 的附加文本注入，不修改 system prompt
- 只在必要时注入（不是每次 tool result 都附加）
- 注入内容轻量（保持 token 效率）
- 可配置开关

#### 3.2.2 新增 SideChannelInjector

```rust
// crates/nova-core/src/prompt.rs — 新增

/// 侧信道注入配置。
#[derive(Debug, Clone)]
pub struct SideChannelConfig {
    /// 是否启用侧信道
    pub enabled: bool,
    /// 注入 skill 列表的间隔（每 N 次 tool result 注入一次）
    pub skill_reminder_interval: usize,
    /// 是否注入当前日期
    pub inject_date: bool,
    /// 自定义注入内容
    pub custom_reminders: Vec<String>,
}

impl Default for SideChannelConfig {
    fn default() -> Self {
        Self {
            enabled: false, // 默认关闭，逐步启用
            skill_reminder_interval: 5,
            inject_date: true,
            custom_reminders: vec![],
        }
    }
}

/// 侧信道注入器。
pub struct SideChannelInjector {
    config: SideChannelConfig,
    tool_result_counter: std::sync::atomic::AtomicUsize,
}

impl SideChannelInjector {
    pub fn new(config: SideChannelConfig) -> Self {
        Self {
            config,
            tool_result_counter: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// 生成要附加到 tool result 后的侧信道内容。
    ///
    /// 返回 None 表示本次不注入。
    pub fn generate_injection(
        &self,
        skills: &crate::skill::SkillRegistry,
    ) -> Option<String> {
        if !self.config.enabled {
            return None;
        }

        let count = self.tool_result_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // 检查是否到了注入间隔
        if count % self.config.skill_reminder_interval != 0 {
            return None;
        }

        let mut parts = Vec::new();

        // Skill 列表提醒
        if !skills.packages.is_empty() {
            let skill_list: Vec<String> = skills.packages.iter()
                .map(|p| format!("- {}: {}", p.slug, p.description))
                .collect();
            parts.push(format!(
                "<system-reminder>\nAvailable skills:\n{}\n\nUse /skill-<name> to activate.\n</system-reminder>",
                skill_list.join("\n")
            ));
        }

        // 日期提醒
        if self.config.inject_date {
            let date = chrono::Local::now().format("%Y-%m-%d").to_string();
            parts.push(format!(
                "<system-reminder>\nCurrent date: {}\n</system-reminder>",
                date
            ));
        }

        // 自定义提醒
        for reminder in &self.config.custom_reminders {
            parts.push(format!(
                "<system-reminder>\n{}\n</system-reminder>",
                reminder
            ));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        }
    }

    /// 将侧信道内容附加到 tool result 输出后面。
    pub fn inject_into_tool_result(
        &self,
        tool_output: &str,
        skills: &crate::skill::SkillRegistry,
    ) -> String {
        match self.generate_injection(skills) {
            Some(injection) => format!("{}\n\n{}", tool_output, injection),
            None => tool_output.to_string(),
        }
    }
}
```

#### 3.2.3 接入 agent.rs 工具执行流程

```rust
// crates/nova-core/src/agent.rs — 在 run_turn() 的工具执行部分

// 在工具结果处理中注入侧信道（约 agent.rs:283-305）
let (content, is_error) = match result {
    Ok(Ok(out)) => {
        let content = if let Some(ref injector) = self.side_channel_injector {
            if let Some(ref sr) = self.skill_registry {
                injector.inject_into_tool_result(&out.content, sr)
            } else {
                out.content
            }
        } else {
            out.content
        };
        (content, out.is_error)
    }
    Ok(Err(e)) => (format!("Internal execution error: {}", e), true),
    Err(_) => ("Tool execution timed out".to_string(), true),
};
```

#### 3.2.4 AgentRuntime 新增字段

```rust
// crates/nova-core/src/agent.rs

pub struct AgentRuntime<C: LlmClient> {
    client: C,
    tools: ToolRegistry,
    config: AgentConfig,
    pub task_store: Option<Arc<tokio::sync::Mutex<crate::tool::builtin::task::TaskStore>>>,
    pub skill_registry: Option<Arc<crate::skill::SkillRegistry>>,
    pub read_files: Arc<tokio::sync::Mutex<std::collections::HashSet<String>>>,
    pub side_channel_injector: Option<SideChannelInjector>,  // 新增
}
```

### 3.3 注意事项

- 侧信道内容使用 `<system-reminder>` XML 标签包裹，模型需要被训练或 prompt 中需要说明如何处理此标签
- 在 agent prompt 模板中需要添加对 `<system-reminder>` 标签的说明
- 默认关闭此功能，通过配置逐步启用

---

## 四、G11 — prepare_turn 接入主流程

### 4.1 问题描述

`agent.rs` 中定义了两套 turn 执行路径：

1. `run_turn()`（行 74-351）：旧路径，被 `ConversationService` 实际调用。功能完整。
2. `run_turn_with_context()`（行 400-493）：新路径，基于 `TurnContext`。但存在关键缺陷：
   - **工具执行逻辑缺失**（行 468-474）：只将 tool_calls 记录为 `ContentBlock::ToolUse`，但不执行工具
   - **usage 统计失效**（行 413）：`cumulative_usage` 初始化后不再更新
   - **无 MaxTokens 自动续写**：缺少 stop_reason 检查和续写逻辑
   - **无 cancellation 支持**：缺少 CancellationToken 检查

### 4.2 设计方案

分两步：先修复 `run_turn_with_context()`，再让 `ConversationService` 切换到新路径。

#### 4.2.1 修复 run_turn_with_context()

核心思路：将 `run_turn()` 中的工具执行逻辑提取为共享方法，两套路径复用。

```rust
// crates/nova-core/src/agent.rs — 新增共享方法

/// 执行一组工具调用并返回结果。
async fn execute_tool_calls(
    &self,
    parsed_tool_calls: Vec<(String, String, serde_json::Value)>,
    event_tx: &mpsc::Sender<crate::event::AgentEvent>,
    cancellation_token: &Option<CancellationToken>,
) -> Result<Vec<ContentBlock>> {
    let mut tool_results_fut = FuturesUnordered::new();

    for (call_idx, (id, name, input_val)) in parsed_tool_calls.into_iter().enumerate() {
        let tool_registry = &self.tools;
        let tx = event_tx.clone();
        let tool_timeout_duration = self.config.tool_timeout;

        tool_results_fut.push(async move {
            let _ = tx
                .send(crate::event::AgentEvent::ToolStart {
                    id: id.clone(),
                    name: name.clone(),
                    input: input_val.clone(),
                })
                .await;

            let result = timeout(
                tool_timeout_duration,
                tool_registry.execute(
                    &name,
                    input_val,
                    Some(crate::tool::ToolContext {
                        event_tx: tx.clone(),
                        tool_use_id: id.clone(),
                        task_store: self.task_store.clone(),
                        skill_registry: self.skill_registry.clone(),
                        read_files: self.read_files.clone(),
                    }),
                ),
            )
            .await;

            let (content, is_error) = match result {
                Ok(Ok(out)) => (out.content, out.is_error),
                Ok(Err(e)) => (format!("Internal execution error: {}", e), true),
                Err(_) => ("Tool execution timed out".to_string(), true),
            };

            let _ = tx
                .send(crate::event::AgentEvent::ToolEnd {
                    id: id.clone(),
                    name: name.clone(),
                    output: content.clone(),
                    is_error,
                })
                .await;

            (
                call_idx,
                ContentBlock::ToolResult {
                    tool_use_id: id,
                    output: content,
                    is_error,
                },
            )
        });
    }

    let mut indexed_results = Vec::new();
    while let Some(res) = tool_results_fut.next().await {
        if let Some(token) = cancellation_token {
            if token.is_cancelled() {
                // 返回已收集的结果
                break;
            }
        }
        indexed_results.push(res);
    }
    indexed_results.sort_by_key(|&(idx, _)| idx);

    Ok(indexed_results.into_iter().map(|(_, b)| b).collect())
}
```

#### 4.2.2 重写 run_turn_with_context()

```rust
/// 运行 turn 并使用 TurnContext。
pub async fn run_turn_with_context(
    &self,
    ctx: TurnContext,
    user_message: Message,
    event_tx: mpsc::Sender<crate::event::AgentEvent>,
    cancellation_token: Option<CancellationToken>,
) -> Result<TurnResult> {
    let mut all_messages = Arc::try_unwrap(ctx.history)
        .unwrap_or_else(|h| (*h).clone());

    // 追加用户消息
    all_messages.push(user_message);

    let mut turn_messages = Vec::new();
    let mut cumulative_usage = crate::provider::types::Usage::default();
    let mut completed_naturally = false;

    for iteration in 0..ctx.iteration_budget {
        // 检查取消
        if let Some(ref token) = cancellation_token {
            if token.is_cancelled() {
                return Ok(TurnResult { messages: turn_messages, usage: cumulative_usage });
            }
        }

        let _ = event_tx
            .send(crate::event::AgentEvent::Iteration {
                current: iteration + 1,
                total: ctx.iteration_budget,
            })
            .await;

        // 使用 TurnContext 中的工具定义
        let mut receiver = self
            .client
            .stream(&all_messages, &ctx.tool_definitions, &self.config.model_config)
            .await?;

        let mut current_text = String::new();
        let mut current_thinking = String::new();
        let mut tool_calls: Vec<(String, String, String)> = Vec::new();
        let mut iter_usage = crate::provider::types::Usage::default();
        let mut last_stop_reason: Option<crate::provider::types::StopReason> = None;

        while let Some(event) = receiver.next_event().await? {
            if let Some(ref token) = cancellation_token {
                if token.is_cancelled() {
                    return Ok(TurnResult { messages: turn_messages, usage: cumulative_usage });
                }
            }

            match event {
                ProviderStreamEvent::ThinkingDelta(delta) => {
                    current_thinking.push_str(&delta);
                    let _ = event_tx.send(crate::event::AgentEvent::ThinkingDelta(delta)).await;
                }
                ProviderStreamEvent::TextDelta(delta) => {
                    current_text.push_str(&delta);
                    let _ = event_tx.send(crate::event::AgentEvent::TextDelta(delta)).await;
                }
                ProviderStreamEvent::ToolUseStart { id, name } => {
                    tool_calls.push((id, name, String::new()));
                }
                ProviderStreamEvent::ToolUseInputDelta(delta) => {
                    if let Some(last) = tool_calls.last_mut() {
                        last.2.push_str(&delta);
                    }
                }
                ProviderStreamEvent::MessageComplete { usage, stop_reason } => {
                    iter_usage = usage;
                    last_stop_reason = stop_reason;
                }
                _ => {}
            }
        }

        // 累计 usage
        cumulative_usage.input_tokens += iter_usage.input_tokens;
        cumulative_usage.output_tokens += iter_usage.output_tokens;
        cumulative_usage.cache_creation_input_tokens += iter_usage.cache_creation_input_tokens;
        cumulative_usage.cache_read_input_tokens += iter_usage.cache_read_input_tokens;

        // 构建 assistant message
        let mut current_blocks = Vec::new();
        if !current_thinking.is_empty() {
            current_blocks.push(ContentBlock::Thinking { thinking: current_thinking });
        }
        if !current_text.is_empty() {
            current_blocks.push(ContentBlock::Text { text: current_text });
        }

        let parsed_tool_calls: Vec<(String, String, serde_json::Value)> = tool_calls
            .into_iter()
            .map(|(id, name, input_json)| {
                let input_val = serde_json::from_str(&input_json).unwrap_or_else(|e| {
                    log::warn!("Failed to parse tool input JSON: {}", e);
                    serde_json::json!({ "__error": format!("Invalid JSON: {}", e) })
                });
                (id, name, input_val)
            })
            .collect();

        for (id, name, input_val) in &parsed_tool_calls {
            current_blocks.push(ContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input_val.clone(),
            });
        }

        let assistant_msg = Message { role: Role::Assistant, content: current_blocks };
        all_messages.push(assistant_msg.clone());
        turn_messages.push(assistant_msg);

        // MaxTokens 自动续写
        if last_stop_reason == Some(crate::provider::types::StopReason::MaxTokens) {
            let is_truncated = if parsed_tool_calls.is_empty() {
                true
            } else {
                parsed_tool_calls.last().unwrap().2.get("__error").is_some()
            };
            if is_truncated {
                all_messages.push(Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "Please continue your last tool call or response.".to_string(),
                    }],
                });
                continue;
            }
        }

        if parsed_tool_calls.is_empty() {
            completed_naturally = true;
            break;
        }

        // 执行工具调用（使用共享方法）
        let tool_result_blocks = self.execute_tool_calls(
            parsed_tool_calls,
            &event_tx,
            &cancellation_token,
        ).await?;

        let tool_res_msg = Message { role: Role::User, content: tool_result_blocks };
        all_messages.push(tool_res_msg.clone());
        turn_messages.push(tool_res_msg);
    }

    if !completed_naturally {
        let _ = event_tx
            .send(crate::event::AgentEvent::IterationLimitReached {
                iterations: ctx.iteration_budget,
            })
            .await;
    }

    Ok(TurnResult { messages: turn_messages, usage: cumulative_usage })
}
```

#### 4.2.3 同步重构 run_turn()

将 `run_turn()` 中的工具执行逻辑替换为 `execute_tool_calls()` 调用，减少代码重复：

```rust
// agent.rs run_turn() 中，替换行 251-330 的工具执行代码为：
let tool_result_blocks = self.execute_tool_calls(
    parsed_tool_calls,
    &event_tx,
    &cancellation_token,
).await?;

let tool_res_msg = Message { role: Role::User, content: tool_result_blocks };
all_messages.push(tool_res_msg.clone());
turn_messages.push(tool_res_msg);
```

### 4.3 ConversationService 切换

切换 `ConversationService` 的调用路径从 `run_turn()` 到 `prepare_turn()` + `run_turn_with_context()`。

#### 4.3.1 ConversationService 修改

```rust
// crates/nova-app/src/conversation_service.rs — 修改 start_turn 方法

// 改前：
let result = self.agent.run_turn(
    &history_for_turn,
    &input,
    event_tx,
    Some(cancellation_token),
).await?;

// 改后：
let prompt_config = self.build_prompt_config(&agent_descriptor, &session)?;
let ctx = self.agent.prepare_turn(&input, Arc::new(history_for_turn), &prompt_config)?;
let user_message = Message {
    role: Role::User,
    content: vec![ContentBlock::Text { text: input.clone() }],
};
let result = self.agent.run_turn_with_context(
    ctx,
    user_message,
    event_tx,
    Some(cancellation_token),
).await?;
```

#### 4.3.2 ConversationService 新增辅助方法

```rust
impl ConversationService {
    fn build_prompt_config(
        &self,
        agent: &AgentDescriptor,
        _session: &Session,
    ) -> PromptConfig {
        PromptConfig::new(
            agent.id.clone(),
            agent.system_prompt_template.clone(),
            self.workspace_path.clone(),
        )
        // 未来可以从 session 中获取 active_skill、workflow_stage 等
    }
}
```

### 4.4 渐进切换策略

为降低风险，可以通过配置控制使用哪条路径：

```toml
[gateway]
# 使用新的 prepare_turn + run_turn_with_context 路径
use_turn_context = false  # 默认 false，手动启用
```

```rust
// ConversationService.start_turn()
if self.config.gateway.use_turn_context {
    // 新路径
    let ctx = self.agent.prepare_turn(...)?;
    self.agent.run_turn_with_context(ctx, ...).await?
} else {
    // 旧路径
    self.agent.run_turn(...).await?
}
```

---

## 五、Workflow 阶段 Prompt 加载（附加目标）

### 5.1 问题描述

`workflow-stages.md` 定义了 7 个阶段的 prompt 模板（GatherRequirements、Discover、AwaitSelection 等），但代码中没有加载和注入该文件的逻辑。`agent.rs:541` 中 `workflow_section("")` 永远传入空字符串。

### 5.2 设计方案

#### 5.2.1 解析 workflow-stages.md

```rust
// crates/nova-core/src/prompt.rs — 新增

use std::collections::HashMap;

/// 工作流阶段 prompt 集合。
pub struct WorkflowStagePrompts {
    /// 阶段名称 → prompt 内容
    stages: HashMap<String, String>,
}

impl WorkflowStagePrompts {
    /// 从 workflow-stages.md 文件加载。
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut stages = HashMap::new();
        let mut current_stage: Option<String> = None;
        let mut current_content = String::new();
        let mut in_code_block = false;

        for line in content.lines() {
            if line.starts_with("## ") && !in_code_block {
                // 保存上一个阶段
                if let Some(stage) = current_stage.take() {
                    let trimmed = current_content.trim().to_string();
                    if !trimmed.is_empty() {
                        stages.insert(stage, trimmed);
                    }
                }
                current_stage = Some(line[3..].trim().to_string());
                current_content.clear();
            } else {
                if line.starts_with("```") {
                    in_code_block = !in_code_block;
                    // 不包含 ``` 围栏本身
                } else if in_code_block {
                    current_content.push_str(line);
                    current_content.push('\n');
                }
            }
        }
        // 保存最后一个阶段
        if let Some(stage) = current_stage {
            let trimmed = current_content.trim().to_string();
            if !trimmed.is_empty() {
                stages.insert(stage, trimmed);
            }
        }

        Ok(Self { stages })
    }

    /// 获取指定阶段的 prompt 模板。
    pub fn get(&self, stage: &str) -> Option<&str> {
        self.stages.get(stage).map(|s| s.as_str())
    }

    /// 获取指定阶段的 prompt，并用变量替换占位符。
    pub fn render(&self, stage: &str, vars: &HashMap<String, String>) -> Option<String> {
        self.get(stage).map(|template| {
            TemplateContext::render(template, vars)
        })
    }
}
```

#### 5.2.2 在 from_config() 中注入 workflow prompt

```rust
// from_config() 内——当有 workflow_stage 模板变量时

if let Some(stage) = config.template_vars.get("workflow_stage") {
    if stage != "idle" {
        // 加载 workflow 阶段 prompt
        let workflow_path = config.workspace_path.join("prompts").join("workflow-stages.md");
        if let Ok(stages) = WorkflowStagePrompts::load_from_file(&workflow_path) {
            if let Some(rendered) = stages.render(stage, &config.template_vars) {
                builder = builder.workflow_section(&rendered);
            }
        }
    }
}
```

---

## 六、完整变更清单

| 文件 | 变更类型 | 变更说明 |
|------|----------|----------|
| `prompt.rs` | 新增 | `TrimmerConfig` 结构体 |
| `prompt.rs` | 新增 | `HistoryTrimmer` 结构体及 `trim()` 方法 |
| `prompt.rs` | 新增 | `TrimResult` 结构体 |
| `prompt.rs` | 新增 | `SideChannelConfig` 和 `SideChannelInjector` |
| `prompt.rs` | 新增 | `WorkflowStagePrompts` 和加载/渲染方法 |
| `agent.rs` | 新增 | `execute_tool_calls()` 共享方法 |
| `agent.rs` | 重写 | `run_turn_with_context()` 补全工具执行和 usage |
| `agent.rs` | 修改 | `run_turn()` 提取工具执行为 `execute_tool_calls()` |
| `agent.rs` | 修改 | `trim_history()` 接入 `HistoryTrimmer` |
| `agent.rs` | 修改 | `prepare_turn()` 传入 trimmer config |
| `agent.rs` | 新增 | `side_channel_injector` 字段 |
| `config.rs` | 新增 | `TrimmerConfigToml` 及相关默认值 |
| `config.rs` | 修改 | `GatewayConfig` 新增 `trimmer` 和 `use_turn_context` 字段 |
| `conversation_service.rs` | 修改 | 切换到 `prepare_turn` + `run_turn_with_context` 路径 |
| `conversation_service.rs` | 新增 | `build_prompt_config()` 辅助方法 |

---

## 七、测试计划

### 7.1 单元测试

| 测试 | 文件 | 说明 |
|------|------|------|
| `trim_no_op_when_under_budget` | prompt.rs | 总 token 在预算内时不裁剪 |
| `trim_removes_oldest_messages` | prompt.rs | 超出预算时从最旧消息开始移除 |
| `trim_preserves_system_message` | prompt.rs | System 消息永远保留 |
| `trim_preserves_recent_messages` | prompt.rs | 最近 N 条消息不被裁剪 |
| `trim_inserts_truncation_notice` | prompt.rs | 裁剪后插入提示消息 |
| `estimate_tokens_rough_accuracy` | prompt.rs | token 估算误差在 2x 以内 |
| `side_channel_disabled_returns_none` | prompt.rs | 关闭时不注入 |
| `side_channel_interval_skips` | prompt.rs | 非注入间隔时返回 None |
| `side_channel_includes_skills` | prompt.rs | 注入内容包含 skill 列表 |
| `workflow_stages_load_all` | prompt.rs | 正确解析 7 个阶段 |
| `workflow_stages_render_vars` | prompt.rs | 占位符被正确替换 |
| `execute_tool_calls_returns_ordered` | agent.rs | 工具结果保持调用顺序 |

### 7.2 集成测试

| 测试 | 说明 |
|------|------|
| `run_turn_with_context` 执行工具并返回结果 | 修复后的工具执行验证 |
| `run_turn_with_context` usage 统计非零 | usage 累计验证 |
| `run_turn_with_context` MaxTokens 触发续写 | 续写逻辑验证 |
| `prepare_turn` 产生完整 TurnContext | 上下文构建验证 |
| 历史超 budget 时自动裁剪并标注 | 裁剪 e2e 验证 |

### 7.3 运行验证

```bash
cargo clippy --workspace -- -D warnings
cargo fmt --all
cargo test --workspace
```

---

## 八、风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| Token 估算误差导致裁剪不准 | 中 | 使用保守估算（chars/3），后续接入精确 tokenizer |
| 裁剪破坏 tool_use/tool_result 配对 | 高 | 裁剪单位为完整消息而非 content block，且保护最近 N 条 |
| 切换到 run_turn_with_context 导致行为回归 | 高 | 通过 `use_turn_context` 配置开关渐进切换 |
| 侧信道的 `<system-reminder>` 标签被非 Anthropic 模型忽略 | 中 | 在 agent prompt 中显式说明标签含义 |
| WorkflowStagePrompts 文件路径硬编码 | 低 | 通过 config.toml 的 prompts_dir 配置间接控制 |
| `execute_tool_calls` 提取后 self borrow 冲突 | 中 | 方法使用 `&self` 不可变借用，工具执行通过 registry 进行 |
