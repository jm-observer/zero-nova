use super::SkillRegistry;
use super::ToolPolicy;
use super::{Skill, SkillPackage};
use anyhow::Result;
use std::path::Path;

impl SkillRegistry {
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

    /// 异步加载单个目录中的技能。
    pub async fn load_single_skill_async<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref();

        let skill_toml_path = path.join("skill.toml");
        if skill_toml_path.exists() {
            match self.parse_skill_toml_async(&skill_toml_path).await {
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

        let skill_md = path.join("SKILL.md");
        if skill_md.exists() {
            match self.parse_skill_file_async(&skill_md).await {
                Ok(skill) => {
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
        let content = read_to_string_runtime_aware(path)?;
        self.parse_skill_content(path, content)
    }

    /// 异步读取 SKILL.md 并解析为旧 Skill 结构。
    async fn parse_skill_file_async(&self, path: &Path) -> Result<Skill> {
        let content = tokio::fs::read_to_string(path).await?;
        self.parse_skill_content(path, content)
    }

    fn parse_skill_content(&self, path: &Path, content: String) -> Result<Skill> {
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
        let content = read_to_string_runtime_aware(path)?;
        self.parse_skill_toml_content(path, content)
    }

    /// 异步读取 skill.toml 并解析为 SkillPackage。
    async fn parse_skill_toml_async(&self, path: &Path) -> Result<SkillPackage> {
        let content = tokio::fs::read_to_string(path).await?;
        self.parse_skill_toml_content(path, content)
    }

    fn parse_skill_toml_content(&self, path: &Path, content: String) -> Result<SkillPackage> {
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
}

/// 启动期/测试同步读取辅助。
/// 运行时热路径应优先使用 async discovery API（load_from_dir_async）。
fn read_to_string_runtime_aware(path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}
