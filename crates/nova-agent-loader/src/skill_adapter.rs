use anyhow::Result;
use nova_agent::skill::{SkillPackage, ToolPolicy};
use nova_skill_loader::{LoadedSkill, LoadedSkillPackage, LoadedToolPolicy};
use std::path::{Path, PathBuf};

pub async fn load_skills(skills_dir: &Path, extra_paths: &[PathBuf]) -> Result<Vec<SkillPackage>> {
    let mut loaded = match nova_skill_loader::load_skills_from_dir_async(skills_dir).await {
        Ok(skills) => skills,
        Err(err) => {
            log::warn!("Failed to load skills from {:?}: {}", skills_dir, err);
            Vec::new()
        }
    };

    for path in extra_paths {
        match nova_skill_loader::load_single_skill(path) {
            Ok(Some(skill)) => loaded.push(skill),
            Ok(None) => log::warn!("Included skill path {:?} did not contain a valid skill", path),
            Err(err) => log::error!("Failed to load included skill from {:?}: {}", path, err),
        }
    }

    let mut packages = convert_loaded_skills(loaded);
    for package in &mut packages {
        apply_preload_overrides(package).await;
    }
    Ok(packages)
}

pub fn convert_loaded_skills(loaded: Vec<LoadedSkill>) -> Vec<SkillPackage> {
    loaded
        .into_iter()
        .map(|skill| match skill {
            LoadedSkill::Package(package) => convert_package(package),
            LoadedSkill::Compat { package, .. } => convert_package(package),
        })
        .collect()
}

fn convert_package(package: LoadedSkillPackage) -> SkillPackage {
    SkillPackage {
        id: package.id,
        slug: package.slug,
        display_name: package.display_name,
        description: package.description,
        instructions: package.instructions,
        tool_policy: convert_tool_policy(package.tool_policy),
        sticky: package.sticky,
        aliases: package.aliases,
        examples: package.examples,
        source_path: package.source_path,
        compat_mode: package.compat_mode,
        // 由同级 preload.toml 注入（见 apply_preload_overrides）；
        // nova-skill-loader 不感知此字段，默认空。
        preload: Vec::new(),
    }
}

fn convert_tool_policy(policy: LoadedToolPolicy) -> ToolPolicy {
    match policy {
        LoadedToolPolicy::InheritAll => ToolPolicy::InheritAll,
        LoadedToolPolicy::AllowList(tools) => ToolPolicy::AllowList(tools),
        LoadedToolPolicy::AllowListWithDeferred(tools) => ToolPolicy::AllowListWithDeferred(tools),
    }
}

/// 读取技能目录下同级 `preload.toml`（`preload = ["tool", ...]`）注入 `SkillPackage.preload`。
///
/// nova-skill-loader 保持零改动：preload 属 nova/zero 运行时关切，由适配层处理。
/// 文件缺失视为空（常见情况，不告警）；解析失败仅告警、不阻断加载。
async fn apply_preload_overrides(package: &mut SkillPackage) {
    let Some(dir) = skill_dir_of(&package.source_path) else {
        return;
    };
    let file = dir.join("preload.toml");
    let content = match tokio::fs::read_to_string(&file).await {
        Ok(content) => content,
        Err(_) => return,
    };
    match toml::from_str::<toml::Value>(&content) {
        Ok(value) => {
            if let Some(items) = value.get("preload").and_then(|v| v.as_array()) {
                package.preload = items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect();
            }
        }
        Err(err) => {
            log::warn!(
                "skill '{}' preload.toml parse failed ({:?}): {}",
                package.slug,
                file,
                err
            );
        }
    }
}

/// 推导技能目录：SKILL.md 技能的 `source_path` 即目录（无扩展名）；
/// skill.toml 技能的 `source_path` 为文件（有扩展名），取其父目录。
/// 用扩展名判定以避免 async 上下文中的同步 fs stat。
fn skill_dir_of(source_path: &Path) -> Option<PathBuf> {
    if source_path.as_os_str().is_empty() {
        return None;
    }
    if source_path.extension().is_some() {
        source_path.parent().map(Path::to_path_buf)
    } else {
        Some(source_path.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::load_skills;
    use std::fs;
    use tempfile::TempDir;

    fn write_skill(dir: &std::path::Path, name: &str, preload: Option<&str>) {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: test {name}\n---\nBODY for {name}\n"),
        )
        .unwrap();
        if let Some(content) = preload {
            fs::write(skill_dir.join("preload.toml"), content).unwrap();
        }
    }

    #[tokio::test]
    async fn reads_sibling_preload_toml() {
        let tmp = TempDir::new().unwrap();
        write_skill(
            tmp.path(),
            "alarm",
            Some("preload = [\"alarm_cli_once\", \"alarm_cli_cron\"]\n"),
        );

        let packages = load_skills(tmp.path(), &[]).await.unwrap();
        let alarm = packages.iter().find(|p| p.slug == "alarm").expect("alarm skill loaded");
        assert_eq!(
            alarm.preload,
            vec!["alarm_cli_once".to_string(), "alarm_cli_cron".to_string()]
        );
        assert!(alarm.instructions.contains("BODY for alarm"));
    }

    #[tokio::test]
    async fn missing_preload_toml_yields_empty_preload() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "noload", None);

        let packages = load_skills(tmp.path(), &[]).await.unwrap();
        let skill = packages.iter().find(|p| p.slug == "noload").expect("skill loaded");
        assert!(skill.preload.is_empty());
    }

    #[tokio::test]
    async fn malformed_preload_toml_is_non_fatal() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "bad", Some("preload = not-a-valid-array ["));

        let packages = load_skills(tmp.path(), &[]).await.unwrap();
        let skill = packages.iter().find(|p| p.slug == "bad").expect("skill still loaded");
        assert!(skill.preload.is_empty());
    }
}
