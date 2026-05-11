use super::AgentRuntime;
use crate::message::{ContentBlock, Message, Role};
use crate::prompt::{PromptSectionSize, SystemPromptBuilder, ToolSize};
use crate::provider::types::ToolDefinition;
use crate::provider::LlmClient;

#[derive(Debug, Clone)]
struct MessageSize {
    index: usize,
    role: Role,
    chars: usize,
    tool_calls: usize,
    tool_result_chars: usize,
    has_large_tool_result: bool,
    is_empty_assistant: bool,
    is_large: bool,
}

impl<C: LlmClient> AgentRuntime<C> {
    pub(super) fn log_prompt_diagnostics(&self, builder: &SystemPromptBuilder, tool_definitions: &[ToolDefinition]) {
        let cfg = &self.config.prompt_diagnostics;
        if !cfg.enabled {
            return;
        }
        let section_reports = builder.size_report(cfg.large_section_chars);
        let tool_reports = SystemPromptBuilder::tool_size_report(tool_definitions);
        let tools_chars = tool_reports.iter().map(|r| r.chars).sum::<usize>();
        let system_chars = section_reports.iter().map(|r| r.chars).sum::<usize>();
        log::info!("Prompt size summary: system={}, tools={}", system_chars, tools_chars);

        for PromptSectionSize {
            name, chars, is_large, ..
        } in &section_reports
        {
            if *is_large {
                log::warn!("Large section: {:?} chars={}", name, chars);
            }
        }
        for ToolSize { name, chars } in &tool_reports {
            log::debug!("Tool schema size: {} chars={}", name, chars);
        }
    }

    pub(super) fn log_history_diagnostics(&self, history: &[Message]) {
        let cfg = &self.config.prompt_diagnostics;
        if !cfg.enabled {
            return;
        }
        let reports = history
            .iter()
            .enumerate()
            .map(|(index, message)| self.build_message_size(index, message))
            .collect::<Vec<_>>();
        let history_chars = reports.iter().map(|r| r.chars).sum::<usize>();
        log::info!(
            "History size summary: total_chars={}, messages={}",
            history_chars,
            reports.len()
        );
        for report in reports {
            log::debug!(
                "History message: index={} role={:?} chars={} tool_calls={} tool_result_chars={}",
                report.index,
                report.role,
                report.chars,
                report.tool_calls,
                report.tool_result_chars
            );
            if report.is_large {
                log::warn!(
                    "Large message: index={} role={:?} chars={}",
                    report.index,
                    report.role,
                    report.chars
                );
            }
            if report.has_large_tool_result {
                log::warn!(
                    "Large tool result message: index={} role={:?} tool_result_chars={}",
                    report.index,
                    report.role,
                    report.tool_result_chars
                );
            }
            if report.is_empty_assistant {
                log::warn!("Empty assistant message: index={}", report.index);
            }
        }
    }

    fn build_message_size(&self, index: usize, message: &Message) -> MessageSize {
        let cfg = &self.config.prompt_diagnostics;
        let mut chars = 0usize;
        let mut tool_calls = 0usize;
        let mut tool_result_chars = 0usize;
        for block in &message.content {
            match block {
                ContentBlock::Text { text } => chars += text.chars().count(),
                ContentBlock::Thinking { thinking } => chars += thinking.chars().count(),
                ContentBlock::ToolUse { .. } => {
                    tool_calls += 1;
                }
                ContentBlock::ToolResult { output, .. } => {
                    let c = output.chars().count();
                    chars += c;
                    tool_result_chars += c;
                }
            }
        }
        let is_empty_assistant = matches!(message.role, Role::Assistant) && chars == 0 && tool_calls == 0;
        MessageSize {
            index,
            role: message.role.clone(),
            chars,
            tool_calls,
            tool_result_chars,
            has_large_tool_result: tool_result_chars > cfg.large_tool_result_chars,
            is_empty_assistant,
            is_large: chars > cfg.large_message_chars,
        }
    }

    pub(super) fn compact_tool_output(&self, tool_name: &str, is_error: bool, output: &str) -> String {
        let cfg = &self.config.tool_result_compaction;
        if !cfg.enabled || cfg.disable_for_tools.contains(&tool_name.to_ascii_lowercase()) {
            return output.to_string();
        }

        let total_chars = output.chars().count();
        if total_chars <= cfg.max_chars {
            return output.to_string();
        }

        let head = output.chars().take(cfg.head_chars).collect::<String>();
        let tail = output
            .chars()
            .skip(total_chars.saturating_sub(cfg.tail_chars))
            .collect::<String>();
        let total_lines = output.lines().count();

        format!(
            "[Tool output compacted]\nTool: {tool_name}\nIs error: {is_error}\nOriginal chars: {total_chars}\nOriginal lines: {total_lines}\nKept head chars: {}\nKept tail chars: {}\nReason: output exceeded {} chars\n\n--- head ---\n{}\n\n--- tail ---\n{}\n\n[Full output omitted from model context. Re-run a narrower command or read a specific range if needed.]",
            head.chars().count(),
            tail.chars().count(),
            cfg.max_chars,
            head,
            tail
        )
    }
}
