use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
//  SkillPackage — Plan 1 统一技能包模型
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
//  保留兼容旧 API 的 Skill 类型
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
//  文件工具优先级（基于 v1_messages 会话分析）
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
//  CapabilityPolicy — 策略对象
// ---------------------------------------------------------------------------

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

impl Default for CapabilityPolicy {
    fn default() -> Self {
        Self {
            always_enabled_tools: vec![
                "Bash".to_string(),
                "Read".to_string(),
                "Write".to_string(),
                "Edit".to_string(),
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

// ---------------------------------------------------------------------------
//  SkillRegistry
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct SkillRegistry {
    /// 兼容旧层级的技能列表
    pub skills: Vec<Skill>,
    /// 新 SkillPackage 列表
    pub packages: Vec<SkillPackage>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 递归扫描 skill 根目录，加载所有 SKILL.md 和 skill.toml。
    pub fn load_from_dir<P: AsRef<Path>>(&mut self, dir: P) -> Result<()> {
        let dir = dir.as_ref();
        if !dir.exists() || !dir.is_dir() {
            return Ok(());
        }
        Self::scan_dir_recursive(dir, self)?;
        Ok(())
    }

    /// 递归扫描目录。
    fn scan_dir_recursive(dir: &Path, registry: &mut SkillRegistry) -> Result<()> {
        let entries = std::fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                // 尝试加载子目录
                let skill_md = path.join("SKILL.md");
                let skill_toml = path.join("skill.toml");
                if skill_md.exists() || skill_toml.exists() {
                    registry.load_single_skill(&path)?;
                }
                // 继续递归子目录
                Self::scan_dir_recursive(&path, registry)?;
            } else {
                // 直接加载文件 - 递归扫描时直接使用 load_single_skill
                let skill_md = path.join("SKILL.md");
                if skill_md.exists() {
                    registry.load_single_skill(&path)?;
                }
            }
        }
        Ok(())
    }

    /// 加载单个目录中的技能。
    pub fn load_single_skill<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref();

        // 优先检查是否已有 skill.toml
        let skill_toml_path = path.join("skill.toml");
        if skill_toml_path.exists() {
            match self.parse_skill_toml(&skill_toml_path) {
                Ok(pkg) => {
                    log::info!("Loaded SkillPackage: {} (via skill.toml) from {:?}", pkg.slug, path);
                    self.packages.push(pkg);
                    return Ok(());
                }
                Err(e) => {
                    log::warn!(
                        "Failed to parse skill.toml at {:?}, falling back to SKILL.md: {}",
                        path,
                        e
                    );
                }
            }
        }

        // 回退到 SKILL.md 解析
        let skill_md = path.join("SKILL.md");
        if skill_md.exists() {
            match self.parse_skill_file(&skill_md) {
                Ok(skill) => {
                    // 同时生成兼容的 SkillPackage（在 skill 被 push 之前调用）
                    let pkg = self.to_skill_package(&skill);
                    log::info!("Loaded skill: {} from {:?}", skill.name, path);
                    self.skills.push(skill);
                    self.packages.push(pkg);
                    Ok(())
                }
                Err(e) => Err(anyhow::anyhow!("Failed to parse skill at {:?}: {}", path, e)),
            }
        } else {
            Ok(())
        }
    }

    /// 从 SKILL.md 解析为旧 Skill 结构。
    fn parse_skill_file(&self, path: &Path) -> Result<Skill> {
        let content = std::fs::read_to_string(path)?;
        let parts: Vec<&str> = content.split("---").collect();

        if parts.len() < 3 {
            return Ok(Skill {
                name: path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
                description: String::new(),
                body: content,
                path: path.parent().unwrap().to_path_buf(),
                compat_mode: true,
            });
        }

        let frontmatter = parts[1];
        let body = parts[2..].join("---");

        let mut name = String::new();
        let mut description = String::new();

        for line in frontmatter.lines() {
            let line = line.trim();
            if let Some(stripped) = line.strip_prefix("name:") {
                name = stripped.trim().trim_matches('"').to_string();
            } else if let Some(stripped) = line.strip_prefix("description:") {
                description = stripped.trim().trim_matches('"').to_string();
            }
        }

        let fallback_name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let is_compat = name.is_empty();
        Ok(Skill {
            name: if name.is_empty() { fallback_name.clone() } else { name },
            description,
            body: body.trim().to_string(),
            path: path.parent().unwrap().to_path_buf(),
            compat_mode: is_compat,
        })
    }

    /// 将旧 Skill 转换为兼容的 SkillPackage。
    fn to_skill_package(&self, skill: &Skill) -> SkillPackage {
        let slug = skill
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&skill.name)
            .to_string();

        SkillPackage {
            id: slug.clone(),
            slug,
            display_name: skill.name.clone(),
            description: skill.description.clone(),
            instructions: skill.body.clone(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec![],
            examples: vec![],
            source_path: skill.path.clone(),
            compat_mode: true,
        }
    }

    /// 从 skill.toml 解析为 SkillPackage。
    fn parse_skill_toml(&self, path: &Path) -> Result<SkillPackage> {
        let content = std::fs::read_to_string(path)?;
        let toml: toml::Value = toml::from_str(&content)?;

        let slug = toml
            .get("slug")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| toml.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| {
                path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            });

        let display_name = toml
            .get("display_name")
            .and_then(|v| v.as_str())
            .unwrap_or(&slug)
            .to_string();

        let description = toml
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let instructions = toml
            .get("instructions")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let tool_policy = match toml
            .get("tool_policy")
            .and_then(|v| v.as_str())
            .unwrap_or("inherit_all")
        {
            "inherit_all" | "" => ToolPolicy::InheritAll,
            "allow_list" => {
                let list: Vec<String> = toml
                    .get("tool_policy")
                    .and_then(|t| t.get("allow_list"))
                    .and_then(|l| l.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();
                ToolPolicy::AllowList(list)
            }
            "allow_list_with_deferred" => {
                let list: Vec<String> = toml
                    .get("tool_policy")
                    .and_then(|t| t.get("allow_list"))
                    .and_then(|l| l.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();
                ToolPolicy::AllowListWithDeferred(list)
            }
            _ => ToolPolicy::InheritAll,
        };

        let sticky = toml.get("sticky").and_then(|v| v.as_bool()).unwrap_or(false);

        let aliases: Vec<String>;
        if let Some(arr) = toml.get("aliases").and_then(|v| v.as_array()) {
            aliases = arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
        } else {
            aliases = vec![];
        }

        let examples: Vec<String>;
        if let Some(arr) = toml.get("examples").and_then(|v| v.as_array()) {
            examples = arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
        } else {
            examples = vec![];
        }

        Ok(SkillPackage {
            id: slug.clone(),
            slug: slug.clone(),
            display_name,
            description,
            instructions,
            tool_policy,
            sticky,
            aliases,
            examples,
            source_path: path.to_path_buf(),
            compat_mode: false,
        })
    }

    /// 通过 slug 查找 SkillPackage。
    pub fn find_by_slug(&self, slug: &str) -> Option<&SkillPackage> {
        self.packages.iter().find(|p| p.slug == slug || p.id == slug)
    }

    /// 通过别名查找 SkillPackage。
    pub fn find_by_alias(&self, alias: &str) -> Option<&SkillPackage> {
        self.packages.iter().find(|p| p.aliases.iter().any(|a| a == alias))
    }

    /// 按名称（name/slug）查找 SkillPackage。
    pub fn find_by_name(&self, name: &str) -> Option<&SkillPackage> {
        self.packages
            .iter()
            .find(|p| p.slug == name || p.display_name == name || p.id == name)
    }

    /// 返回所有可用的 SkillPackage 列表（供路由器使用）。
    pub fn all_candidates(&self) -> Vec<&SkillPackage> {
        self.packages.iter().collect()
    }

    /// 获取指定 slug 的 instructions 文本（简化接口）。
    pub fn get_skill_prompt(&self, slug: &str) -> Option<String> {
        self.find_by_slug(slug).map(|p| p.instructions.clone())
    }

    /// 生成上下文感知的 skill prompt。
    ///
    /// - 无 active skill 时：仅输出 skill 名称 + 描述的索引表
    /// - 有 active skill 时：输出该 skill 的完整 instructions + 其余 skill 的名称列表
    ///
    /// 替代 `generate_system_prompt()` 的全量注入。
    pub fn generate_contextual_prompt(&self, active_skill_id: Option<&str>) -> String {
        if self.packages.is_empty() {
            return String::new();
        }

        let mut parts = Vec::new();

        // 活跃 skill：完整注入 instructions
        if let Some(active_id) = active_skill_id {
            if let Some(pkg) = self.find_package_by_id(active_id) {
                parts.push(format!(
                    "### Active Skill: {}\n\n{}\n",
                    pkg.display_name, pkg.instructions,
                ));
            }
        }

        // 其余 skill：仅名称 + 描述
        let other_skills: Vec<String> = self
            .packages
            .iter()
            .filter(|p| active_skill_id.map(|id| id != p.id && id != p.slug).unwrap_or(true))
            .map(|p| {
                let aliases = if p.aliases.is_empty() {
                    String::new()
                } else {
                    format!(" (aliases: {})", p.aliases.join(", "))
                };
                format!("- **{}**{}: {}", p.display_name, aliases, p.description)
            })
            .collect();

        if !other_skills.is_empty() {
            let header = if active_skill_id.is_some() {
                "### Other Available Skills"
            } else {
                "### Available Skills"
            };
            parts.push(format!(
                "{}\n\n{}\n\nUse `/skill-<name>` to activate a skill.",
                header,
                other_skills.join("\n"),
            ));
        }

        parts.join("\n\n")
    }

    /// 生成旧格式的整包 system prompt（向后兼容）。
    ///
    /// 请改用 `generate_contextual_prompt()` 以减少 token 消耗。
    #[deprecated(note = "Use generate_contextual_prompt() instead")]
    pub fn generate_system_prompt(&self) -> String {
        if self.packages.is_empty() && self.skills.is_empty() {
            return String::new();
        }

        let mut prompt = String::from("\n\n# Available Skills\n\n");
        for pkg in &self.packages {
            prompt.push_str(&format!("## Skill: {}\n", pkg.display_name));
            prompt.push_str(&format!("Description: {}\n", pkg.description));
            prompt.push_str(&format!("Path: {}\n\n", pkg.source_path.to_string_lossy()));
            prompt.push_str(&format!("### Instructions for {}\n", pkg.display_name));
            prompt.push_str(&pkg.instructions);
            prompt.push_str("\n\n---\n\n");
        }
        for skill in &self.skills {
            // 避免重复（兼容模式下 package 可能已包含）
            if !self
                .packages
                .iter()
                .any(|p| p.slug == skill.path.file_name().and_then(|s| s.to_str()).unwrap_or_default())
            {
                prompt.push_str(&format!("## Skill: {}\n", skill.name));
                prompt.push_str(&format!("Description: {}\n", skill.description));
                prompt.push_str(&format!("Path: {}\n\n", skill.path.to_string_lossy()));
                prompt.push_str(&format!("### Instructions for {}\n", skill.name));
                prompt.push_str(&skill.body);
                prompt.push_str("\n\n---\n\n");
            }
        }
        prompt
    }

    // -----------------------------------------------------------------------
    //  Plan 2 — SkillRouter 辅助方法（阶段一：纯规则匹配）
    // -----------------------------------------------------------------------

    /// 通过 id 查找 SkillPackage（供路由决策使用）。
    pub fn find_package_by_id(&self, skill_id: &str) -> Option<&SkillPackage> {
        self.packages.iter().find(|p| p.id == skill_id || p.slug == skill_id)
    }

    /// 根据 SkillPolicy 生成当前轮次的 CapabilityPolicy。
    pub fn policy_from_skill(&self, skill_id: &str) -> CapabilityPolicy {
        let mut policy = CapabilityPolicy {
            source: PolicySource::ActiveSkill,
            ..CapabilityPolicy::default()
        };

        if let Some(pkg) = self.find_package_by_id(skill_id) {
            match &pkg.tool_policy {
                ToolPolicy::InheritAll => {
                    // 全部继承，不需要额外调整
                }
                ToolPolicy::AllowList(tools) | ToolPolicy::AllowListWithDeferred(tools) => {
                    let base_tool_names: std::collections::HashSet<&str> =
                        ["Bash", "Read", "Write", "Edit"].iter().cloned().collect();

                    // 白名单中的基础工具保留在 always_enabled
                    policy.always_enabled_tools = tools
                        .iter()
                        .filter(|t| base_tool_names.contains(t.as_str()))
                        .cloned()
                        .collect();

                    // 白名单中的非基础工具放入 deferred
                    policy.deferred_tools = tools
                        .iter()
                        .filter(|t| !base_tool_names.contains(t.as_str()))
                        .cloned()
                        .collect();

                    // AllowListWithDeferred 保留 ToolSearch
                    if matches!(&pkg.tool_policy, ToolPolicy::AllowListWithDeferred(_)) {
                        policy.tool_search_enabled = true;
                    } else {
                        policy.tool_search_enabled = false;
                    }
                }
            }
        }

        policy
    }

    /// 检查用户输入是否为显式 skill 退出信号。
    pub fn is_exit_signal(&self, input: &str) -> bool {
        let trimmed = input.trim();
        trimmed == "/exit-skill" || trimmed == "/reset-skill" || trimmed == "/skill-off"
    }

    /// 检查用户输入是否匹配某个 skill（/skill-name 模式）。
    pub fn match_skill_by_input(&self, input: &str) -> Option<String> {
        let trimmed = input.trim();

        // 检查 /skill-name 模式
        if let Some(suffix) = trimmed.strip_prefix("/skill-") {
            if suffix.len() <= 50 {
                if let Some(pkg) = self.find_by_slug(suffix) {
                    return Some(pkg.id.clone());
                }
                if let Some(pkg) = self.find_by_alias(suffix) {
                    return Some(pkg.id.clone());
                }
            }
        }

        // 检查 /skill-name 模式（带空格分隔）
        if let Some(parts) = trimmed.strip_prefix("/skill ") {
            let parts: Vec<&str> = parts.split_whitespace().collect();
            if parts.len() >= 2 {
                let name = parts[1].to_string();
                if let Some(pkg) = self.find_by_slug(&name) {
                    return Some(pkg.id.clone());
                }
                if let Some(pkg) = self.find_by_name(&name) {
                    return Some(pkg.id.clone());
                }
            }
        }

        None
    }

    /// 根据工具策略生成 Tool 视图（裁剪后的工具集合）。
    pub fn get_tool_view(&self, skill_id: &str) -> Vec<String> {
        let mut tools = vec![
            "Bash".to_string(),
            "Read".to_string(),
            "Write".to_string(),
            "Edit".to_string(),
        ];

        if let Some(pkg) = self.find_package_by_id(skill_id) {
            match &pkg.tool_policy {
                ToolPolicy::AllowList(allow_list) | ToolPolicy::AllowListWithDeferred(allow_list) => {
                    // 只保留白名单中的工具（加上文件操作）
                    tools.clear();
                    tools.extend(allow_list.clone());
                }
                ToolPolicy::InheritAll => {
                    // 不调整，保留全部
                }
            }
        }

        tools.sort();
        tools.dedup();
        tools
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_source_defaults_to_default() {
        let policy = CapabilityPolicy::default();
        assert_eq!(policy.source, PolicySource::Default);
    }

    #[test]
    fn tool_policy_inherit_all() {
        let policy = ToolPolicy::InheritAll;
        assert!(matches!(policy, ToolPolicy::InheritAll));
    }

    #[test]
    fn tool_policy_allow_list() {
        let tools = vec!["Bash".to_string(), "Read".to_string()];
        let policy = ToolPolicy::AllowList(tools.clone());
        assert!(matches!(policy, ToolPolicy::AllowList(list) if list == tools));
    }

    #[test]
    fn tool_policy_allow_list_with_deferred() {
        let tools = vec!["Bash".to_string(), "Read".to_string()];
        let policy = ToolPolicy::AllowListWithDeferred(tools.clone());
        assert!(matches!(policy,
            ToolPolicy::AllowListWithDeferred(list)
            if list == tools
        ));
    }

    #[test]
    fn file_tool_priority_prefer_file_tools() {
        let priority = FileToolPriority::PreferFileTools;
        assert!(matches!(priority, FileToolPriority::PreferFileTools));
    }

    #[test]
    fn capability_policy_allows_all_tools_by_default() {
        let policy = CapabilityPolicy::default();
        assert!(policy.tool_search_enabled);
        assert!(policy.skill_tool_enabled);
        assert!(policy.agent_tools_enabled);
    }

    #[test]
    fn policy_from_skill_allow_list_preserves_base_tools() {
        let mut registry = SkillRegistry::new();
        registry.packages.push(SkillPackage {
            id: "test".to_string(),
            slug: "test".to_string(),
            display_name: "Test".to_string(),
            description: "test".to_string(),
            instructions: "test".to_string(),
            tool_policy: ToolPolicy::AllowList(vec!["Bash".to_string(), "Read".to_string(), "CustomTool".to_string()]),
            sticky: false,
            aliases: vec![],
            examples: vec![],
            source_path: PathBuf::from("test"),
            compat_mode: false,
        });

        let policy = registry.policy_from_skill("test");
        // 基础工具应保留在 always_enabled
        assert!(policy.always_enabled_tools.contains(&"Bash".to_string()));
        assert!(policy.always_enabled_tools.contains(&"Read".to_string()));
        // Write 和 Edit 不在白名单中，不应出现
        assert!(!policy.always_enabled_tools.contains(&"Write".to_string()));
        // 非基础工具应在 deferred
        assert!(policy.deferred_tools.contains(&"CustomTool".to_string()));
    }

    #[test]
    fn policy_from_skill_allow_list_empty_keeps_no_base_tools() {
        let mut registry = SkillRegistry::new();
        registry.packages.push(SkillPackage {
            id: "test".to_string(),
            slug: "test".to_string(),
            display_name: "Test".to_string(),
            description: "test".to_string(),
            instructions: "test".to_string(),
            tool_policy: ToolPolicy::AllowList(vec!["CustomTool".to_string()]),
            sticky: false,
            aliases: vec![],
            examples: vec![],
            source_path: PathBuf::from("test"),
            compat_mode: false,
        });

        let policy = registry.policy_from_skill("test");
        // 白名单不含基础工具 → always_enabled 应为空
        assert!(policy.always_enabled_tools.is_empty());
    }

    #[test]
    fn contextual_prompt_no_active_shows_index() {
        let mut registry = SkillRegistry::new();
        registry.packages.push(SkillPackage {
            id: "skill-1".to_string(),
            slug: "skill-1".to_string(),
            display_name: "Skill One".to_string(),
            description: "First skill".to_string(),
            instructions: "Full instructions for skill one".to_string(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec!["s1".to_string()],
            examples: vec![],
            source_path: PathBuf::from("skill-1"),
            compat_mode: false,
        });
        registry.packages.push(SkillPackage {
            id: "skill-2".to_string(),
            slug: "skill-2".to_string(),
            display_name: "Skill Two".to_string(),
            description: "Second skill".to_string(),
            instructions: "Full instructions for skill two".to_string(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec![],
            examples: vec![],
            source_path: PathBuf::from("skill-2"),
            compat_mode: false,
        });

        let prompt = registry.generate_contextual_prompt(None);
        assert!(prompt.contains("### Available Skills"));
        assert!(prompt.contains("**Skill One** (aliases: s1): First skill"));
        assert!(prompt.contains("**Skill Two**: Second skill"));
        assert!(
            !prompt.contains("Full instructions"),
            "无活跃 skill 时不应包含完整 instructions"
        );
    }

    #[test]
    fn contextual_prompt_with_active_shows_full() {
        let mut registry = SkillRegistry::new();
        registry.packages.push(SkillPackage {
            id: "skill-1".to_string(),
            slug: "skill-1".to_string(),
            display_name: "Skill One".to_string(),
            description: "First skill".to_string(),
            instructions: "### Instructions for Skill One\nFull instructions content".to_string(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec!["s1".to_string()],
            examples: vec![],
            source_path: PathBuf::from("skill-1"),
            compat_mode: false,
        });
        registry.packages.push(SkillPackage {
            id: "skill-2".to_string(),
            slug: "skill-2".to_string(),
            display_name: "Skill Two".to_string(),
            description: "Second skill".to_string(),
            instructions: "### Instructions for Skill Two\nFull instructions content for two".to_string(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec![],
            examples: vec![],
            source_path: PathBuf::from("skill-2"),
            compat_mode: false,
        });

        let prompt = registry.generate_contextual_prompt(Some("skill-1"));
        assert!(prompt.contains("### Active Skill: Skill One"));
        assert!(
            prompt.contains("Full instructions content"),
            "活跃 skill 应包含完整 instructions"
        );
        assert!(prompt.contains("### Other Available Skills"));
        assert!(prompt.contains("**Skill Two**: Second skill"));
        assert!(
            !prompt.contains("Full instructions content for two"),
            "非活跃 skill 不应包含完整 instructions"
        );
    }

    #[test]
    fn contextual_prompt_empty_registry() {
        let registry = SkillRegistry::new();
        let prompt = registry.generate_contextual_prompt(None);
        assert!(prompt.is_empty());
    }
}
