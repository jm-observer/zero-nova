/// 纯数据结构模块。
///
/// 包含 TurnContext, SkillRouteDecision, AgentCatalogEntry 等数据定义以及
/// PromptConfig、SectionName 等配置类型。
use crate::message::Message;
use crate::provider::types::ToolDefinition;
pub use crate::skill::types::{SkillInvocationLevel, SkillRouteDecision, SkillSwitchResult};
use crate::skill::CapabilityPolicy;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

// ---------------------------------------------------------------------------
//  常量 (常量需在此层级，不可下沉到子模块)
// ---------------------------------------------------------------------------

/// 系统提示词 section 名称。
///
/// 每个 section 按优先级和条件注入到最终 prompt 中。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SectionName {
    /// 身份与角色（Base）
    Base,
    /// Agent 配置
    Agent,
    /// 可用 Skills
    Skill,
    /// 项目上下文
    ProjectContext,
    /// 行为约束
    BehaviorGuards,
    /// 运行环境
    Environment,
    /// 工作流状态
    Workflow,
    /// 工具能力指导
    ToolGuidance,
    /// 对话历史摘要
    History,
    /// 开发项目提示词（Plan 2）
    DeveloperProjectPrompt,
    /// 可用 Agent 目录（Plan 1）
    AgentCatalog,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSectionSize {
    pub name: SectionName,
    pub heading: String,
    pub chars: usize,
    pub priority: PromptPriority,
    pub required: bool,
    pub is_large: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSize {
    pub name: String,
    pub chars: usize,
}

// ---------------------------------------------------------------------------
//  Prompt 配置类型
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectInstructionProfile {
    Auto,
    Analysis,
    Code,
    Design,
    Review,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillInjectionMode {
    Catalog,
    ActiveFull,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolGuidanceMode {
    Compact,
    Full,
}

/// Prompt 构建所需的完整配置。
///
/// 由 bootstrap / CLI / ConversationService 统一创建。
#[derive(Debug, Clone, Default)]
pub struct PromptLoadContext {
    /// 项目目录（用于加载项目上下文文件等）
    pub project_dir: Option<PathBuf>,
    /// 自定义项目上下文文件路径
    pub project_context_path: Option<PathBuf>,
    /// 开发项目提示词文件名列表（按配置顺序）
    pub developer_prompt_files: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PromptConfig {
    /// Agent 标识（用于日志和调试）
    pub agent_id: String,
    /// 从文件加载的 agent prompt 内容（已读取为字符串）
    pub agent_prompt: String,
    /// 当前活跃的 skill id（如果有）
    pub active_skill: Option<String>,
    /// 模板变量键值对（用于替换 {{key}} 占位符）
    pub template_vars: HashMap<String, String>,
    /// 运行时环境快照
    pub environment: Option<super::context::EnvironmentSnapshot>,
    /// 文件加载上下文（路径与文件列表）
    pub load_context: PromptLoadContext,
    /// 已预加载的项目上下文内容（用于消除同步 I/O）
    pub project_context_content: Option<String>,
    /// workflow-stages.md 路径
    pub workflow_prompt_path: Option<PathBuf>,
    /// 已合并完成的开发项目提示词内容
    pub developer_project_prompt_content: Option<String>,
    /// Orchestrator agent catalog（Plan 1）。为空时不注入 catalog section。
    pub agent_catalog: Option<String>,
    /// 项目规则注入 profile。
    pub project_instruction_profile: ProjectInstructionProfile,
    /// skill 注入策略。
    pub skill_injection: SkillInjectionMode,
    /// tool 提示策略。
    pub tool_guidance: ToolGuidanceMode,
}

impl PromptConfig {
    pub fn new(agent_id: impl Into<String>, agent_prompt: impl Into<String>, load_context: PromptLoadContext) -> Self {
        Self {
            agent_id: agent_id.into(),
            agent_prompt: agent_prompt.into(),
            active_skill: None,
            template_vars: HashMap::new(),
            environment: None,
            load_context,
            project_context_content: None,
            workflow_prompt_path: None,
            developer_project_prompt_content: None,
            agent_catalog: None,
            project_instruction_profile: ProjectInstructionProfile::Auto,
            skill_injection: SkillInjectionMode::Catalog,
            tool_guidance: ToolGuidanceMode::Compact,
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

    pub fn with_environment(mut self, env: super::context::EnvironmentSnapshot) -> Self {
        self.environment = Some(env);
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

    pub fn with_load_context(mut self, context: PromptLoadContext) -> Self {
        self.load_context = context;
        self
    }

    pub fn with_developer_project_prompt_content(mut self, content: String) -> Self {
        self.developer_project_prompt_content = Some(content);
        self
    }

    /// 设置 agent catalog 文本（Plan 1）。
    pub fn with_agent_catalog(mut self, catalog: String) -> Self {
        self.agent_catalog = Some(catalog);
        self
    }

    pub fn with_project_instruction_profile(mut self, profile: ProjectInstructionProfile) -> Self {
        self.project_instruction_profile = profile;
        self
    }

    pub fn with_skill_injection(mut self, mode: SkillInjectionMode) -> Self {
        self.skill_injection = mode;
        self
    }

    pub fn with_tool_guidance(mut self, mode: ToolGuidanceMode) -> Self {
        self.tool_guidance = mode;
        self
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
    /// 当前轮次可见的工具名集合（用于 ToolInfo 可见性过滤）
    pub visible_tool_names: Arc<HashSet<String>>,
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

// ---------------------------------------------------------------------------
//  Agent Catalog Entry — Plan 1
// ---------------------------------------------------------------------------

/// 单个 agent 在 catalog 中的条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCatalogEntry {
    /// Agent 唯一标识（对应 `gateway.agents[].id`）
    pub id: String,
    /// 显示名称
    pub display_name: String,
    /// 简短描述
    pub description: String,
    /// 是否默认 agent
    pub is_default: bool,
    /// 适用场景说明（可选）
    pub use_cases: Vec<String>,
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
            Self::DeveloperProjectPrompt => super::templates::DEVELOPER_PROMPT_SECTION_HEADING,
            Self::AgentCatalog => "Available Agents",
        }
    }
}
