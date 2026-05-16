use crate::config::AgentSpec;
use crate::prompt::context::EnvironmentSnapshot;
use crate::prompt::templates::{TemplateContext, BEHAVIOR_GUARDS};
use crate::prompt::types::{
    AgentCatalogEntry, NamedSection, ProjectInstructionProfile, PromptConstructionRequest, PromptExtraSections,
    PromptMaterial, PromptPriority, PromptSectionSize, SectionName, SkillInjectionMode, ToolGuidanceMode, ToolSize,
    TurnPromptMaterial,
};
use crate::provider::types::ToolDefinition;
use crate::skill::SkillRegistry;
use crate::tool::ToolRegistry;
use std::collections::HashMap;

#[derive(Default)]
pub struct SystemPromptBuilder {
    sections: Vec<(SectionName, NamedSection)>,
}

impl SystemPromptBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_section(mut self, name: SectionName, content: impl Into<String>, priority: PromptPriority) -> Self {
        let content_val: String = content.into();
        if !content_val.is_empty() {
            self.sections.push((
                name.clone(),
                NamedSection {
                    name,
                    content: content_val,
                    required: priority == PromptPriority::High,
                    priority,
                },
            ));
        }
        self
    }

    pub fn base_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::Base, content, PromptPriority::High)
    }

    pub fn agent_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::Agent, content, PromptPriority::High)
    }

    pub fn skill_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::Skill, content, PromptPriority::Medium)
    }

    pub fn environment_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::Environment, content, PromptPriority::High)
    }

    pub fn workflow_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::Workflow, content, PromptPriority::Medium)
    }

    pub fn tool_guidance_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::ToolGuidance, content, PromptPriority::Medium)
    }

    pub fn history_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::History, content, PromptPriority::Low)
    }

    pub fn role(mut self, role: impl Into<String>) -> Self {
        self.sections.push((
            SectionName::Base,
            NamedSection {
                name: SectionName::Base,
                content: format!("Role: {}", role.into()),
                required: true,
                priority: PromptPriority::High,
            },
        ));
        self
    }

    pub fn guideline(mut self, text: impl Into<String>) -> Self {
        self.sections.push((
            SectionName::Base,
            NamedSection {
                name: SectionName::Base,
                content: format!("Guideline: {}", text.into()),
                required: true,
                priority: PromptPriority::High,
            },
        ));
        self
    }

    pub fn environment(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.sections.push((
            SectionName::Environment,
            NamedSection {
                name: SectionName::Environment,
                content: format!("Environment {} = {}", key.into(), value.into()),
                required: false,
                priority: PromptPriority::Medium,
            },
        ));
        self
    }

    pub fn custom_instruction(mut self, text: impl Into<String>) -> Self {
        self.sections.push((
            SectionName::Workflow,
            NamedSection {
                name: SectionName::Workflow,
                content: format!("Instruction: {}", text.into()),
                required: false,
                priority: PromptPriority::Medium,
            },
        ));
        self
    }

    pub fn extra_section(mut self, text: impl Into<String>) -> Self {
        self.sections.push((
            SectionName::Base,
            NamedSection {
                name: SectionName::Base,
                content: text.into(),
                required: false,
                priority: PromptPriority::Low,
            },
        ));
        self
    }

    pub fn behavior_guards_section(self) -> Self {
        self.add_section(
            SectionName::BehaviorGuards,
            BEHAVIOR_GUARDS.trim(),
            PromptPriority::High,
        )
    }

    pub fn project_context_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::ProjectContext, content, PromptPriority::Medium)
    }

    pub fn developer_project_prompt_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::DeveloperProjectPrompt, content, PromptPriority::Medium)
    }

    pub fn agent_catalog_section(self, content: impl Into<String>) -> Self {
        self.add_section(SectionName::AgentCatalog, content, PromptPriority::Medium)
    }

    pub fn environment_snapshot(self, env: &EnvironmentSnapshot) -> Self {
        self.add_section(SectionName::Environment, env.to_prompt_text(), PromptPriority::High)
    }

    fn with_tool_definitions_internal(
        mut self,
        definitions: &[ToolDefinition],
        visible_tool_names: &std::collections::HashSet<String>,
        mode: ToolGuidanceMode,
    ) -> Self {
        let mut tool_desc = String::new();
        for def in definitions {
            match mode {
                ToolGuidanceMode::Compact => {
                    tool_desc.push_str(&format!("- `{}`: {}\n", def.name, def.description));
                }
                ToolGuidanceMode::Full => {
                    tool_desc.push_str(&format!("## {}\n\n{}\n\n", def.name, def.description));
                }
            }
        }

        let tool_info_visible = visible_tool_names.contains(crate::tool::builtin::tool_info::TOOL_NAME);
        if !definitions.is_empty() && tool_info_visible {
            tool_desc.push_str("---\n\n**Tool parameter lookup**: If you need exact parameters, field types, required/default/enum values, or nested object structures, call the `ToolInfo` tool first. Do not guess tool parameters based on experience.\n\n");
        }

        if let Some((_, section)) = self
            .sections
            .iter_mut()
            .rev()
            .find(|(name, _)| *name == SectionName::ToolGuidance)
        {
            section.content.push_str(&tool_desc);
        } else {
            self.sections.push((
                SectionName::ToolGuidance,
                NamedSection {
                    name: SectionName::ToolGuidance,
                    content: tool_desc,
                    required: false,
                    priority: PromptPriority::Medium,
                },
            ));
        }
        self
    }

    pub async fn with_tools(self, registry: &ToolRegistry) -> Self {
        let definitions: Vec<ToolDefinition> = registry
            .loaded_definitions()
            .await
            .into_iter()
            .map(|def| ToolDefinition {
                name: def.name,
                description: def.description,
                input_schema: def.input_schema,
            })
            .collect();
        self.with_tool_definitions(&definitions, ToolGuidanceMode::Full)
    }

    pub fn with_tool_definitions(self, definitions: &[ToolDefinition], mode: ToolGuidanceMode) -> Self {
        let visible_tool_names: std::collections::HashSet<String> =
            definitions.iter().map(|definition| definition.name.clone()).collect();
        self.with_tool_definitions_internal(definitions, &visible_tool_names, mode)
    }

    pub fn size_report(&self, large_section_chars: usize) -> Vec<PromptSectionSize> {
        self.sections
            .iter()
            .map(|(name, section)| PromptSectionSize {
                name: name.clone(),
                heading: name.heading().to_string(),
                chars: section.content.chars().count(),
                priority: section.priority.clone(),
                required: section.required,
                is_large: section.content.chars().count() > large_section_chars,
            })
            .collect()
    }

    pub fn tool_size_report(definitions: &[ToolDefinition]) -> Vec<ToolSize> {
        definitions
            .iter()
            .map(|tool| ToolSize {
                name: tool.name.clone(),
                chars: Self::single_tool_chars(tool),
            })
            .collect()
    }

    fn single_tool_chars(tool: &ToolDefinition) -> usize {
        let schema = serde_json::to_string(&tool.input_schema).unwrap_or_else(|_| "{}".to_string());
        [tool.name.as_str(), tool.description.as_str(), schema.as_str()]
            .join("")
            .chars()
            .count()
    }

    /// 从纯内容模型构建 prompt，不执行文件 IO。
    pub fn from_material(
        material: &PromptMaterial,
        turn_material: &TurnPromptMaterial,
        skills: &SkillRegistry,
    ) -> Self {
        let mut builder = Self::new();
        let mut template_vars = material.initial_template_vars.clone();
        template_vars.extend(turn_material.turn_template_vars.clone());

        let rendered_prompt = if template_vars.is_empty() {
            material.agent_prompt.clone()
        } else {
            TemplateContext::render(&material.agent_prompt, &template_vars)
        };
        if !rendered_prompt.is_empty() {
            builder = builder.base_section(&rendered_prompt);
        }

        builder = builder.behavior_guards_section();

        let skill_prompt = match material.skill_injection_mode {
            SkillInjectionMode::Catalog => skills.generate_catalog_prompt(),
            SkillInjectionMode::ActiveFull => skills.generate_contextual_prompt(turn_material.active_skill.as_deref()),
            SkillInjectionMode::Full => skills.generate_full_prompt(),
        };
        if !skill_prompt.is_empty() {
            builder = builder.skill_section(&skill_prompt);
        }

        if let Some(content) = &turn_material.developer_project_prompt {
            builder = builder.developer_project_prompt_section(filter_project_instruction_by_profile(
                content,
                material.project_instruction_profile,
            ));
        }

        if let Some(content) = &turn_material.project_context {
            builder = builder.project_context_section(content);
        }

        if let Some(env) = &material.environment_snapshot {
            builder = builder.environment_snapshot(env);
        }

        if let Some(ref catalog) = material.agent_catalog {
            if !catalog.is_empty() {
                builder = builder.agent_catalog_section(catalog);
            }
        }

        if let Some(content) = &turn_material.workflow_prompt {
            if !content.is_empty() {
                builder = builder.workflow_section(content);
            }
        }

        builder
    }

    /// 根据构造请求构建 prompt（统一的主入口）。
    ///
    /// 这是所有 Agent（包括主 Agent 和子 Agent）构建 system prompt 的统一方式。
    /// 取代之前的"双轨制"实现。
    ///
    /// 额外参数用于包含 turn-specific sections（developer_project_prompt, project_context,
    /// workflow_prompt, environment_snapshot）。如果 `build_from_request` 的未来版本需要
    /// 这些字段，可以将它们移动到 `PromptConstructionRequest` 中。
    pub fn build_from_request(
        &self,
        request: &PromptConstructionRequest,
        name_overrides: &HashMap<String, String>,
        skill_registry: &SkillRegistry,
        extra_sections: PromptExtraSections,
    ) -> String {
        let mut builder = Self::new();

        // 1. Base section — 构建基础 prompt（合并 template vars）
        let mut template_vars = request.initial_template_vars.clone();
        template_vars.extend(request.context_overrides.clone());

        // 优先使用 extra_sections 中的 system_prompt_base，否则使用请求自带的 base prompt。
        let base_prompt = if let Some(ref base) = extra_sections.system_prompt_base {
            if !base.is_empty() {
                base.clone()
            } else if !request.base_prompt.is_empty() {
                request.base_prompt.clone()
            } else if let Some(name) = name_overrides.get(&request.base_material_id) {
                name.clone()
            } else {
                format!("Agent task: {}", request.base_material_id)
            }
        } else if !request.base_prompt.is_empty() {
            request.base_prompt.clone()
        } else if let Some(name) = name_overrides.get(&request.base_material_id) {
            name.clone()
        } else {
            format!("Agent task: {}", request.base_material_id)
        };

        let rendered_prompt = if template_vars.is_empty() {
            base_prompt.clone()
        } else {
            TemplateContext::render(&base_prompt, &template_vars)
        };
        if !rendered_prompt.is_empty() {
            builder = builder.base_section(&rendered_prompt);
        }

        // 2. Behavior guards
        builder = builder.behavior_guards_section();

        // 3. Skill section — 根据 injection_mode 注入技能
        let skill_prompt = match request.injection_mode {
            SkillInjectionMode::Catalog => skill_registry.generate_catalog_prompt(),
            SkillInjectionMode::ActiveFull => skill_registry.generate_contextual_prompt(request.skill_id.as_deref()),
            SkillInjectionMode::Full => skill_registry.generate_full_prompt(),
        };
        if !skill_prompt.is_empty() {
            builder = builder.skill_section(&skill_prompt);
        }

        // 4. 额外 sections（来自 TurnPromptMaterial / PromptMaterial）
        if let Some(ref content) = extra_sections.developer_project_prompt {
            builder = builder.developer_project_prompt_section(filter_project_instruction_by_profile(
                content,
                request.project_instruction_profile,
            ));
        }

        if let Some(ref content) = extra_sections.project_context {
            builder = builder.project_context_section(content);
        }

        if let Some(ref env) = extra_sections.environment_snapshot {
            builder = builder.environment_snapshot(env);
        }

        if let Some(ref catalog) = request.agent_catalog {
            if !catalog.is_empty() {
                builder = builder.agent_catalog_section(catalog);
            }
        }

        if let Some(ref content) = extra_sections.workflow_prompt {
            if !content.is_empty() {
                builder = builder.workflow_section(content);
            }
        }

        // 5. Tool guidance section
        builder = builder.with_tool_definitions_internal(
            &request.tool_definitions,
            request.visible_tool_names.as_ref(),
            request.tool_guidance,
        );

        // 6. Build and return
        builder.build()
    }

    pub fn build(&self) -> String {
        self.sections
            .iter()
            .filter(|(_, section)| !section.content.is_empty())
            .map(|(name, section)| format!("## {}\n\n{}", name.heading(), section.content))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    }

    pub fn debug_sections(&self) -> Vec<String> {
        self.sections
            .iter()
            .map(|(name, section)| {
                format!(
                    "{:?}: {} ({:?}, required={})",
                    name,
                    if section.content.is_empty() { "empty" } else { "present" },
                    section.priority,
                    section.required
                )
            })
            .collect()
    }

    pub fn get_section(&self, name: &SectionName) -> Option<&str> {
        self.sections
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, section)| section.content.as_str())
    }
}

pub fn filter_project_instruction_by_profile(content: &str, profile: ProjectInstructionProfile) -> String {
    if matches!(profile, ProjectInstructionProfile::Full) {
        return content.to_string();
    }
    let normalized = if matches!(profile, ProjectInstructionProfile::Auto) {
        ProjectInstructionProfile::Code
    } else {
        profile
    };

    let mut output = Vec::new();
    let mut current_heading = String::new();
    let mut current_lines: Vec<String> = Vec::new();
    let flush = |heading: &str, lines: &[String], dst: &mut Vec<String>| {
        if heading.is_empty() {
            return;
        }
        let keep = match normalized {
            ProjectInstructionProfile::Analysis => matches!(heading, "基本行为" | "代码结构"),
            ProjectInstructionProfile::Code => {
                matches!(heading, "基本行为" | "技术栈" | "代码结构" | "代码质量" | "修复流程")
            }
            ProjectInstructionProfile::Design => matches!(heading, "基本行为" | "计划与设计文档"),
            ProjectInstructionProfile::Review => matches!(heading, "基本行为" | "代码质量"),
            ProjectInstructionProfile::Auto | ProjectInstructionProfile::Full => false,
        };
        if keep {
            dst.extend(lines.iter().cloned());
        }
    };

    for line in content.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            flush(&current_heading, &current_lines, &mut output);
            current_heading = heading.trim().to_string();
            current_lines.clear();
        }
        current_lines.push(line.to_string());
    }
    flush(&current_heading, &current_lines, &mut output);

    if output.is_empty() {
        content.to_string()
    } else {
        output.join("\n")
    }
}

pub fn build_agent_catalog_section(agents: &[AgentSpec], primary_agent_id: &str) -> String {
    if agents.is_empty() {
        return String::new();
    }

    let mut lines = vec![
        "## Available Agents".to_string(),
        String::new(),
        "The following agents are available for task execution.".to_string(),
        "Choose from this list only — do not invent new agent names.".to_string(),
        String::new(),
        "| ID | Display Name | Default | Description | Use Cases |".to_string(),
        "|----|-------------|---------|-------------|-----------|".to_string(),
    ];

    for agent in agents {
        let is_default = agent.id == primary_agent_id;
        let default_mark = if is_default { "✓" } else { " " };
        let use_cases = if agent.description.is_empty() {
            "general".to_string()
        } else {
            agent.description.clone()
        };
        lines.push(format!(
            "| {} | {} | {} | {} | {} |",
            agent.id,
            agent.display_name,
            default_mark,
            use_cases,
            agent.aliases.join(", ")
        ));
    }

    lines.push(String::new());
    lines.push("Rules:".to_string());
    lines.push("- Always select an agent from the list above.".to_string());
    lines.push("- If unsure, use the default agent.".to_string());
    lines.push("- Do not use natural language names like \"reviewer\" or \"coder\".".to_string());
    lines.push("- The `subagent_type` field is deprecated; use the agent `id` directly.".to_string());

    lines.join("\n")
}

pub fn build_agent_catalog_hint(agents: &[AgentSpec], primary_agent_id: &str) -> String {
    if agents.is_empty() {
        return String::new();
    }

    let mut entries: Vec<AgentCatalogEntry> = agents
        .iter()
        .map(|agent| AgentCatalogEntry {
            id: agent.id.clone(),
            display_name: agent.display_name.clone(),
            description: agent.description.clone(),
            is_default: agent.id == primary_agent_id,
            use_cases: agent.aliases.clone(),
        })
        .collect();
    entries.sort_by(|a, b| a.id.cmp(&b.id));

    let agent_list = entries
        .into_iter()
        .map(|entry| {
            let default_note = if entry.is_default { " (default)" } else { "" };
            format!("`{}`: {}{}", entry.id, entry.display_name, default_note)
        })
        .collect::<Vec<_>>()
        .join("; ");

    format!(
        "Available agent IDs for orchestration: {}. Do NOT use natural language names like 'reviewer', 'coder', or 'researcher'. If unsure, use the default agent.",
        agent_list
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::types::SkillInjectionMode;
    use crate::skill::{SkillPackage, ToolPolicy};
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn empty_builder_produces_empty_string() {
        let builder = SystemPromptBuilder::new();
        assert_eq!(builder.build(), "");
    }

    #[test]
    fn section_with_content_is_included() {
        let builder = SystemPromptBuilder::new()
            .base_section("Base content")
            .agent_section("Agent content");
        let result = builder.build();
        assert!(result.contains("## Identity & Role\n\nBase content"));
        assert!(result.contains("## Agent Configuration\n\nAgent content"));
    }

    #[test]
    fn project_instruction_profile_code_filters_sections() {
        let raw = "## 基本行为\nA\n## 计划与设计文档\nB\n## 代码质量\nC";
        let filtered = filter_project_instruction_by_profile(raw, ProjectInstructionProfile::Code);
        assert!(filtered.contains("## 基本行为"));
        assert!(filtered.contains("## 代码质量"));
        assert!(!filtered.contains("## 计划与设计文档"));
    }

    #[test]
    fn template_context_render_replaces_vars() {
        let mut vars = HashMap::new();
        vars.insert("workflow_stage".into(), "idle".into());
        vars.insert("pending_interaction".into(), "none".into());
        let result = TemplateContext::render("Stage: {{workflow_stage}}, Pending: {{pending_interaction}}", &vars);
        assert_eq!(result, "Stage: idle, Pending: none");
    }

    #[test]
    fn developer_prompt_section_heading() {
        assert_eq!(
            SectionName::DeveloperProjectPrompt.heading(),
            crate::prompt::templates::DEVELOPER_PROMPT_SECTION_HEADING
        );
    }

    #[test]
    fn from_material_merges_vars_and_sections() {
        let mut skills = SkillRegistry::new();
        skills.packages.push(SkillPackage {
            id: "skill-a".to_string(),
            slug: "skill-a".to_string(),
            display_name: "Skill A".to_string(),
            description: "desc".to_string(),
            instructions: "instruction".to_string(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec![],
            examples: vec![],
            source_path: PathBuf::from("skill-a"),
            compat_mode: false,
        });

        let mut initial_vars = HashMap::new();
        initial_vars.insert("name".to_string(), "base".to_string());
        initial_vars.insert("override".to_string(), "from_base".to_string());

        let mut turn_vars = HashMap::new();
        turn_vars.insert("override".to_string(), "from_turn".to_string());

        let material = PromptMaterial {
            agent_id: "agent-a".to_string(),
            agent_prompt: "Hello {{name}} {{override}}".to_string(),
            agent_catalog: Some("catalog".to_string()),
            environment_snapshot: None,
            initial_template_vars: initial_vars.clone(),
            skill_injection_mode: SkillInjectionMode::Catalog,
            project_instruction_profile: ProjectInstructionProfile::Code,
            tool_guidance: ToolGuidanceMode::Compact,
        };
        let turn_material = TurnPromptMaterial {
            developer_project_prompt: Some("## 基本行为\nA\n## 计划与设计文档\nB".to_string()),
            project_context: Some("context".to_string()),
            workflow_prompt: Some("workflow".to_string()),
            turn_template_vars: turn_vars.clone(),
            active_skill: None,
        };

        let output = SystemPromptBuilder::from_material(&material, &turn_material, &skills).build();
        assert!(output.contains("Hello base from_turn"));
        assert!(output.contains("## Available Skills"));
        assert!(output.contains("context"));
        assert!(output.contains("workflow"));
    }

    #[test]
    fn build_from_request_matches_from_material_semantics() {
        let mut skills = SkillRegistry::new();
        skills.packages.push(SkillPackage {
            id: "skill-a".to_string(),
            slug: "skill-a".to_string(),
            display_name: "Skill A".to_string(),
            description: "desc".to_string(),
            instructions: "instruction".to_string(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec![],
            examples: vec![],
            source_path: PathBuf::from("skill-a"),
            compat_mode: false,
        });

        let mut initial_vars = HashMap::new();
        initial_vars.insert("name".to_string(), "base".to_string());
        initial_vars.insert("override".to_string(), "from_base".to_string());

        let mut turn_vars = HashMap::new();
        turn_vars.insert("override".to_string(), "from_turn".to_string());

        let tool_definitions = vec![ToolDefinition {
            name: "ToolInfo".to_string(),
            description: "lookup".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        }];
        let visible_tool_names = std::collections::HashSet::from(["ToolInfo".to_string()]);

        let material = PromptMaterial {
            agent_id: "agent-a".to_string(),
            agent_prompt: "Hello {{name}} {{override}}".to_string(),
            agent_catalog: Some("catalog".to_string()),
            environment_snapshot: None,
            initial_template_vars: initial_vars.clone(),
            skill_injection_mode: SkillInjectionMode::Catalog,
            project_instruction_profile: ProjectInstructionProfile::Code,
            tool_guidance: ToolGuidanceMode::Compact,
        };
        let turn_material = TurnPromptMaterial {
            developer_project_prompt: Some("## 基本行为\nA\n## 计划与设计文档\nB".to_string()),
            project_context: Some("context".to_string()),
            workflow_prompt: Some("workflow".to_string()),
            turn_template_vars: turn_vars.clone(),
            active_skill: None,
        };
        let extra_sections = PromptExtraSections {
            system_prompt_base: None,
            developer_project_prompt: turn_material.developer_project_prompt.clone(),
            project_context: turn_material.project_context.clone(),
            workflow_prompt: turn_material.workflow_prompt.clone(),
            environment_snapshot: material.environment_snapshot.clone(),
        };
        let request = PromptConstructionRequest {
            base_material_id: material.agent_id.clone(),
            base_prompt: material.agent_prompt.clone(),
            skill_id: None,
            injection_mode: material.skill_injection_mode,
            initial_template_vars: material.initial_template_vars.clone(),
            context_overrides: turn_material.turn_template_vars.clone(),
            original_base_user_message: None,
            tool_definitions: std::sync::Arc::new(tool_definitions),
            visible_tool_names: std::sync::Arc::new(visible_tool_names),
            project_instruction_profile: material.project_instruction_profile,
            tool_guidance: material.tool_guidance,
            agent_catalog: material.agent_catalog.clone(),
        };

        let from_material_output = SystemPromptBuilder::from_material(&material, &turn_material, &skills)
            .with_tool_definitions(request.tool_definitions.as_ref(), material.tool_guidance)
            .build();
        let from_request_output =
            SystemPromptBuilder::default().build_from_request(&request, &HashMap::new(), &skills, extra_sections);

        assert_eq!(from_request_output, from_material_output);
    }
}
