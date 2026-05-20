use crate::message::{ContentBlock, Message, Role};

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
    pub messages: Vec<Message>,
    /// 是否发生了裁剪
    pub was_trimmed: bool,
    /// 被移除的消息数量
    pub removed_count: usize,
    /// 摘要文本（如果启用了摘要）
    pub summary: Option<String>,
}

impl HistoryTrimmer {
    pub fn new(config: TrimmerConfig) -> Self {
        Self { config }
    }

    fn estimate_tokens(messages: &[Message]) -> usize {
        let total_chars: usize = messages
            .iter()
            .map(|m| {
                m.content
                    .iter()
                    .map(|block| match block {
                        ContentBlock::Text { text } => text.len(),
                        ContentBlock::Thinking { thinking } => thinking.len(),
                        ContentBlock::ToolUse { name, input, .. } => name.len() + input.to_string().len(),
                        ContentBlock::ToolResult { output, .. } => output.len(),
                        ContentBlock::Image { data_base64, .. } => data_base64.len(),
                    })
                    .sum::<usize>()
            })
            .sum();

        total_chars / 3
    }

    fn estimate_system_prompt_tokens(system_prompt: &str) -> usize {
        system_prompt.len() / 3
    }

    pub fn trim(&self, messages: &[Message], system_prompt: &str) -> TrimResult {
        let system_tokens = Self::estimate_system_prompt_tokens(system_prompt);
        let history_budget = self
            .config
            .context_window
            .saturating_sub(system_tokens)
            .saturating_sub(self.config.output_reserve);

        let (system_msgs, conversation_msgs): (Vec<_>, Vec<_>) = messages.iter().partition(|m| m.role == Role::System);

        let system_msgs: Vec<_> = system_msgs.into_iter().cloned().collect();
        let conversation_msgs: Vec<_> = conversation_msgs.into_iter().cloned().collect();
        let current_tokens = Self::estimate_tokens(&conversation_msgs);

        if current_tokens <= history_budget {
            return TrimResult {
                messages: messages.to_vec(),
                was_trimmed: false,
                removed_count: 0,
                summary: None,
            };
        }

        let protected_count = self.config.min_recent_messages.min(conversation_msgs.len());
        let trimmable = &conversation_msgs[..conversation_msgs.len() - protected_count];
        let protected = &conversation_msgs[conversation_msgs.len() - protected_count..];

        let protected_tokens = Self::estimate_tokens(protected);
        let mut remaining_budget = history_budget.saturating_sub(protected_tokens);

        let mut kept_trimmable = Vec::new();
        for msg in trimmable.iter().rev() {
            let msg_tokens = Self::estimate_tokens(std::slice::from_ref(msg));
            if msg_tokens <= remaining_budget {
                remaining_budget -= msg_tokens;
                kept_trimmable.push(msg.clone());
            } else {
                break;
            }
        }
        kept_trimmable.reverse();
        let removed_count = trimmable.len().saturating_sub(kept_trimmable.len());

        let mut result = system_msgs;
        if removed_count > 0 {
            result.push(Message::new(
                Role::User,
                vec![ContentBlock::Text {
                    text: format!(
                        "[System: {} earlier messages were trimmed to fit context window. The conversation continues from the most recent messages below.]",
                        removed_count
                    ),
                }],
                chrono::Utc::now().timestamp_millis(),
            ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_no_op_when_under_budget() {
        let trimmer = HistoryTrimmer::new(TrimmerConfig {
            context_window: 1_000,
            output_reserve: 100,
            min_recent_messages: 2,
            enable_summary: false,
        });
        let now = chrono::Utc::now().timestamp_millis();
        let messages = vec![
            Message::new(
                Role::System,
                vec![ContentBlock::Text {
                    text: "system".to_string(),
                }],
                now,
            ),
            Message::new(
                Role::User,
                vec![ContentBlock::Text {
                    text: "short".to_string(),
                }],
                now,
            ),
        ];

        let result = trimmer.trim(&messages, "system");
        assert!(!result.was_trimmed);
        assert_eq!(result.messages.len(), 2);
    }
}
