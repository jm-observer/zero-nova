use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

/// Tool 政策模式，决定该 skill 对工具的暴露范围。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ToolPolicy {
    /// 继承当前 agent 所有工具
    InheritAll,
    /// 严格工具白名单
    AllowList(Vec<String>),
    /// 白名单 + ToolSearch 可补充
    AllowListWithDeferred(Vec<String>),
}

/// SkillPackage 是统一后的技能包模型，可支撑路由与工具裁剪。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillPackage {
    /// 唯一标识符（推荐使用 slug）
    pub id: String,
    /// 文件系统中的路径标识
    pub slug: String,
    /// 用户展示的显示名
    pub display_name: String,
    /// 简短描述（≤100 字）
    pub description: String,
    /// 注入 system prompt 的核心指令
    pub instructions: String,
    /// 工具策略
    pub tool_policy: ToolPolicy,
    /// true = 激活后不自动退出
    pub sticky: bool,
    /// 路由匹配别名
    pub aliases: Vec<String>,
    /// 路由训练样本
    pub examples: Vec<String>,
    /// 来源文件路径
    pub source_path: PathBuf,
    /// 兼容旧格式时标记
    pub compat_mode: bool,
}

/// 兼容旧层级的 Skill 结构（被 SkillPackage 逐步替代）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
    pub path: PathBuf,
    /// 兼容旧格式时标记。
    #[serde(default)]
    pub compat_mode: bool,
}

/// 文件工具 vs Bash 的优先级策略。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileToolPriority {
    /// 优先 Read/Write/Edit，失败时 fallback 到 Bash
    PreferFileTools,
    /// 优先 Bash，适用于大量 shell 操作场景
    PreferBash,
    /// 根据操作类型自适应（读 → 文件工具，探测 → Bash）
    Adaptive,
}

/// 记录 CapabilityPolicy 的来源，便于调试和回溯。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PolicySource {
    /// 运行入口默认策略
    Default,
    /// 当前 agent 规格
    AgentSpec,
    /// active skill 的 tool_policy
    ActiveSkill,
    /// 用户显式模式切换
    UserOverride,
}

/// 工具状态枚举，描述工具在特定 Policy 下的启用状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ToolStatus {
    /// 始终可用的工具
    AlwaysEnabled,
    /// 通过 ToolSearch 延迟加载的工具
    Deferred,
    /// 当前不可用的工具
    Disabled,
}

/// CapabilityPolicy 描述当前轮次可见能力。
///
/// 基于 v1_messages 会话分析，增加了 cache 预算约束。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityPolicy {
    /// 始终可用的工具（如 Bash、Read、Write、Edit）
    pub always_enabled_tools: Vec<String>,
    /// 可能使用的延迟工具
    pub deferred_tools: Vec<String>,
    /// 允许 ToolSearch 按需加载
    pub tool_search_enabled: bool,
    /// 允许技能补充加载
    pub skill_tool_enabled: bool,
    /// 允许 Task 工具
    pub task_tools_enabled: bool,
    /// 允许 Agent 子代理
    pub agent_tools_enabled: bool,
    /// 策略来源追踪
    pub source: PolicySource,

    // Cache 预算约束（基于 v1_messages 会话分析，102733 tokens 缓存）
    pub cache_section_min_tokens: usize,      // 触发缓存创建的最小段（100）
    pub cache_section_max_tokens: usize,      // 单个 cache section 上限（4000）
    pub system_prompt_cache_target: usize,    // 目标缓存大小（98000）
    pub file_tool_priority: FileToolPriority, // 文件 vs Bash 优先级
}

impl CapabilityPolicy {
    /// 获取当前 Policy 下所有工具的完整状态映射。
    pub fn get_enabled_tools(&self, all_tools: &[String]) -> Vec<(String, ToolStatus)> {
        let mut result = Vec::with_capacity(all_tools.len());

        for tool_name in all_tools {
            let status = if self.always_enabled_tools.contains(tool_name) {
                ToolStatus::AlwaysEnabled
            } else if self.deferred_tools.contains(tool_name) {
                ToolStatus::Deferred
            } else {
                ToolStatus::Disabled
            };
            result.push((tool_name.clone(), status));
        }

        // 添加能力开关状态
        if self.tool_search_enabled {
            result.push(("ToolSearch".to_string(), ToolStatus::Deferred));
        }
        if self.skill_tool_enabled {
            result.push(("Skill".to_string(), ToolStatus::Deferred));
        }
        if self.task_tools_enabled {
            for task_tool in &["TaskCreate", "TaskList", "TaskUpdate"] {
                if !result.iter().any(|(name, _)| name == task_tool) {
                    result.push((task_tool.to_string(), ToolStatus::Deferred));
                }
            }
        }
        if self.agent_tools_enabled && !result.iter().any(|(name, _)| name == "Agent") {
            result.push(("Agent".to_string(), ToolStatus::Deferred));
        }

        result
    }

    /// 检查指定工具是否启用（无论状态类型）。
    pub fn is_tool_enabled(&self, tool_name: &str) -> bool {
        self.always_enabled_tools.iter().any(|t| t == tool_name) || self.deferred_tools.iter().any(|t| t == tool_name)
    }

    /// 获取已启用工具的数量。
    pub fn enabled_tool_count(&self) -> usize {
        self.always_enabled_tools.len() + self.deferred_tools.len()
    }
}

impl Default for CapabilityPolicy {
    fn default() -> Self {
        Self {
            always_enabled_tools: vec![
                "Bash".to_string(),
                "Read".to_string(),
                "Write".to_string(),
                "Edit".to_string(),
                "ProjectManager".to_string(),
                "Agent".to_string(),
            ],
            deferred_tools: vec![
                "TaskCreate".to_string(),
                "TaskList".to_string(),
                "TaskUpdate".to_string(),
                "Skill".to_string(),
            ],
            tool_search_enabled: true,
            skill_tool_enabled: true,
            task_tools_enabled: false,
            agent_tools_enabled: true,
            source: PolicySource::Default,
            // 缓存预算约束
            cache_section_min_tokens: 100,
            cache_section_max_tokens: 4000,
            system_prompt_cache_target: 98000,
            file_tool_priority: FileToolPriority::PreferFileTools,
        }
    }
}
