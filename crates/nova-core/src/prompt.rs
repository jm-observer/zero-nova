use crate::message::Message;
use crate::provider::types::ToolDefinition;
use crate::skill::CapabilityPolicy;
use crate::tool::ToolRegistry;
use std::sync::Arc;
use std::time::Instant;

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
            self.sections.push((
                name.clone(),
                NamedSection {
                    name,
                    content: content_val,
                    required: priority == PromptPriority::High,
                    priority,
                },
            ));
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

    /// 添加 agent 环境 section（快速方法）。
    pub fn environment_agent(self) -> Self {
        self.add_section(
            SectionName::Environment,
            "Zero-Nova Agent Environment",
            PromptPriority::High,
        )
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
        if let Some((_, section)) = self
            .sections
            .iter_mut()
            .rev()
            .find(|(name, _)| *name == SectionName::ToolGuidance)
        {
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

// ---------------------------------------------------------------------------
//  Turn 上下文 — Plan 2 (Turn 前准备)
// ---------------------------------------------------------------------------

/// Turn 上下文：在 `run_turn` 调用前由 `prepare_turn` 组装的轮次上下文。
pub struct TurnContext {
    /// 系统提示词（已组装的完整 system prompt）
    pub system_prompt: String,
    /// 当前轮次可见的工具定义集合
    pub tool_definitions: Vec<ToolDefinition>,
    /// 当前轮次使用的历史消息
    pub history: Arc<Vec<Message>>,
    /// 当前活跃的 skill 状态（可选）
    pub active_skill: Option<ActiveSkillState>,
    /// 当前轮次的可见能力策略
    pub capability_policy: CapabilityPolicy,
    /// 是否启用 SkillTool 三层模型（第二阶段启用）
    pub skill_tool_enabled: bool,
    /// 构造后只读：最大 token 限
    pub max_tokens: usize,
    /// 构造后只读：当前轮剩余最大迭代次数
    pub iteration_budget: usize,
}

impl TurnContext {
    pub fn system_prompt(&self) -> &str {
        &self.system_prompt
    }

    pub fn tool_definitions(&self) -> &[ToolDefinition] {
        &self.tool_definitions
    }

    pub fn history(&self) -> &[Message] {
        &self.history
    }

    pub fn active_skill(&self) -> Option<&ActiveSkillState> {
        self.active_skill.as_ref()
    }

    pub fn capability_policy(&self) -> &CapabilityPolicy {
        &self.capability_policy
    }
}

/// 会话级 Active Skill 状态。
///
/// 放在会话层（nova-conversation）而非 AgentRuntime 中，
/// 确保 AgentRuntime 在同一个进程中跨多个会话复用时，
/// skill 数据不会在会话间泄漏。
#[derive(Debug, Clone)]
pub struct ActiveSkillState {
    /// 当前 active skill 的 id
    pub skill_id: String,
    /// 激活时间（用于 debug）
    pub entered_at: Instant,
    /// 最近一次路由评估时间
    pub last_routed_at: Instant,
    /// 追踪当前 session token 使用量
    pub history_token_count: usize,
}

impl ActiveSkillState {
    pub fn new(skill_id: String) -> Self {
        Self {
            skill_id,
            entered_at: Instant::now(),
            last_routed_at: Instant::now(),
            history_token_count: 0,
        }
    }

    pub fn update_route_time(&mut self) {
        self.last_routed_at = Instant::now();
    }
}

/// 路由决策结果。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SkillRouteDecision {
    /// 保持当前 skill
    KeepCurrent,
    /// 激活指定 skill
    Activate(String),
    /// 退出当前 skill
    Deactivate,
    /// 不激活任何 skill
    NoSkill,
}

/// Skill 调用来源层级（三层模型）。
///
/// 基于 v1_messages 会话分析，Skills 暴露但未调用（`/skill-name` 模式
/// 只支持用户显式输入）。需三层模型区分调用来源：
/// - 会话级 Skill — Turn 自动路由决定
/// - 工具级 SkillTool — 模型自动调用 SkillTool（需 prompt 明确触发条件）
/// - 用户级 /skill-name — 用户显式输入
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SkillInvocationLevel {
    /// 会话级 —— Turn 自动路由决定
    SessionLevel,
    /// 工具级 —— 模型自动调用 SkillTool
    ToolLevel,
    /// 用户级 —— 用户显式输入 /skill-name
    UserLevel,
}

/// 三层模型下的 Skill 切换结果。
#[derive(Debug, Clone)]
pub struct SkillSwitchResult {
    /// 是否发生了 skill 切换
    pub switched: bool,
    /// 切换到的 skill（可能和之前一样表示重新激活）
    pub to_skill: String,
    /// 切换原因
    pub reason: String,
    /// 调用层级
    pub level: SkillInvocationLevel,
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
        let builder = SystemPromptBuilder::new().base_section("Base").add_section(
            SectionName::Skill,
            "Skill content",
            PromptPriority::Medium,
        );

        let debug = builder.debug_sections();
        assert!(debug.len() >= 2);
    }

    #[test]
    fn get_section_retrieves_by_name() {
        let builder = SystemPromptBuilder::new().base_section("Base content").add_section(
            SectionName::Agent,
            "Agent content",
            PromptPriority::High,
        );

        assert_eq!(builder.get_section(&SectionName::Base), Some("Base content"));
        assert_eq!(builder.get_section(&SectionName::Agent), Some("Agent content"));
        assert_eq!(builder.get_section(&SectionName::Skill), None);
    }
}
