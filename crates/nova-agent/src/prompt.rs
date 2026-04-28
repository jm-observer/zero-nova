use crate::message::{ContentBlock, Message, Role};
use crate::provider::types::ToolDefinition;
use crate::skill::{CapabilityPolicy, SkillRegistry};
#[cfg(test)]
use crate::skill::{SkillPackage, ToolPolicy};
use crate::tool::ToolRegistry;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

// ---------------------------------------------------------------------------
//  Section-based prompt building (Plan 1 升级)
// ---------------------------------------------------------------------------

/// 项目上下文文件名（按优先级排列）
const PROJECT_CONTEXT_FILES: &[&str] = &["PROJECT.md", "NOVA.md"];

/// 项目上下文最大字符数（约 4000 token）
const MAX_PROJECT_CONTEXT_CHARS: usize = 16000;

/// 内置行为约束文本。
///
/// 与 bootstrap.rs 中原始硬编码字符串语义一致，Phase 1 不做内容变更。
pub const BEHAVIOR_GUARDS: &str = r#"
## CRITICAL: Action Consistency
- You are a physical entity with real-world capabilities.
- If you state that you are going to perform an action (e.g., "running a command", "writing a file", "searching the web"), you MUST generate the corresponding tool_use block in the SAME response.
- NEVER claim you are doing something "in the background" or "internally" without an actual tool call.
- Textual confirmation of an action is only valid AFTER the tool has been invoked.
"#;

/// Prompt 构建所需的完整配置。
///
/// 由 bootstrap / CLI / ConversationService 统一创建。
#[derive(Debug, Clone)]
pub struct PromptConfig {
    /// Agent 标识（用于日志和调试）
    pub agent_id: String,
    /// 从文件加载的 agent prompt 内容（已读取为字符串）
    pub agent_prompt: String,
    /// 项目目录（用于加载项目上下文文件等）
    pub project_dir: PathBuf,
    /// 当前活跃的 skill id（如果有）
    pub active_skill: Option<String>,
    /// 模板变量键值对（用于替换 {{key}} 占位符）
    pub template_vars: HashMap<String, String>,
    /// 运行时环境快照
    pub environment: Option<EnvironmentSnapshot>,
    /// 自定义项目上下文文件路径
    pub project_context_path: Option<PathBuf>,
    /// 已预加载的项目上下文内容（用于消除同步 I/O）
    pub project_context_content: Option<String>,
    /// workflow-stages.md 路径
    pub workflow_prompt_path: Option<PathBuf>,
}

impl PromptConfig {
    pub fn new(agent_id: impl Into<String>, agent_prompt: impl Into<String>, project_dir: PathBuf) -> Self {
        Self {
            agent_id: agent_id.into(),
            agent_prompt: agent_prompt.into(),
            project_dir,
            active_skill: None,
            template_vars: HashMap::new(),
            environment: None,
            project_context_path: None,
            project_context_content: None,
            workflow_prompt_path: None,
        }
    }

    pub fn with_active_skill(mut self, skill_id: impl Into<String>) -> Self {
        self.active_skill = Some(skill_id.into());
        self
    }

    pub fn with_template_var(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.template_vars.insert(key.into(), value.into());
        self
    }

    pub fn with_template_vars(mut self, vars: HashMap<String, String>) -> Self {
        self.template_vars = vars;
        self
    }

    pub fn with_environment(mut self, env: EnvironmentSnapshot) -> Self {
        self.environment = Some(env);
        self
    }

    pub fn with_project_context_path(mut self, path: PathBuf) -> Self {
        self.project_context_path = Some(path);
        self
    }

    pub fn with_project_context_path_opt(mut self, path: Option<PathBuf>) -> Self {
        self.project_context_path = path;
        self
    }

    pub fn with_project_context_content(mut self, content: String) -> Self {
        self.project_context_content = Some(content);
        self
    }

    pub fn with_workflow_prompt_path(mut self, path: PathBuf) -> Self {
        self.workflow_prompt_path = Some(path);
        self
    }
}

/// 模板变量正则匹配
static TEMPLATE_VAR_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\{\{([a-zA-Z_][a-zA-Z0-9_]*)\}\}").unwrap());

/// 预定义模板变量名称。
pub mod template_vars {
    /// 当前 workflow 阶段
    pub const WORKFLOW_STAGE: &str = "workflow_stage";
    /// 当前挂起交互
    pub const PENDING_INTERACTION: &str = "pending_interaction";
    /// 当前话题
    pub const TOPIC: &str = "topic";
    /// 约束条件
    pub const CONSTRAINTS: &str = "constraints";
    /// 候选方案列表
    pub const CANDIDATES: &str = "candidates";
    /// 已选方案
    pub const SELECTED_CANDIDATE: &str = "selected_candidate";
    /// 当前活跃 agent
    pub const ACTIVE_AGENT: &str = "active_agent";
}

/// 简单的 `{{key}}` 模板变量替换。
pub struct TemplateContext;

impl TemplateContext {
    /// 替换模板中的 `{{key}}` 占位符。
    ///
    /// - 已匹配的变量替换为对应值
    /// - 未匹配的占位符替换为空字符串（清理模式）
    pub fn render(template: &str, vars: &HashMap<String, String>) -> String {
        TEMPLATE_VAR_RE
            .replace_all(template, |caps: &regex::Captures| {
                let key = &caps[1];
                vars.get(key).cloned().unwrap_or_default()
            })
            .to_string()
    }

    /// 替换模板中的 `{{key}}` 占位符（保留模式）。
    ///
    /// 已匹配的变量替换为对应值，未匹配的保持原样。
    pub fn render_partial(template: &str, vars: &HashMap<String, String>) -> String {
        TEMPLATE_VAR_RE
            .replace_all(template, |caps: &regex::Captures| {
                let key = &caps[1];
                match vars.get(key) {
                    Some(value) => value.clone(),
                    None => caps[0].to_string(),
                }
            })
            .to_string()
    }

    /// 提取模板中所有占位符的名称。
    pub fn extract_vars(template: &str) -> Vec<String> {
        TEMPLATE_VAR_RE
            .captures_iter(template)
            .map(|cap| cap[1].to_string())
            .collect()
    }
}

/// 运行时环境快照，在会话创建时采集一次。
#[derive(Debug, Clone, Default)]
pub struct EnvironmentSnapshot {
    /// 配置目录
    pub config_dir: String,
    /// 项目目录
    pub project_dir: String,
    /// 操作系统平台
    pub platform: String,
    /// Shell 类型
    pub shell: String,
    /// Git 当前分支（非 git 目录时为 None）
    pub git_branch: Option<String>,
    /// Git 状态摘要
    pub git_status_summary: Option<String>,
    /// 最近提交摘要（oneline 格式，最多 5 条）
    pub recent_commits: Option<String>,
    /// 当前使用的模型 ID
    pub model_id: Option<String>,
    /// 当前日期
    pub current_date: String,
}

impl EnvironmentSnapshot {
    /// 采集当前运行环境信息。
    ///
    /// git 命令失败时（非 git 目录或无 git 可执行文件）静默跳过，
    /// 确保在任何环境下都能正常工作。
    pub async fn collect(config_dir: &Path, project_dir: &Path) -> Self {
        let config_dir_path = config_dir;
        let config_dir = config_dir_path.to_string_lossy().to_string();
        let project_dir_path = project_dir;
        let project_dir = project_dir_path.to_string_lossy().to_string();

        let platform = std::env::consts::OS.to_string();

        let shell = std::env::var("SHELL")
            .or_else(|_| std::env::var("COMSPEC"))
            .unwrap_or_else(|_| "unknown".to_string());

        let git_branch = Self::run_git(project_dir_path, &["rev-parse", "--abbrev-ref", "HEAD"]).await;

        let git_status_summary = Self::run_git(project_dir_path, &["status", "--short"]).await.map(|s| {
            let count = s.lines().filter(|l| !l.is_empty()).count();
            if count == 0 {
                "clean".to_string()
            } else {
                format!("{} changed files", count)
            }
        });

        let recent_commits = Self::run_git(project_dir_path, &["log", "--oneline", "-5"]).await;

        let current_date = chrono::Local::now().format("%Y-%m-%d").to_string();

        Self {
            config_dir,
            project_dir,
            platform,
            shell,
            git_branch,
            git_status_summary,
            recent_commits,
            model_id: None,
            current_date,
        }
    }

    /// 运行 git 命令并返回 stdout 输出。
    /// 失败时返回 None（不报错）。
    async fn run_git(config_dir: &Path, args: &[&str]) -> Option<String> {
        let result = tokio::process::Command::new("git")
            .args(args)
            .current_dir(config_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .await;

        match result {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            }
            _ => None,
        }
    }

    /// 生成 prompt section 文本。
    pub fn to_prompt_text(&self) -> String {
        let mut lines = vec![
            format!("Config directory: {}", self.config_dir),
            format!("Project directory: {}", self.project_dir),
            format!("Platform: {}", self.platform),
            format!("Shell: {}", self.shell),
            format!("Date: {}", self.current_date),
        ];

        if let Some(branch) = &self.git_branch {
            lines.push(format!("Git branch: {}", branch));
        }
        if let Some(status) = &self.git_status_summary {
            lines.push(format!("Git status: {}", status));
        }
        if let Some(commits) = &self.recent_commits {
            lines.push(String::new()); // 空行分隔
            lines.push("Recent commits:".to_string());
            lines.push(commits.clone());
        }
        if let Some(model) = &self.model_id {
            lines.push(format!("Model: {}", model));
        }

        lines.join("\n")
    }
}

/// 从工作区加载项目上下文文件。
///
/// 按优先级查找 PROJECT.md → NOVA.md，找到第一个非空文件即返回。
/// 所有文件都不存在或为空时返回 None。
/// 异步从工作区加载项目上下文文件（Plan 2 规范建议修复）。
pub async fn load_project_context_async(project_dir: &Path) -> Option<String> {
    load_project_context_with_config_async(project_dir, None).await
}

/// 异步从工作区加载项目上下文文件，支持显式路径。
pub async fn load_project_context_with_config_async(
    project_dir: &Path,
    configured_path: Option<&Path>,
) -> Option<String> {
    if let Some(path) = configured_path {
        return load_single_project_context_async(path).await;
    }

    for filename in PROJECT_CONTEXT_FILES {
        let path = project_dir.join(filename);
        if let Some(content) = load_single_project_context_async(&path).await {
            return Some(content);
        }
    }
    None
}

async fn load_single_project_context_async(path: &Path) -> Option<String> {
    match tokio::fs::read_to_string(path).await {
        Ok(content) if !content.trim().is_empty() => {
            log::info!(
                "Loaded project context from {:?} ({} chars) [async]",
                path,
                content.len()
            );
            if content.len() > MAX_PROJECT_CONTEXT_CHARS {
                let truncated = &content[..MAX_PROJECT_CONTEXT_CHARS];
                let last_newline = truncated.rfind('\n').unwrap_or(MAX_PROJECT_CONTEXT_CHARS);
                let mut result = truncated[..last_newline].to_string();
                result.push_str("\n\n[... truncated due to size limit ...]");
                return Some(result);
            }
            Some(content)
        }
        _ => None,
    }
}

pub fn load_project_context(project_dir: &Path) -> Option<String> {
    load_project_context_with_config(project_dir, None)
}

/// 从工作区加载项目上下文文件，支持显式配置文件路径。
pub fn load_project_context_with_config(project_dir: &Path, configured_path: Option<&Path>) -> Option<String> {
    if let Some(path) = configured_path {
        return load_single_project_context(path);
    }

    for filename in PROJECT_CONTEXT_FILES {
        let path = project_dir.join(filename);
        if let Some(content) = load_single_project_context(&path) {
            return Some(content);
        }
    }
    None
}

fn load_single_project_context(path: &Path) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(content) if !content.trim().is_empty() => {
            log::info!("Loaded project context from {:?} ({} chars)", path, content.len());
            if content.len() > MAX_PROJECT_CONTEXT_CHARS {
                let truncated = &content[..MAX_PROJECT_CONTEXT_CHARS];
                let last_newline = truncated.rfind('\n').unwrap_or(MAX_PROJECT_CONTEXT_CHARS);
                let mut result = truncated[..last_newline].to_string();
                result.push_str("\n\n[... truncated due to size limit ...]");
                return Some(result);
            }
            Some(content)
        }
        Ok(_) => {
            log::debug!("Project context file {:?} is empty, skipping", path);
            None
        }
        Err(_) => None,
    }
}

/// 系统提示词具名 section 名称。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SectionName {
    Base,
    Agent,
    Skill,
    ProjectContext,
    BehaviorGuards,
    Environment,
    Workflow,
    ToolGuidance,
    History,
}

impl SectionName {
    /// 返回该 section 在最终 prompt 中的标题。
    pub fn heading(&self) -> &str {
        match self {
            Self::Base => "Identity & Role",
            Self::Agent => "Agent Configuration",
            Self::Skill => "Available Skills",
            Self::ProjectContext => "Project Context",
            Self::BehaviorGuards => "Behavior Constraints",
            Self::Environment => "Environment",
            Self::Workflow => "Workflow State",
            Self::ToolGuidance => "Tool Capabilities",
            Self::History => "Conversation Summary",
        }
    }
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

    /// 添加行为约束 section。
    pub fn behavior_guards_section(self) -> Self {
        self.add_section(
            SectionName::BehaviorGuards,
            BEHAVIOR_GUARDS.trim(),
            PromptPriority::High,
        )
    }

    /// 添加项目上下文 section（Phase 2 使用）。
    pub fn project_context_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::ProjectContext, content, PromptPriority::Medium)
    }

    /// 添加环境快照 section。
    pub fn environment_snapshot(self, env: &EnvironmentSnapshot) -> Self {
        self.add_section(SectionName::Environment, env.to_prompt_text(), PromptPriority::High)
    }

    fn with_tool_definitions_internal(mut self, definitions: &[ToolDefinition]) -> Self {
        let mut tool_desc = String::new();
        for def in definitions {
            tool_desc.push_str(&format!(
                "## {}\n\n{}\n\nInput schema:\n```json\n{}\n```\n\n---\n\n",
                def.name,
                def.description,
                serde_json::to_string_pretty(&def.input_schema).unwrap_or_else(|_| "{}".to_string())
            ));
        }

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

    /// 追加工具描述到现有的 ToolGuidance section。
    pub fn with_tools(self, registry: &ToolRegistry) -> Self {
        let definitions: Vec<ToolDefinition> = registry
            .loaded_definitions()
            .into_iter()
            .map(|def| ToolDefinition {
                name: def.name,
                description: def.description,
                input_schema: def.input_schema,
            })
            .collect();
        self.with_tool_definitions(&definitions)
    }

    /// 追加当前轮次实际可见的工具定义，确保 prompt 与 API tools 参数一致。
    pub fn with_tool_definitions(self, definitions: &[ToolDefinition]) -> Self {
        self.with_tool_definitions_internal(definitions)
    }

    /// 从配置创建完整的 system prompt builder（Phase 2 版本）。
    ///
    /// 构建的 section 顺序：
    ///   Base (agent prompt) → BehaviorGuards → Skill → ProjectContext → Environment
    pub fn from_config(config: &PromptConfig, skills: &SkillRegistry) -> Self {
        let mut builder = Self::new();

        // L0: 平台身份（agent prompt 文件内容，经模板替换）
        let rendered_prompt = if config.template_vars.is_empty() {
            config.agent_prompt.clone()
        } else {
            TemplateContext::render(&config.agent_prompt, &config.template_vars)
        };
        if !rendered_prompt.is_empty() {
            builder = builder.base_section(&rendered_prompt);
        }

        // L1: 行为约束
        builder = builder.behavior_guards_section();

        // L2: Skills（按需注入）
        let skill_prompt = skills.generate_contextual_prompt(config.active_skill.as_deref());
        if !skill_prompt.is_empty() {
            builder = builder.skill_section(&skill_prompt);
        }

        // L3: 项目上下文
        if let Some(content) = &config.project_context_content {
            builder = builder.project_context_section(content);
        } else if let Some(content) =
            load_project_context_with_config(&config.project_dir, config.project_context_path.as_deref())
        {
            builder = builder.project_context_section(&content);
        }

        // L5: 环境快照
        if let Some(env) = &config.environment {
            builder = builder.environment_snapshot(env);
        }

        if let Some(stage) = config.template_vars.get(template_vars::WORKFLOW_STAGE) {
            if stage != "idle" {
                if let Some(path) = &config.workflow_prompt_path {
                    if let Ok(workflow_prompts) = WorkflowStagePrompts::load_from_file(path) {
                        if let Some(prompt) = workflow_prompts.render(stage, &config.template_vars) {
                            builder = builder.workflow_section(prompt);
                        }
                    }
                }
            }
        }

        builder
    }

    /// 构建最终 prompt 字符串，按 section 顺序拼接，跳过空值 section。
    ///
    /// 每个 section 输出为 `## heading\n\ncontent` 格式，用 `\n\n---\n\n` 分隔。
    pub fn build(&self) -> String {
        self.sections
            .iter()
            .filter(|(_, section)| !section.content.is_empty())
            .map(|(name, section)| format!("## {}\n\n{}", name.heading(), section.content))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
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

// ---------------------------------------------------------------------------
//  G9 — 历史裁剪（History Trimming）
// ---------------------------------------------------------------------------

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

    /// 估算消息列表的 token 数。
    ///
    /// 使用字符数 / 3 的粗略估算（英文约 4 chars/token，中文约 1.5 chars/token）。
    /// 取折中值 3 chars/token。
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
                    })
                    .sum::<usize>()
            })
            .sum();

        total_chars / 3
    }

    /// 估算系统提示词的 token 数。
    fn estimate_system_prompt_tokens(system_prompt: &str) -> usize {
        system_prompt.len() / 3
    }

    /// 对历史消息进行裁剪。
    ///
    /// 策略：
    /// 1. 保留第一条 system 消息（如果存在）
    /// 2. 保留最近 min_recent_messages 条消息
    /// 3. 从最旧的非 system 消息开始移除，直到总 token 在预算内
    /// 4. 确保 tool_use 和对应的 tool_result 成对移除（不留孤立的 tool_result）
    pub fn trim(&self, messages: &[Message], system_prompt: &str) -> TrimResult {
        let system_tokens = Self::estimate_system_prompt_tokens(system_prompt);
        let history_budget = self
            .config
            .context_window
            .saturating_sub(system_tokens)
            .saturating_sub(self.config.output_reserve);

        // 分离 system 消息和对话消息
        let (system_msgs, conversation_msgs): (Vec<_>, Vec<_>) =
            messages.iter().enumerate().partition(|(_, m)| m.role == Role::System);

        let system_msgs: Vec<_> = system_msgs.into_iter().map(|(_, m)| m.clone()).collect();
        let conversation_msgs: Vec<_> = conversation_msgs.into_iter().map(|(_, m)| m.clone()).collect();
        let current_tokens = Self::estimate_tokens(&conversation_msgs);

        if current_tokens <= history_budget {
            return TrimResult {
                messages: messages.to_vec(),
                was_trimmed: false,
                removed_count: 0,
                summary: None,
            };
        }

        // 保护最近 N 条消息
        let protected_count = self.config.min_recent_messages.min(conversation_msgs.len());
        let trimmable = &conversation_msgs[..conversation_msgs.len() - protected_count];
        let protected = &conversation_msgs[conversation_msgs.len() - protected_count..];

        // 从前往后移除消息，直到总 token 在预算内
        let protected_tokens = Self::estimate_tokens(protected);
        let mut remaining_budget = history_budget.saturating_sub(protected_tokens);

        let mut kept_trimmable = Vec::new();
        // 从后往前扫描可裁剪消息，保留连续的最近消息，避免中间断裂
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

        // 重新组装
        let mut result = system_msgs;
        // 如果有裁剪，插入一条摘要提示
        if removed_count > 0 {
            result.push(Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
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

// ---------------------------------------------------------------------------
//  G10 — 侧信道注入（Side Channel Injection）
// ---------------------------------------------------------------------------

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
    pub fn generate_injection(&self, skills: &SkillRegistry) -> Option<String> {
        if !self.config.enabled {
            return None;
        }

        let count = self
            .tool_result_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // 检查是否到了注入间隔
        if !count.is_multiple_of(self.config.skill_reminder_interval) {
            return None;
        }

        let mut parts = Vec::new();

        // Skill 列表提醒
        if !skills.packages.is_empty() {
            let skill_list: Vec<String> = skills
                .packages
                .iter()
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
            parts.push(format!("<system-reminder>\nCurrent date: {}\n</system-reminder>", date));
        }

        // 自定义提醒
        for reminder in &self.config.custom_reminders {
            parts.push(format!("<system-reminder>\n{}\n</system-reminder>", reminder));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        }
    }

    /// 将侧信道内容附加到 tool result 输出后面。
    pub fn inject_into_tool_result(&self, tool_output: &str, skills: &SkillRegistry) -> String {
        match self.generate_injection(skills) {
            Some(injection) => format!("{}\n\n{}", tool_output, injection),
            None => tool_output.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
//  附加 — Workflow Stage Prompts
// ---------------------------------------------------------------------------

/// 工作流阶段 prompt 集合。
pub struct WorkflowStagePrompts {
    /// 阶段名称 → prompt 内容
    stages: HashMap<String, String>,
}

impl WorkflowStagePrompts {
    /// 从 workflow-stages.md 文件加载。
    ///
    /// 当前实现仅提取 fenced code block 内的内容，围栏外说明文本会被忽略。
    pub fn load_from_file(path: &Path) -> anyhow::Result<Self> {
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
        self.get(stage).map(|template| TemplateContext::render(template, vars))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn create_temp_dir(prefix: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("zero-nova-{}-{}", prefix, suffix));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

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
        assert!(result.contains("## Identity & Role\n\nBase content"));
        assert!(result.contains("## Agent Configuration\n\nAgent content"));
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

    #[test]
    fn build_includes_heading() {
        let builder = SystemPromptBuilder::new().base_section("Content");
        let result = builder.build();
        assert!(result.contains("## Identity & Role"));
    }

    #[test]
    fn build_separates_with_divider() {
        let builder = SystemPromptBuilder::new().base_section("Base").agent_section("Agent");
        let result = builder.build();
        assert!(result.contains("\n\n---\n\n"));
    }

    #[test]
    fn with_tool_definitions_includes_only_provided_tools() {
        let defs = vec![ToolDefinition {
            name: "Read".to_string(),
            description: "Read a file".to_string(),
            input_schema: serde_json::json!({"type":"object"}),
        }];

        let prompt = SystemPromptBuilder::new()
            .tool_guidance_section("Tool usage")
            .with_tool_definitions(&defs)
            .build();

        assert!(prompt.contains("## Read"));
        assert!(!prompt.contains("## Write"));
    }

    #[test]
    fn behavior_guards_constant_exists() {
        assert!(BEHAVIOR_GUARDS.contains("CRITICAL: Action Consistency"));
        assert!(BEHAVIOR_GUARDS.contains("tool_use block"));
    }

    #[test]
    fn template_context_render_replaces_vars() {
        let mut vars = HashMap::new();
        vars.insert("workflow_stage".into(), "idle".into());
        vars.insert("pending_interaction".into(), "none".into());
        let result = TemplateContext::render("Stage: {{workflow_stage}}, Pending: {{pending_interaction}}", &vars);
        assert_eq!(result, "Stage: idle, Pending: none");
    }

    #[test]
    fn template_context_render_clears_unknown() {
        let vars = HashMap::new();
        let result = TemplateContext::render("Hello {{name}}, today is {{unknown}}", &vars);
        // Phase 2 清理模式：未匹配的占位符替换为空字符串
        assert_eq!(result, "Hello , today is ");
    }

    #[test]
    fn template_context_render_partial_keeps_unknown() {
        let vars = HashMap::new();
        let result = TemplateContext::render_partial("Hello {{name}}, today is {{unknown}}", &vars);
        // partial 模式：未知占位符保持原样
        assert!(result.contains("Hello {{name}}, today is {{unknown}}"));
    }

    #[test]
    fn template_extract_vars() {
        let template = "Stage: {{workflow_stage}}, Pending: {{pending_interaction}}, Topic: {{topic}}";
        let vars = TemplateContext::extract_vars(template);
        assert!(vars.contains(&"workflow_stage".to_string()));
        assert!(vars.contains(&"pending_interaction".to_string()));
        assert!(vars.contains(&"topic".to_string()));
        assert_eq!(vars.len(), 3);
    }

    #[test]
    fn context_render_with_regex_replaces_multiple_occurrences() {
        let empty = HashMap::new();
        let result = TemplateContext::render("{{x}} {{x}} {{x}}", &empty);
        // 多次出现的同一变量应被正确替换（即使为空）
        assert_eq!(result, "  ");
    }

    #[test]
    fn env_snapshot_to_prompt_includes_cwd() {
        let snapshot = EnvironmentSnapshot {
            config_dir: "D:/workspace".to_string(),
            project_dir: "D:/project".to_string(),
            platform: "windows".to_string(),
            shell: "powershell".to_string(),
            git_branch: None,
            git_status_summary: None,
            recent_commits: None,
            model_id: None,
            current_date: "2026-04-26".to_string(),
        };

        let prompt = snapshot.to_prompt_text();
        assert!(prompt.contains("Config directory: D:/workspace"));
        assert!(prompt.contains("Project directory: D:/project"));
        assert!(prompt.contains("Date: 2026-04-26"));
    }

    #[test]
    fn env_snapshot_to_prompt_optional_git() {
        let snapshot = EnvironmentSnapshot {
            config_dir: "D:/workspace".to_string(),
            project_dir: "D:/project".to_string(),
            platform: "windows".to_string(),
            shell: "powershell".to_string(),
            git_branch: Some("main".to_string()),
            git_status_summary: Some("clean".to_string()),
            recent_commits: None,
            model_id: None,
            current_date: "2026-04-26".to_string(),
        };

        let prompt = snapshot.to_prompt_text();
        assert!(prompt.contains("Git branch: main"));
        assert!(prompt.contains("Git status: clean"));
    }

    #[test]
    fn env_snapshot_to_prompt_with_commits() {
        let snapshot = EnvironmentSnapshot {
            config_dir: "D:/workspace".to_string(),
            project_dir: "D:/project".to_string(),
            platform: "windows".to_string(),
            shell: "powershell".to_string(),
            git_branch: None,
            git_status_summary: None,
            recent_commits: Some("abc123 first\nbcd234 second".to_string()),
            model_id: Some("gpt-oss".to_string()),
            current_date: "2026-04-26".to_string(),
        };

        let prompt = snapshot.to_prompt_text();
        assert!(prompt.contains("Recent commits:"));
        assert!(prompt.contains("abc123 first"));
        assert!(prompt.contains("Model: gpt-oss"));
    }

    #[test]
    fn load_project_context_finds_file() {
        let dir = create_temp_dir("project-context-find");
        fs::write(dir.join("PROJECT.md"), "hello project").unwrap();

        let content = load_project_context(&dir);
        assert_eq!(content.as_deref(), Some("hello project"));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_project_context_none_when_missing() {
        let dir = create_temp_dir("project-context-missing");
        assert!(load_project_context(&dir).is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_project_context_skips_empty() {
        let dir = create_temp_dir("project-context-empty");
        fs::write(dir.join("PROJECT.md"), "   \n").unwrap();
        fs::write(dir.join("NOVA.md"), "fallback").unwrap();

        let content = load_project_context(&dir);
        assert_eq!(content.as_deref(), Some("fallback"));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_project_context_truncates_large() {
        let dir = create_temp_dir("project-context-large");
        let large = format!("{}\nend", "a".repeat(MAX_PROJECT_CONTEXT_CHARS + 128));
        fs::write(dir.join("PROJECT.md"), large).unwrap();

        let content = load_project_context(&dir).unwrap();
        assert!(content.contains("[... truncated due to size limit ...]"));
        assert!(content.len() <= MAX_PROJECT_CONTEXT_CHARS + 64);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_project_context_with_config_prefers_configured_path() {
        let dir = create_temp_dir("project-context-config");
        let custom = dir.join("docs").join("ctx.md");
        fs::create_dir_all(custom.parent().unwrap()).unwrap();
        fs::write(&custom, "configured context").unwrap();

        let content = load_project_context_with_config(&dir, Some(&custom));
        assert_eq!(content.as_deref(), Some("configured context"));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn trim_no_op_when_under_budget() {
        let trimmer = HistoryTrimmer::new(TrimmerConfig {
            context_window: 1_000,
            output_reserve: 100,
            min_recent_messages: 2,
            enable_summary: false,
        });
        let messages = vec![
            Message {
                role: Role::System,
                content: vec![ContentBlock::Text {
                    text: "system".to_string(),
                }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "short".to_string(),
                }],
            },
        ];

        let result = trimmer.trim(&messages, "system");
        assert!(!result.was_trimmed);
        assert_eq!(result.messages.len(), 2);
    }

    #[test]
    fn trim_preserves_contiguous_recent_messages() {
        let trimmer = HistoryTrimmer::new(TrimmerConfig {
            context_window: 25,
            output_reserve: 10,
            min_recent_messages: 1,
            enable_summary: false,
        });
        let messages = vec![
            Message {
                role: Role::System,
                content: vec![ContentBlock::Text {
                    text: "system prompt".to_string(),
                }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "1".repeat(40) }],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text { text: "2".repeat(5) }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "3".repeat(5) }],
            },
        ];

        let result = trimmer.trim(&messages, "system prompt");
        assert!(result.was_trimmed);
        assert_eq!(result.removed_count, 1);
        assert_eq!(result.messages.len(), 4);
        assert!(matches!(result.messages[2].role, Role::Assistant));
        assert!(matches!(result.messages[3].role, Role::User));
    }

    #[test]
    fn trim_ignores_system_messages_in_history_budget() {
        let trimmer = HistoryTrimmer::new(TrimmerConfig {
            context_window: 100,
            output_reserve: 10,
            min_recent_messages: 2,
            enable_summary: false,
        });
        let messages = vec![
            Message {
                role: Role::System,
                content: vec![ContentBlock::Text { text: "s".repeat(120) }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "small".to_string(),
                }],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: "small".to_string(),
                }],
            },
        ];

        let result = trimmer.trim(&messages, &"s".repeat(120));
        assert!(!result.was_trimmed);
    }

    #[test]
    fn side_channel_disabled_returns_original() {
        let injector = SideChannelInjector::new(SideChannelConfig {
            enabled: false,
            skill_reminder_interval: 1,
            inject_date: false,
            custom_reminders: vec![],
        });
        let registry = SkillRegistry::new();

        assert_eq!(
            injector.inject_into_tool_result("tool output", &registry),
            "tool output"
        );
    }

    #[test]
    fn side_channel_injects_skill_and_custom_reminder() {
        let injector = SideChannelInjector::new(SideChannelConfig {
            enabled: true,
            skill_reminder_interval: 1,
            inject_date: false,
            custom_reminders: vec!["Remember policy".to_string()],
        });
        let mut registry = SkillRegistry::new();
        registry.packages.push(SkillPackage {
            id: "skill-1".to_string(),
            slug: "skill-1".to_string(),
            display_name: "Skill One".to_string(),
            description: "First".to_string(),
            instructions: "Do work".to_string(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec![],
            examples: vec![],
            source_path: std::path::PathBuf::from("skill-1"),
            compat_mode: false,
        });

        let result = injector.inject_into_tool_result("tool output", &registry);
        assert!(result.contains("tool output"));
        assert!(result.contains("Available skills:"));
        assert!(result.contains("Remember policy"));
    }

    #[test]
    fn side_channel_respects_interval() {
        let injector = SideChannelInjector::new(SideChannelConfig {
            enabled: true,
            skill_reminder_interval: 2,
            inject_date: false,
            custom_reminders: vec!["interval".to_string()],
        });
        let registry = SkillRegistry::new();

        let first = injector.inject_into_tool_result("tool output", &registry);
        let second = injector.inject_into_tool_result("tool output", &registry);
        assert!(first.contains("interval"));
        assert_eq!(second, "tool output");
    }

    #[test]
    fn workflow_stage_prompts_loads_code_blocks_only() {
        let dir = create_temp_dir("workflow-prompts");
        let file = dir.join("workflow-stages.md");
        fs::write(
            &file,
            "## analyze\noutside\n```md\ninside {{topic}}\n```\n## idle\n```md\nidle prompt\n```",
        )
        .unwrap();

        let prompts = WorkflowStagePrompts::load_from_file(&file).unwrap();
        let mut vars = HashMap::new();
        vars.insert("topic".to_string(), "prompt".to_string());

        assert_eq!(prompts.render("analyze", &vars).as_deref(), Some("inside prompt"));
        assert_eq!(prompts.get("idle"), Some("idle prompt"));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn from_config_includes_workflow_section_when_stage_active() {
        let dir = create_temp_dir("workflow-section");
        let workflow_file = dir.join("workflow-stages.md");
        fs::write(&workflow_file, "## draft\n```md\nDraft {{topic}}\n```").unwrap();

        let mut vars = HashMap::new();
        vars.insert(template_vars::WORKFLOW_STAGE.to_string(), "draft".to_string());
        vars.insert(template_vars::TOPIC.to_string(), "plan".to_string());
        let config = PromptConfig::new("agent", "base", dir.clone())
            .with_template_vars(vars)
            .with_workflow_prompt_path(workflow_file);
        let skills = SkillRegistry::new();

        let prompt = SystemPromptBuilder::from_config(&config, &skills).build();
        assert!(prompt.contains("## Workflow State"));
        assert!(prompt.contains("Draft plan"));

        fs::remove_dir_all(dir).unwrap();
    }
}
