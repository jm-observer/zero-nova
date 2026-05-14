use crate::skill::{SkillPackage, ToolPolicy};
use nova_skill_loader::{LoadedSkill, LoadedSkillPackage, LoadedToolPolicy};

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
