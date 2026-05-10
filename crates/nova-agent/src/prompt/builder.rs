/// SystemPromptBuilder 及相关构建器模块。
///
/// 包含：
/// - `SystemPromptBuilder` — 组装完整的 System Prompt
/// - `HistoryTrimmer` — 历史消息裁剪
/// - `SideChannelInjector` — 侧信道注入器
/// - `WorkflowStagePrompts` — Workflow 阶段提示词
/// - `TrimmerConfig` — 裁剪配置
/// - `SideChannelConfig` — 侧信道配置
/// - `build_agent_catalog_section` — Agent 目录构建

use crate::message::ContentBlock;
use crate::prompt::templates::TemplateContext;
use crate::prompt::types::{
    AgentCatalogEntry, NamedSection, PromptConfig, PromptSectionSize, PromptPriority, SectionName, SkillInvocationLevel, SkillRouteDecision, SkillSwitchResult, ToolSize, TurnContext, ActiveSkillState,
};
use crate::skill::SkillRegistry;
use crate::config::{SideChannelConfigToml, TrimmerConfigToml};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
//  配置类型
// ---------------------------------------------------------------------------

/// 历史裁剪配置（非 TOML 版本，用于运行时）。
#[derive(Debug, Clone)]
pub struct TrimmerConfig {
    /// 模型上下文窗口大小
    pub context_window: usize,
    /// 输出预留 token 数
    pub output_reserve: usize,
    /// 最少保留的最近消息数
    pub min_recent_messages: usize,
}

impl Default for TrimmerConfig {
    fn default() -> Self {
        Self {
            context_window: 128_000,
            output_reserve: 8_000,
            min_recent_messages: 10,
        }
    }
}

impl From<TrimmerConfigToml> for TrimmerConfig {
    fn from(toml: TrimmerConfigToml) -> Self {
        Self {
            context_window: toml.context_window,
            output_reserve: toml.output_reserve,
            min_recent_messages: toml.min_recent_messages,
        }
    }
}

/// 侧信道注入配置（运行时版本）。
#[derive(Debug, Clone)]
pub struct SideChannelConfig {
    /// 是否启用侧信道
    pub enabled: bool,
    /// 注入 skill 列表的间隔
    pub skill_reminder_interval: usize,
    /// 是否注入当前日期
    pub inject_date: bool,
    /// 自定义提醒文本
    pub custom_reminders: Vec<String>,
}

impl From<SideChannelConfigToml> for SideChannelConfig {
    fn from(toml: SideChannelConfigToml) -> Self {
        Self {
            enabled: toml.enabled,
            skill_reminder_interval: toml.skill_reminder_interval,
            inject_date: toml.inject_date.unwrap_or(true),
            custom_reminders: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
//  HistoryTrimmer
// ---------------------------------------------------------------------------

/// 历史消息裁剪器。
///
/// 根据 `TrimmerConfig` 的配置，对历史消息进行 token 预算感知的裁剪。
pub struct HistoryTrimmer {
    config: TrimmerConfig,
}

impl HistoryTrimmer {
    /// 创建新的 `HistoryTrimmer`。
    pub fn new(config: TrimmerConfig) -> Self {
        Self { config }
    }

    /// 估算消息的 token 数量（简化版：字符数 / 4）。
    pub fn estimate_tokens(text: &str) -> usize {
        text.chars().count() / 4
    }

    /// 裁剪历史消息。
    ///
    /// 策略：
    /// 1. 保留最近的 `min_recent_messages` 条消息（全量保留）
    /// 2. 从旧到新一段段检查，累积 token
    /// 3. 当总 token 超过 `context_window - output_reserve` 时停止
    pub fn trim(&self, messages: &[Message]) -> Vec<Message> {
        let max_token = self.config.context_window.saturating_sub(self.config.output_reserve);

        let mut total_tokens: usize = 0;

        // 首先计算保留最近消息的 token 数量
        let keep_recent = self.config.min_recent_messages.min(messages.len());
        let recent_messages = &messages[messages.len() - keep_recent..];
        let recent_tokens: usize = recent_messages.iter().map(|m| {
            let content = match &m.content_blocks[0] {
                ContentBlock::Text { text, .. } => text.as_str(),
                _ => "",
            };
            Self::estimate_tokens(content)
        }).sum();

        total_tokens += recent_tokens;

        // 如果最近消息已经超过限制，直接返回
        if total_tokens >= max_token {
            return recent_messages.to_vec();
        }

        // 从旧消息中追加直到超过限制
        let mut result = Vec::with_capacity(keep_recent);
        result.extend(recent_messages.iter().cloned());

        let older_messages = &messages[..messages.len() - keep_recent];
        for msg in older_messages.iter().rev() {
            let content = match &msg.content_blocks[0] {
                ContentBlock::Text { text, .. } => text.as_str(),
                _ => "",
            };
            let tokens = Self::estimate_tokens(content);

            if total_tokens + tokens > max_token {
                break;
            }

            total_tokens += tokens;
            result.push(msg.clone());
        }

        result.reverse();
        result
    }
}

// ---------------------------------------------------------------------------
//  SideChannelInjector
// ---------------------------------------------------------------------------

/// 侧信道注入器。
///
/// 在 System Prompt 末尾注入额外的提示词，例如：
/// - 当前日期
/// - 可用 Skill 列表
/// - 自定义提醒
pub struct SideChannelInjector {
    config: SideChannelConfig,
}

impl SideChannelInjector {
    /// 创建新的 `SideChannelInjector`。
    pub fn new(config: SideChannelConfig) -> Self {
        Self { config }
    }

    /// 构建侧信道注入内容。
    pub fn build(&self) -> String {
        let mut parts = Vec::new();

        if self.config.enabled {
            if self.config.inject_date {
                if let Some(date) = self.get_current_date() {
                    parts.push(format!("**Current date:** {}", date));
                }
            }

            if !self.config.custom_reminders.is_empty() {
                parts.push("\n**Reminders:**".to_string());
                for reminder in &self.config.custom_reminders {
                    parts.push(format!("- {}", reminder));
                }
            }
        }

        if !parts.is_empty() {
            format!("\n---\n\nSide Channel:\n{}\n", parts.join("\n"))
        } else {
            String::new()
        }
    }

    fn get_current_date(&self) -> Option<String> {
        Some(chrono::Local::now().format("%Y-%m-%d").to_string())
    }
}

// ---------------------------------------------------------------------------
//  WorkflowStagePrompts
// ---------------------------------------------------------------------------

/// 单个 Workflow 阶段提示词。
#[derive(Debug, Clone)]
pub struct StagePrompt {
    /// 阶段名称
    pub name: String,
    /// 阶段描述
    pub description: String,
    /// 阶段约束
    pub constraints: String,
}

/// Workflow 阶段提示词管理器。
///
/// 从 `workflow-stages.md` 文件中加载和解析所有阶段。
pub struct WorkflowStagePrompts {
    stages: Vec<StagePrompt>,
    current_stage: String,
    template_vars: HashMap<String, String>,
}

impl WorkflowStagePrompts {
    /// 创建新的 `WorkflowStagePrompts`（空）。
    pub fn new() -> Self {
        Self {
            stages: Vec::new(),
            current_stage: "idle".to_string(),
            template_vars: HashMap::new(),
        }
    }

    /// 从工作流提示词文件加载阶段信息。
    ///
    /// 当前实现预留，后续可解析 `workflow-stages.md` 格式。
    pub async fn load_from_file(&mut self, _path: &std::path::Path) {
        // Placeholder for future workflow-stages.md parsing
        log::debug!("Loading workflow stages from {:?}", _path);
    }

    /// 设置当前阶段。
    pub fn set_current_stage(&mut self, stage: String) {
        self.current_stage = stage;
        self.template_vars.insert(
            "workflow_stage".to_string(),
            stage.clone(),
        );
    }

    /// 获取当前阶段的提示词。
    pub fn get_current_prompt(&self) -> String {
        if let Some(stage) = self.stages.iter().find(|s| s.name == self.current_stage) {
            TemplateContext::render(&stage.description, &self.template_vars)
        } else {
            String::new()
        }
    }

    /// 获取所有阶段名称。
    pub fn stage_names(&self) -> Vec<String> {
        self.stages.iter().map(|s| s.name.clone()).collect()
    }

    pub fn current_stage(&self) -> &str {
        &self.current_stage
    }

    fn get_current_date(&self) -> Option<String> {
        Some(chrono::Local::now().format("%Y-%m-%d").to_string())
    }
}

impl Default for WorkflowStagePrompts {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
//  SystemPromptBuilder
// ---------------------------------------------------------------------------

/// System Prompt 构建器。
///
/// 负责根据 `PromptConfig` 和 `SkillRegistry` 构建完整的 System Prompt。
/// 所有 section 按优先级和条件注入到最终 prompt 中。
pub struct SystemPromptBuilder {
    config: PromptConfig,
    skills: Vec<String>,
    sections: Vec<NamedSection>,
}

impl SystemPromptBuilder {
    /// 从配置和 Skill 注册表创建构建器。
    pub fn from_config(config: &PromptConfig, skill_registry: &SkillRegistry) -> Self {
        let skills: Vec<String> = skill_registry
            .list()
            .iter()
            .map(|s| s.id.clone())
            .collect();

        let mut builder = Self {
            config: config.clone(),
            skills,
            sections: Vec::new(),
        };

        builder.add_default_sections();
        builder
    }

    /// 添加默认 section。
    fn add_default_sections(&mut self) {
        // Base section always included
        self.sections.push(NamedSection {
            name: SectionName::Base,
            content: String::new(),
            required: true,
            priority: PromptPriority::High,
        });

        // Agent section always included
        self.sections.push(NamedSection {
            name: SectionName::Agent,
            content: self.config.agent_prompt.clone(),
            required: true,
            priority: PromptPriority::High,
        });

        // Skill section conditionally included
        if !self.skills.is_empty() {
            let skill_section = format!("Available Skills:\n{}", self.skills.join(", "));
            self.sections.push(NamedSection {
                name: SectionName::Skill,
                content: skill_section,
                required: true,
                priority: PromptPriority::Medium,
            });
        }

        // Tool guidance
        let tool_guidance = match self.config.tool_guidance {
            crate::prompt::types::ToolGuidanceMode::Compact => "Use tools when appropriate. Keep tool explanations concise.".to_string(),
            crate::prompt::types::ToolGuidanceMode::Full => "Always explain your reasoning before using tools. Consider alternative approaches.".to_string(),
        };
        self.sections.push(NamedSection {
            name: SectionName::ToolGuidance,
            content: tool_guidance,
            required: true,
            priority: PromptPriority::High,
        });
    }

    /// 构建完整的 System Prompt。
    pub fn build(&self) -> String {
        let mut parts = Vec::new();

        // Sort sections by priority
        let mut sorted_sections = self.sections.clone();
        sorted_sections.sort_by(|a, b| {
            let priority_order = match (a.priority, b.priority) {
                (PromptPriority::High, PromptPriority::High) => std::cmp::Ordering::Equal,
                (PromptPriority::High, _) => std::cmp::Ordering::Greater,
                (_, PromptPriority::High) => std::cmp::Ordering::Less,
                (PromptPriority::Medium, PromptPriority::Medium) => std::cmp::Ordering::Equal,
                (PromptPriority::Medium, PromptPriority::Low) => std::cmp::Ordering::Greater,
                (PromptPriority::Low, PromptPriority::Medium) => std::cmp::Ordering::Less,
                (PromptPriority::Low, PromptPriority::Low) => std::cmp::Ordering::Equal,
            };
            priority_order.then_with(|| a.name.cmp(&b.name))
        });

        for section in &sorted_sections {
            if required_or_has_content(&section.required, &section.content) {
                parts.push(format!("\n## {}\n{}", section.name, section.content));
            }
        }

        // Add agent catalog if available
        if let Some(catalog) = &self.config.agent_catalog {
            if !catalog.is_empty() {
                parts.push(format!("\n## {}\n{}", SectionName::AgentCatalog, catalog));
            }
        }

        // Add developer project prompt if available
        if let Some(developer_prompt) = &self.config.developer_project_prompt_content {
            if !developer_prompt.is_empty() {
                parts.push(format!("\n## {}\n{}", SectionName::DeveloperProjectPrompt, developer_prompt));
            }
        }

        // Add project context if available
        if let Some(project_context) = &self.config.project_context_content {
            if !project_context.is_empty() {
                parts.push(format!("\n## {}\n{}", SectionName::ProjectContext, project_context));
            }
        }

        parts.join("")
    }

    /// 获取 section 大小报告。
    pub fn section_size_report(&self) -> Vec<PromptSectionSize> {
        self.sections.iter().map(|s| {
            PromptSectionSize {
                name: s.name.clone(),
                heading: format!("{}", s.name),
                chars: s.content.chars().count(),
                priority: s.priority.clone(),
                required: s.required,
                is_large: s.content.chars().count() > 4000,
            }
        }).collect()
    }

    /// 生成工具大小报告。
    pub fn tool_size_report(tools: &[ToolDefinition]) -> Vec<ToolSize> {
        tools.iter().map(|t| {
            ToolSize {
                name: t.name.clone(),
                chars: t.json_description().unwrap_or_default().chars().count(),
            }
        }).collect()
    }
}

/// 检查 section 是否应该被包含。
fn required_or_has_content(required: bool, content: &str) -> bool {
    if required {
        return true;
    }
    !content.trim().is_empty()
}

// ---------------------------------------------------------------------------
//  Agent Catalog 构建
// ---------------------------------------------------------------------------

/// 构建 agent catalog section 文本。
pub fn build_agent_catalog_section(
    agents: &[crate::agent_catalog::AgentDescriptor],
    primary_agent_id: &str,
) -> String {
    let entries: Vec<AgentCatalogEntry> = agents.iter().map(|a| {
        AgentCatalogEntry {
            id: a.id.clone(),
            display_name: a.display_name.clone(),
            description: a.description.clone(),
            is_default: a.id == primary_agent_id,
            use_cases: vec![],
        }
    }).collect();

    let mut parts = Vec::new();
    parts.push("## Available Agents".to_string());
    parts.push("Use the appropriate agent based on task requirements.".to_string());
    parts.push(String::new());

    for entry in &entries {
        let default_marker = if entry.is_default { " (default)" } else { "" };
        parts.push(format!("- **{}{}**: {}", entry.id, default_marker, entry.description));
    }

    parts.push(String::new());
    parts.push(format!("Current agent: {}", primary_agent_id));

    parts.join("\n")
}

// ---------------------------------------------------------------------------
//  辅助模块
// ---------------------------------------------------------------------------


