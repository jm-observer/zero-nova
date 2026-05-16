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

/// CapabilityPolicy 描述当前轮次的轻量能力元数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityPolicy {
    /// 策略来源追踪
    pub source: PolicySource,
    pub file_tool_priority: FileToolPriority, // 文件 vs Bash 优先级
}

impl Default for CapabilityPolicy {
    fn default() -> Self {
        Self {
            source: PolicySource::Default,
            file_tool_priority: FileToolPriority::PreferFileTools,
        }
    }
}
