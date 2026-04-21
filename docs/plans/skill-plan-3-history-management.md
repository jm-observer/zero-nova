# Plan 3：历史消息管理与摘要压缩

> 前置依赖：Plan 1（可并行开发，集成时依赖 Plan 2） | 预期产出：`src/skill/history.rs`

## 1. 目标

当 skill 切换时，对旧 skill 的对话历史做摘要压缩，避免 token 浪费和上下文污染，同时保留必要的跨 skill 引用信息。

## 2. 问题分析

### 2.1 不处理历史的代价

假设一次会话中发生了 skill 切换：

```
Turn 1-5: commit skill (讨论代码提交)
  - 5 条 user message
  - 5 条 assistant message
  - 12 个 tool_use/tool_result 对（git status, git diff, git add ...）
  - 约 8000 tokens

Turn 6: review skill (用户要求审查代码)
  → 如果把 Turn 1-5 全部带入，8000 tokens 中大部分对 review 无用
  → commit skill 的工具调用细节（git diff 输出等）尤其冗余
```

### 2.2 完全清空的代价

```
Turn 5: "已提交 src/main.rs 的修改"
Turn 6: "刚才提交的那个文件，帮我审查一下"
  → 如果清空历史，LLM 不知道 "刚才提交的那个文件" 是什么
```

## 3. 设计方案

### 3.1 分段模型

将消息历史按 skill 切换点分成若干**段（Segment）**：

```
messages: [
  ── Segment 1 (commit skill) ──
  user: "帮我提交代码"
  assistant: [tool_use: bash "git status"]
  user: [tool_result: "M src/main.rs"]
  assistant: "检测到 src/main.rs 有修改，提交信息建议..."
  user: "就用这个信息提交吧"
  assistant: [tool_use: bash "git commit ..."]
  user: [tool_result: "1 file changed"]
  assistant: "已成功提交 src/main.rs 的修改"

  ── Segment 2 (review skill) ──   ← 当前段
  user: "刚才提交的那个文件，帮我审查一下"
  ...
]
```

### 3.2 摘要策略

当 skill 切换时，对**之前所有段**做摘要压缩：

```
摘要后的 messages: [
  ── 摘要消息 ──
  user: "[上下文摘要] 用户通过 commit skill 提交了 src/main.rs 的修改。提交信息为：fix: 修复主入口的初始化逻辑。"
  assistant: "好的，我了解之前的上下文。"

  ── 当前段（完整保留） ──
  user: "刚才提交的那个文件，帮我审查一下"
  ...
]
```

## 4. 数据结构

### 4.1 历史快照

```rust
// src/skill/history.rs

use crate::message::{ContentBlock, Message, Role};

/// 一个 skill 段的元信息
#[derive(Debug, Clone)]
pub struct HistorySegment {
    /// 该段对应的 skill slug（None = 无 skill）
    pub skill_slug: Option<String>,
    /// 该段的起始消息索引（在完整历史中的位置）
    pub start_index: usize,
    /// 该段的消息数量
    pub message_count: usize,
}

/// 历史管理上下文
pub struct HistoryManager {
    /// 分段信息
    segments: Vec<HistorySegment>,
}
```

### 4.2 摘要生成

```rust
impl HistoryManager {
    /// 根据 skill 切换情况，准备本轮的消息历史
    ///
    /// - 如果 skill 没有切换，返回完整历史
    /// - 如果 skill 切换了，对旧段做摘要，新段保留完整
    pub fn prepare_history(
        &mut self,
        full_history: &[Message],
        previous_skill: Option<&str>,
        current_skill: Option<&str>,
    ) -> Vec<Message> {
        let skill_switched = previous_skill != current_skill;

        if !skill_switched || full_history.is_empty() {
            return full_history.to_vec();
        }

        // 找到最后一个 skill 切换点
        let current_segment_start = self.current_segment_start();

        let old_messages = &full_history[..current_segment_start];
        let new_messages = &full_history[current_segment_start..];

        let mut result = Vec::new();

        // 旧历史 → 规则摘要
        if !old_messages.is_empty() {
            let summary = self.summarize_rule_based(old_messages);
            result.push(Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: format!("[上下文摘要] {}", summary),
                }],
            });
            result.push(Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: "好的，我了解之前的上下文。".to_string(),
                }],
            });
        }

        // 新段 → 完整保留
        result.extend(new_messages.iter().cloned());

        result
    }
}
```

## 5. 规则摘要算法

### 5.1 策略

规则摘要不调用 LLM，而是通过提取关键信息来生成摘要：

```rust
impl HistoryManager {
    /// 规则摘要：从历史消息中提取关键信息
    fn summarize_rule_based(&self, messages: &[Message]) -> String {
        let mut user_messages = Vec::new();
        let mut assistant_conclusions = Vec::new();
        let mut tool_names_used: Vec<String> = Vec::new();

        for msg in messages {
            match msg.role {
                Role::User => {
                    // 只提取用户的文本消息（跳过 tool_result）
                    for block in &msg.content {
                        if let ContentBlock::Text { text } = block {
                            user_messages.push(text.clone());
                        }
                    }
                }
                Role::Assistant => {
                    // 提取最后一条 assistant 文本作为结论
                    for block in &msg.content {
                        match block {
                            ContentBlock::Text { text } => {
                                assistant_conclusions.push(text.clone());
                            }
                            ContentBlock::ToolUse { name, .. } => {
                                if !tool_names_used.contains(name) {
                                    tool_names_used.push(name.clone());
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        let mut summary = String::new();

        // 用户意图概述
        if !user_messages.is_empty() {
            summary.push_str("用户请求：");
            // 只取第一条和最后一条用户消息
            summary.push_str(&user_messages[0]);
            if user_messages.len() > 1 {
                summary.push_str(&format!(
                    " ... 经过{}轮对话 ... ",
                    user_messages.len()
                ));
                summary.push_str(user_messages.last().unwrap());
            }
            summary.push_str("。");
        }

        // 使用的工具
        if !tool_names_used.is_empty() {
            summary.push_str(&format!(
                " 使用了工具：{}。",
                tool_names_used.join(", ")
            ));
        }

        // 最终结论
        if let Some(last_conclusion) = assistant_conclusions.last() {
            // 截取最后结论的前 200 字符
            let truncated = if last_conclusion.len() > 200 {
                format!("{}...", &last_conclusion[..200])
            } else {
                last_conclusion.clone()
            };
            summary.push_str(&format!(" 最终结果：{}", truncated));
        }

        summary
    }
}
```

### 5.2 摘要示例

**原始历史（8 条消息，约 8000 tokens）：**
```
user: "帮我提交代码"
assistant: [tool_use: bash "git status"]
user: [tool_result: "M src/main.rs\nM src/config.rs"]
assistant: "检测到 2 个文件有修改..."
user: "只提交 main.rs"
assistant: [tool_use: bash "git add src/main.rs && git commit -m 'fix: ...'"]
user: [tool_result: "1 file changed, 15 insertions(+)"]
assistant: "已成功提交 src/main.rs。提交信息：fix: 修复主入口的初始化逻辑"
```

**摘要后（2 条消息，约 150 tokens）：**
```
user: "[上下文摘要] 用户请求：帮我提交代码 ... 经过2轮对话 ... 只提交 main.rs。使用了工具：bash。最终结果：已成功提交 src/main.rs。提交信息：fix: 修复主入口的初始化逻辑"
assistant: "好的，我了解之前的上下文。"
```

**压缩比：约 98%**

## 6. Session 改造

### 6.1 新增字段

```rust
// src/gateway/session.rs

pub struct Session {
    pub id: String,
    pub name: String,
    pub history: RwLock<Vec<Message>>,
    pub created_at: i64,
    pub chat_lock: Mutex<()>,
    pub active_skill: RwLock<Option<String>>,        // Plan 2 新增
    pub skill_switch_points: RwLock<Vec<usize>>,     // 新增：skill 切换点索引
}
```

### 6.2 切换点追踪

每次 skill 切换时，记录当前历史的长度作为切换点：

```rust
// 在 handle_chat 中
if skill_switched {
    let history_len = session.history.read().unwrap().len();
    session.skill_switch_points.write().unwrap().push(history_len);
}
```

## 7. LLM 摘要（高级策略，可选）

如果配置 `history_strategy = "llm"`，使用 LLM 生成更高质量的摘要：

```rust
/// LLM 摘要策略
async fn summarize_with_llm(
    client: &impl LlmClient,
    messages: &[Message],
) -> Result<String> {
    let prompt = "请用 2-3 句话总结以下对话的核心内容和结果，保留关键信息（文件名、操作结果等）：";

    // 将历史消息序列化为文本
    let conversation_text = messages_to_text(messages);

    let summary_messages = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: format!("{}\n\n{}", prompt, conversation_text),
        }],
    }];

    // 使用小模型生成摘要
    let response = client.complete(&summary_messages, prompt, &[]).await?;
    Ok(response)
}
```

**注意：** LLM 摘要会引入额外延迟（约 1-2 秒）和成本。建议仅在对话历史特别长（超过 20 轮）时启用。

## 8. 配置

```toml
# config.toml
[skill]
directory = "./skills"
enabled = true
# 历史摘要策略
# "rule"  — 规则摘要（默认，快速，零成本）
# "llm"   — LLM 摘要（高质量，有延迟和成本）
# "none"  — 不做摘要，完整保留（最大 token 消耗）
history_strategy = "rule"
```

## 9. 实施步骤

| 步骤 | 操作 | 涉及文件 |
|------|------|----------|
| 1 | 实现 `HistorySegment` 和 `HistoryManager` 基础结构 | `src/skill/history.rs` |
| 2 | 实现 `summarize_rule_based()` 规则摘要算法 | `src/skill/history.rs` |
| 3 | 实现 `prepare_history()` 历史准备逻辑 | `src/skill/history.rs` |
| 4 | `Session` 新增 `skill_switch_points` 字段 | `src/gateway/session.rs` |
| 5 | 在 `handle_chat()` 中集成切换检测和历史准备 | `src/gateway/router.rs` |
| 6 | 在 `config.rs` 中新增 `history_strategy` 配置 | `src/config.rs` |
| 7 | （可选）实现 `summarize_with_llm()` | `src/skill/history.rs` |
| 8 | 在 `src/skill/mod.rs` 中导出 | `src/skill/mod.rs` |

## 10. 验证标准

- [ ] 同一 skill 内连续对话，历史完整保留（无压缩）
- [ ] skill 切换时，旧历史被摘要为 2 条消息（user 摘要 + assistant 确认）
- [ ] 摘要内容包含：用户核心请求、使用的工具、最终结论
- [ ] 工具调用的详细输入输出被丢弃，不出现在摘要中
- [ ] 跨 skill 引用能工作（例："刚才提交的文件"能被 LLM 理解）
- [ ] `history_strategy = "none"` 时不做任何压缩
- [ ] 空历史不产生摘要消息
