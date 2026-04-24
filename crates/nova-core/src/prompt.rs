use crate::tool::ToolRegistry;

// ---------------------------------------------------------------------------
//  Section-based prompt building (Plan 1 升级)
// ---------------------------------------------------------------------------

/// 系统提示词具名 section 名称。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SectionName {
    Base,
    Agent,
    Skill,
    Environment,
    Workflow,
    ToolGuidance,
    History,
}

/// Section 注入优先级。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptPriority {
    /// 总是插入
    High,
    /// 条件插入（如 active skill 存在时）
    Medium,
    /// 仅调试或覆盖模式插入
    Low,
}

/// 具名 section，支持独立构造和条件注入。
#[derive(Debug, Clone)]
pub struct NamedSection {
    /// 具名 section 名称
    pub name: SectionName,
    /// 内容
    pub content: String,
    /// 是否必须有内容才注入
    pub required: bool,
    /// 注入优先级
    pub priority: PromptPriority,
}

#[derive(Default)]
/// Builder for constructing system prompts with optional sections.
pub struct SystemPromptBuilder {
    sections: Vec<(SectionName, NamedSection)>,
}

impl SystemPromptBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加一个具名 section。
    pub fn add_section(mut self, name: SectionName, content: impl Into<String>, priority: PromptPriority) -> Self {
        let content_val: String = content.into();
        if !content_val.is_empty() {
            self.sections.push((name.clone(), NamedSection {
                name,
                content: content_val,
                required: priority == PromptPriority::High,
                priority,
            }));
        }
        self
    }

    /// 添加 base section。
    pub fn base_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::Base, content, PromptPriority::High)
    }

    /// 添加 agent section。
    pub fn agent_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::Agent, content, PromptPriority::High)
    }

    /// 添加 skill section。
    pub fn skill_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::Skill, content, PromptPriority::Medium)
    }

    /// 添加 environment section。
    pub fn environment_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::Environment, content, PromptPriority::High)
    }

    /// 添加 workflow section。
    pub fn workflow_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::Workflow, content, PromptPriority::Medium)
    }

    /// 添加 tool guidance section。
    pub fn tool_guidance_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::ToolGuidance, content, PromptPriority::Medium)
    }

    /// 添加 history section。
    pub fn history_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::History, content, PromptPriority::Low)
    }

    /// 保留旧兼容接口 — 添加 role section。
    pub fn role(mut self, role: impl Into<String>) -> Self {
        self.sections.push((
            SectionName::Base,
            NamedSection {
                name: SectionName::Base,
                content: format!("Role: {}", role.into()),
                required: true,
                priority: PromptPriority::High,
            },
        ));
        self
    }

    /// 保留旧兼容接口 — 添加 guideline section。
    pub fn guideline(mut self, text: impl Into<String>) -> Self {
        self.sections.push((
            SectionName::Base,
            NamedSection {
                name: SectionName::Base,
                content: format!("Guideline: {}", text.into()),
                required: true,
                priority: PromptPriority::High,
            },
        ));
        self
    }

    /// 保留旧兼容接口 — 添加 environment 变量。
    pub fn environment(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.sections.push((
            SectionName::Environment,
            NamedSection {
                name: SectionName::Environment,
                content: format!("Environment {} = {}", key.into(), value.into()),
                required: false,
                priority: PromptPriority::Medium,
            },
        ));
        self
    }

    /// 保留旧兼容接口 — 添加 custom instruction。
    pub fn custom_instruction(mut self, text: impl Into<String>) -> Self {
        self.sections.push((
            SectionName::Workflow,
            NamedSection {
                name: SectionName::Workflow,
                content: format!("Instruction: {}", text.into()),
                required: false,
                priority: PromptPriority::Medium,
            },
        ));
        self
    }

    /// 保留旧兼容接口 — 添加 extra section。
    pub fn extra_section(mut self, text: impl Into<String>) -> Self {
        self.sections.push((
            SectionName::Base,
            NamedSection {
                name: SectionName::Base,
                content: text.into(),
                required: false,
                priority: PromptPriority::Low,
            },
        ));
        self
    }

    /// 追加工具描述到现有的 ToolGuidance section。
    pub fn with_tools(mut self, registry: &ToolRegistry) -> Self {
        let mut tool_desc = String::new();
        for def in registry.loaded_definitions() {
            tool_desc.push_str(&format!(
                "## {}\n\n{}\n\nInput schema:\n```json\n{}\n```\n\n---\n\n",
                def.name,
                def.description,
                serde_json::to_string_pretty(&def.input_schema).unwrap_or_else(|_| "{}".to_string())
            ));
        }
        // 追加到 ToolGuidance section 而不是创建新的 section
        if let Some((_, section)) = self.sections.iter_mut().rev().find(|(name, _)| *name == SectionName::ToolGuidance) {
            section.content.push_str(&tool_desc);
        } else {
            self.sections.push((
                SectionName::ToolGuidance,
                NamedSection {
                    name: SectionName::ToolGuidance,
                    content: tool_desc,
                    required: false,
                    priority: PromptPriority::Medium,
                },
            ));
        }
        self
    }

    /// 构建最终 prompt 字符串，按 section 顺序拼接，跳过空值和低优先级的可选 section。
    pub fn build(&self) -> String {
        self.sections
            .iter()
            .filter(|(_, section)| {
                // 跳过空内容
                if section.content.is_empty() {
                    return false;
                }
                // 低优先级 section 仅在非空时包含
                section.priority != PromptPriority::Low || !section.content.is_empty()
            })
            .map(|(_, section)| &section.content)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 返回当前所有 section 的调试信息（用于 CLI `/prompt-sections` 命令）。
    pub fn debug_sections(&self) -> Vec<String> {
        self.sections
            .iter()
            .map(|(name, section)| {
                format!(
                    "{:?}: {} ({:?}, required={})",
                    name,
                    if section.content.is_empty() { "empty" } else { "present" },
                    section.priority,
                    section.required
                )
            })
            .collect()
    }

    /// 返回指定名称的 section 内容（用于调试）。
    pub fn get_section(&self, name: &SectionName) -> Option<&str> {
        self.sections
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, section)| section.content.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_builder_produces_empty_string() {
        let builder = SystemPromptBuilder::new();
        assert_eq!(builder.build(), "");
    }

    #[test]
    fn section_with_content_is_included() {
        let builder = SystemPromptBuilder::new()
            .base_section("Base content")
            .agent_section("Agent content");
        let result = builder.build();
        assert!(result.contains("Base content"));
        assert!(result.contains("Agent content"));
    }

    #[test]
    fn section_order_is_preserved() {
        let builder = SystemPromptBuilder::new()
            .base_section("Base")
            .add_section(SectionName::Agent, "Agent", PromptPriority::High)
            .add_section(SectionName::Skill, "Skill", PromptPriority::Medium);

        let sections: Vec<_> = builder.sections.iter().map(|(name, _)| name).collect();
        assert_eq!(sections[0], &SectionName::Base);
        assert_eq!(sections[1], &SectionName::Agent);
        assert_eq!(sections[2], &SectionName::Skill);
    }

    #[test]
    fn debug_sections_returns_info_for_all_sections() {
        let builder = SystemPromptBuilder::new()
            .base_section("Base")
            .add_section(SectionName::Skill, "Skill content", PromptPriority::Medium);

        let debug = builder.debug_sections();
        assert!(debug.len() >= 2);
    }

    #[test]
    fn get_section_retrieves_by_name() {
        let builder = SystemPromptBuilder::new()
            .base_section("Base content")
            .add_section(SectionName::Agent, "Agent content", PromptPriority::High);

        assert_eq!(builder.get_section(&SectionName::Base), Some("Base content"));
        assert_eq!(builder.get_section(&SectionName::Agent), Some("Agent content"));
        assert_eq!(builder.get_section(&SectionName::Skill), None);
    }
}
