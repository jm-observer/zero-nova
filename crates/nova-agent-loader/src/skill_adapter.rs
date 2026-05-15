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

    Ok(convert_loaded_skills(loaded))
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
    }
}

fn convert_tool_policy(policy: LoadedToolPolicy) -> ToolPolicy {
    match policy {
        LoadedToolPolicy::InheritAll => ToolPolicy::InheritAll,
        LoadedToolPolicy::AllowList(tools) => ToolPolicy::AllowList(tools),
        LoadedToolPolicy::AllowListWithDeferred(tools) => ToolPolicy::AllowListWithDeferred(tools),
    }
}
