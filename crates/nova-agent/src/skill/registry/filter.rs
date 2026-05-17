use super::SkillRegistry;
use super::{CapabilityPolicy, PolicySource, SkillPackage, ToolPolicy};

impl SkillRegistry {
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
            if active_skill_id.is_some() {
                parts.push(format!(
                    "### Other Available Skills\n\n{}\n\n调用 `Skill` 工具激活：参数 `skill` 填技能名（上方加粗的标识符）。",
                    other_skills.join("\n"),
                ));
            } else {
                parts.push(format!(
                    "{}\n\n调用 `Skill` 工具激活：参数 `skill` 填技能名（上方加粗的标识符）。",
                    other_skills.join("\n"),
                ));
            }
        }

        parts.join("\n\n")
    }

    /// 仅输出技能目录摘要，不注入完整指令。
    pub fn generate_catalog_prompt(&self) -> String {
        if self.packages.is_empty() {
            return String::new();
        }
        let lines: Vec<String> = self
            .packages
            .iter()
            .map(|p| format!("- **{}** (`{}`): {}", p.display_name, p.id, p.description))
            .collect();
        format!(
            "{}\n\n调用 `Skill` 工具激活：参数 `skill` 填技能标识符（反引号中的名称，如 `orchestrator`）。",
            lines.join("\n")
        )
    }

    /// 注入所有技能完整指令（兼容回退模式）。
    pub fn generate_full_prompt(&self) -> String {
        if self.packages.is_empty() {
            return String::new();
        }
        self.packages
            .iter()
            .map(|p| format!("### Skill: {}\n\n{}", p.display_name, p.instructions))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    }

    // -----------------------------------------------------------------------
    //  Plan 2 — SkillRouter 辅助方法（阶段一：纯规则匹配）
    // -----------------------------------------------------------------------

    /// 通过 id 查找 SkillPackage（供路由决策使用）。
    pub fn find_package_by_id(&self, skill_id: &str) -> Option<&SkillPackage> {
        self.packages.iter().find(|p| p.id == skill_id || p.slug == skill_id)
    }

    /// 根据 skill 命中结果生成当前轮次的 CapabilityPolicy。
    pub fn policy_from_skill(&self, skill_id: &str) -> CapabilityPolicy {
        let mut policy = CapabilityPolicy {
            source: PolicySource::ActiveSkill,
            ..CapabilityPolicy::default()
        };

        if self.find_package_by_id(skill_id).is_none() {
            policy.source = PolicySource::Default;
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

        // 检查 /skill <name> 模式
        if let Some(rest) = trimmed.strip_prefix("/skill ") {
            if let Some(name) = rest.split_whitespace().next() {
                if let Some(pkg) = self.find_by_slug(name) {
                    return Some(pkg.id.clone());
                }
                if let Some(pkg) = self.find_by_alias(name) {
                    return Some(pkg.id.clone());
                }
                if let Some(pkg) = self.find_by_name(name) {
                    return Some(pkg.id.clone());
                }
            }
        }

        // 检查 /<skill> 直达模式，便于 `/orchestrator ...` 这类显式触发。
        if let Some(rest) = trimmed.strip_prefix('/') {
            if !rest.is_empty() {
                let name = rest.split_whitespace().next()?;
                if name.contains('/') {
                    return None;
                }
                if let Some(pkg) = self.find_by_slug(name) {
                    return Some(pkg.id.clone());
                }
                if let Some(pkg) = self.find_by_alias(name) {
                    return Some(pkg.id.clone());
                }
            }
        }

        None
    }

    /// 根据工具策略生成 Tool 视图（仅供展示，不再用于 turn 级工具裁剪）。
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
