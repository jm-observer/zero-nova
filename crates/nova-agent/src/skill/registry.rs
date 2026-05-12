#[cfg(test)]
use super::types::FileToolPriority;
use super::types::{CapabilityPolicy, PolicySource, Skill, SkillPackage, ToolPolicy};

mod discovery;
mod filter;
mod parser;

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

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
        assert!(policy.always_enabled_tools.contains(&"ProjectManager".to_string()));
        assert!(policy.always_enabled_tools.contains(&"Agent".to_string()));
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

    #[test]
    fn catalog_prompt_has_no_full_instructions() {
        let mut registry = SkillRegistry::new();
        registry.packages.push(SkillPackage {
            id: "skill-1".to_string(),
            slug: "skill-1".to_string(),
            display_name: "Skill One".to_string(),
            description: "First skill".to_string(),
            instructions: "Full instructions for skill one".to_string(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec![],
            examples: vec![],
            source_path: PathBuf::from("skill-1"),
            compat_mode: false,
        });
        let prompt = registry.generate_catalog_prompt();
        assert!(prompt.contains("### Available Skills"));
        assert!(!prompt.contains("Full instructions for skill one"));
    }

    #[test]
    fn full_prompt_includes_all_instructions() {
        let mut registry = SkillRegistry::new();
        registry.packages.push(SkillPackage {
            id: "skill-1".to_string(),
            slug: "skill-1".to_string(),
            display_name: "Skill One".to_string(),
            description: "First skill".to_string(),
            instructions: "Instr1".to_string(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec![],
            examples: vec![],
            source_path: PathBuf::from("skill-1"),
            compat_mode: false,
        });
        registry.packages.push(SkillPackage {
            id: "skill-2".to_string(),
            slug: "skill-2".to_string(),
            display_name: "Skill Two".to_string(),
            description: "Second skill".to_string(),
            instructions: "Instr2".to_string(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec![],
            examples: vec![],
            source_path: PathBuf::from("skill-2"),
            compat_mode: false,
        });
        let prompt = registry.generate_full_prompt();
        assert!(prompt.contains("Instr1"));
        assert!(prompt.contains("Instr2"));
    }

    #[test]
    fn match_skill_by_input_supports_skill_space_form() {
        let mut registry = SkillRegistry::new();
        registry.packages.push(SkillPackage {
            id: "orchestrator".to_string(),
            slug: "orchestrator".to_string(),
            display_name: "Orchestrator".to_string(),
            description: "multi-agent orchestrator".to_string(),
            instructions: "instructions".to_string(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec!["multi-agent".to_string()],
            examples: vec![],
            source_path: PathBuf::from("orchestrator"),
            compat_mode: false,
        });

        assert_eq!(
            registry.match_skill_by_input("/skill orchestrator"),
            Some("orchestrator".to_string())
        );
    }

    #[test]
    fn match_skill_by_input_supports_direct_skill_slash_form() {
        let mut registry = SkillRegistry::new();
        registry.packages.push(SkillPackage {
            id: "orchestrator".to_string(),
            slug: "orchestrator".to_string(),
            display_name: "Orchestrator".to_string(),
            description: "multi-agent orchestrator".to_string(),
            instructions: "instructions".to_string(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec!["multi-agent".to_string()],
            examples: vec![],
            source_path: PathBuf::from("orchestrator"),
            compat_mode: false,
        });

        assert_eq!(
            registry.match_skill_by_input("/orchestrator 分两步完成任务"),
            Some("orchestrator".to_string())
        );
        assert_eq!(
            registry.match_skill_by_input("/multi-agent 执行这个任务"),
            Some("orchestrator".to_string())
        );
    }

    #[tokio::test]
    async fn load_from_dir_async_loads_toml_and_markdown_skills() {
        let root = tempdir().unwrap();
        let skill_a = root.path().join("a");
        let skill_b = root.path().join("nested").join("b");
        tokio::fs::create_dir_all(&skill_a).await.unwrap();
        tokio::fs::create_dir_all(&skill_b).await.unwrap();

        tokio::fs::write(
            skill_a.join("skill.toml"),
            r#"
slug = "skill-a"
display_name = "Skill A"
description = "desc a"
instructions = "do a"
"#,
        )
        .await
        .unwrap();

        tokio::fs::write(
            skill_b.join("SKILL.md"),
            r#"---
name: "Skill B"
description: "desc b"
---
do b
"#,
        )
        .await
        .unwrap();

        let mut registry = SkillRegistry::new();
        registry.load_from_dir_async(root.path()).await.unwrap();

        assert_eq!(registry.packages.len(), 2);
        assert!(registry.find_by_slug("skill-a").is_some());
        assert!(registry.find_by_name("Skill B").is_some());
    }
}
