use anyhow::{bail, Context, Result};
use nova_agent::prompt::context::{
    load_developer_project_prompt_async, load_project_context_with_config_async, EnvironmentSnapshot,
};
use nova_agent::prompt::types::{
    ProjectInstructionProfile, PromptMaterial, SkillInjectionMode, ToolGuidanceMode, TurnPromptMaterial,
};
use nova_agent::prompt::workflow::WorkflowStagePrompts;
use nova_agent_config::{AgentSpec, AppConfig};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct PromptLoaderConfig {
    pub config_dir: PathBuf,
    pub prompts_dir: PathBuf,
    pub project_context_file: Option<PathBuf>,
    pub developer_prompt_files: Vec<String>,
}

impl From<&AppConfig> for PromptLoaderConfig {
    fn from(config: &AppConfig) -> Self {
        Self {
            config_dir: config.config_dir.clone(),
            prompts_dir: config.prompts_dir(),
            project_context_file: config.project_context_file(),
            developer_prompt_files: config.developer_prompt_files.clone(),
        }
    }
}

pub struct PromptMaterialLoader {
    pub config_dir: PathBuf,
    pub prompts_dir: PathBuf,
    pub project_context_file: Option<PathBuf>,
    pub developer_prompt_files: Vec<String>,
}

impl PromptMaterialLoader {
    pub fn from_config(config: &PromptLoaderConfig) -> Self {
        Self {
            config_dir: config.config_dir.clone(),
            prompts_dir: config.prompts_dir.clone(),
            project_context_file: config.project_context_file.clone(),
            developer_prompt_files: config.developer_prompt_files.clone(),
        }
    }

    pub async fn load_agent_prompt(&self, spec: &AgentSpec) -> Result<String> {
        if spec.prompt_file.is_some() && spec.prompt_inline.is_some() {
            bail!(
                "Agent '{}' has both prompt_file and prompt_inline configured; only one is allowed",
                spec.id
            );
        }

        if let Some(file) = &spec.prompt_file {
            let prompt_path = self.prompts_dir.join(file);
            let content = tokio::fs::read_to_string(&prompt_path)
                .await
                .with_context(|| format!("Failed to read prompt_file for agent '{}': {:?}", spec.id, prompt_path))?;
            return Ok(content);
        }

        if let Some(inline) = &spec.prompt_inline {
            return Ok(inline.clone());
        }

        if let Some(legacy) = &spec.system_prompt_template {
            log::warn!(
                "Agent '{}' uses legacy system_prompt_template. This field is deprecated; use prompt_file/prompt_inline.",
                spec.id
            );
            return Ok(legacy.clone());
        }

        let default_file = format!("agent-{}.md", spec.id);
        let prompt_path = self.prompts_dir.join(&default_file);
        match tokio::fs::read_to_string(&prompt_path).await {
            Ok(content) => Ok(content),
            Err(err) => {
                log::warn!(
                    "Default prompt file {:?} not found for agent '{}': {}",
                    prompt_path,
                    spec.id,
                    err
                );
                Ok(String::new())
            }
        }
    }

    pub async fn load_agent_material(
        &self,
        spec: &AgentSpec,
        env: Option<EnvironmentSnapshot>,
        agent_catalog: Option<String>,
        template_vars: HashMap<String, String>,
    ) -> Result<PromptMaterial> {
        let agent_prompt = self.load_agent_prompt(spec).await?;
        Ok(PromptMaterial {
            agent_id: spec.id.clone(),
            agent_prompt,
            agent_catalog,
            environment_snapshot: env,
            initial_template_vars: template_vars,
            skill_injection_mode: SkillInjectionMode::Catalog,
            project_instruction_profile: ProjectInstructionProfile::Auto,
            tool_guidance: ToolGuidanceMode::Compact,
        })
    }

    pub async fn load_turn_material(
        &self,
        project_dir: Option<&Path>,
        workflow_stage: Option<&str>,
        active_skill: Option<String>,
        turn_vars: HashMap<String, String>,
        enable_developer_prompt: bool,
    ) -> Result<TurnPromptMaterial> {
        let developer_project_prompt = if enable_developer_prompt {
            load_developer_project_prompt_async(project_dir, &self.developer_prompt_files).await
        } else {
            None
        };

        let project_context =
            load_project_context_with_config_async(project_dir, self.project_context_file.as_deref()).await;
        let workflow_prompt = self.load_workflow_prompt(workflow_stage, &turn_vars).await;

        Ok(TurnPromptMaterial {
            developer_project_prompt,
            project_context,
            workflow_prompt,
            turn_template_vars: turn_vars,
            active_skill,
        })
    }

    async fn load_workflow_prompt(&self, stage: Option<&str>, vars: &HashMap<String, String>) -> Option<String> {
        let stage = stage?;
        if stage == "idle" {
            return None;
        }
        let path = self.prompts_dir.join("workflow-stages.md");
        let prompts = match WorkflowStagePrompts::load_from_file_async(&path).await {
            Ok(prompts) => prompts,
            Err(err) => {
                log::warn!(
                    "Failed to load workflow prompt stage '{}' from {:?}: {}",
                    stage,
                    path,
                    err
                );
                return None;
            }
        };
        prompts.render(stage, vars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_agent_config::ConfiguredAgentModel;
    use tempfile::tempdir;

    fn test_agent_spec() -> AgentSpec {
        AgentSpec {
            id: "demo".to_string(),
            display_name: "Demo".to_string(),
            description: "demo".to_string(),
            aliases: vec![],
            provider: "default".to_string(),
            llm: "default".to_string(),
            prompt_file: None,
            prompt_inline: None,
            system_prompt_template: None,
            model_config: ConfiguredAgentModel {
                model: "gpt-test".to_string(),
                temperature: 0.0,
                max_tokens: None,
                top_p: 1.0,
            },
            enable_project_developer_prompt: false,
        }
    }

    #[tokio::test]
    async fn load_agent_prompt_prioritizes_file_over_legacy() {
        let dir = tempdir().unwrap();
        let prompts_dir = dir.path().join("prompts");
        tokio::fs::create_dir_all(&prompts_dir).await.unwrap();
        tokio::fs::write(prompts_dir.join("custom.md"), "from-file")
            .await
            .unwrap();

        let loader = PromptMaterialLoader {
            config_dir: dir.path().to_path_buf(),
            prompts_dir,
            project_context_file: None,
            developer_prompt_files: vec![],
        };
        let mut spec = test_agent_spec();
        spec.prompt_file = Some("custom.md".to_string());
        spec.system_prompt_template = Some("from-legacy".to_string());

        let content = loader.load_agent_prompt(&spec).await.unwrap();
        assert_eq!(content, "from-file");
    }

    #[tokio::test]
    async fn load_agent_prompt_uses_inline_when_file_not_set() {
        let dir = tempdir().unwrap();
        let loader = PromptMaterialLoader {
            config_dir: dir.path().to_path_buf(),
            prompts_dir: dir.path().join("prompts"),
            project_context_file: None,
            developer_prompt_files: vec![],
        };
        let mut spec = test_agent_spec();
        spec.prompt_inline = Some("from-inline".to_string());
        spec.system_prompt_template = Some("from-legacy".to_string());

        let content = loader.load_agent_prompt(&spec).await.unwrap();
        assert_eq!(content, "from-inline");
    }

    #[tokio::test]
    async fn load_agent_prompt_uses_legacy_then_default_file() {
        let dir = tempdir().unwrap();
        let prompts_dir = dir.path().join("prompts");
        tokio::fs::create_dir_all(&prompts_dir).await.unwrap();
        tokio::fs::write(prompts_dir.join("agent-demo.md"), "from-default")
            .await
            .unwrap();

        let loader = PromptMaterialLoader {
            config_dir: dir.path().to_path_buf(),
            prompts_dir,
            project_context_file: None,
            developer_prompt_files: vec![],
        };

        let mut legacy_spec = test_agent_spec();
        legacy_spec.system_prompt_template = Some("from-legacy".to_string());
        let legacy_content = loader.load_agent_prompt(&legacy_spec).await.unwrap();
        assert_eq!(legacy_content, "from-legacy");

        let default_spec = test_agent_spec();
        let default_content = loader.load_agent_prompt(&default_spec).await.unwrap();
        assert_eq!(default_content, "from-default");
    }

    #[tokio::test]
    async fn load_agent_prompt_rejects_file_and_inline_together() {
        let dir = tempdir().unwrap();
        let loader = PromptMaterialLoader {
            config_dir: dir.path().to_path_buf(),
            prompts_dir: dir.path().join("prompts"),
            project_context_file: None,
            developer_prompt_files: vec![],
        };
        let mut spec = test_agent_spec();
        spec.prompt_file = Some("a.md".to_string());
        spec.prompt_inline = Some("inline".to_string());

        let err = loader.load_agent_prompt(&spec).await.unwrap_err();
        assert!(err.to_string().contains("both prompt_file and prompt_inline"));
    }

    #[tokio::test]
    async fn load_agent_prompt_errors_when_explicit_file_is_missing() {
        let dir = tempdir().unwrap();
        let loader = PromptMaterialLoader {
            config_dir: dir.path().to_path_buf(),
            prompts_dir: dir.path().join("prompts"),
            project_context_file: None,
            developer_prompt_files: vec![],
        };
        let mut spec = test_agent_spec();
        spec.prompt_file = Some("missing.md".to_string());

        let err = loader.load_agent_prompt(&spec).await.unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to read prompt_file for agent 'demo'"));
        assert!(message.contains("missing.md"));
    }

    #[tokio::test]
    async fn load_turn_material_respects_flags_and_idle_workflow() {
        let dir = tempdir().unwrap();
        let project_dir = dir.path().join("project");
        let prompts_dir = dir.path().join("prompts");
        tokio::fs::create_dir_all(&project_dir).await.unwrap();
        tokio::fs::create_dir_all(&prompts_dir).await.unwrap();

        tokio::fs::write(project_dir.join("AGENTS.md"), "agent-rules")
            .await
            .unwrap();
        tokio::fs::write(project_dir.join("PROJECT.md"), "project-context")
            .await
            .unwrap();

        let loader = PromptMaterialLoader {
            config_dir: dir.path().to_path_buf(),
            prompts_dir,
            project_context_file: None,
            developer_prompt_files: vec!["AGENTS.md".to_string()],
        };

        let disabled = loader
            .load_turn_material(Some(project_dir.as_path()), Some("idle"), None, HashMap::new(), false)
            .await
            .unwrap();
        assert!(disabled.developer_project_prompt.is_none());
        assert!(disabled.project_context.is_some());
        assert!(disabled.workflow_prompt.is_none());

        let enabled = loader
            .load_turn_material(
                Some(project_dir.as_path()),
                Some("idle"),
                Some("skill-a".to_string()),
                HashMap::new(),
                true,
            )
            .await
            .unwrap();
        assert!(enabled
            .developer_project_prompt
            .as_deref()
            .unwrap_or_default()
            .contains("### Source: AGENTS.md"));
        assert_eq!(enabled.active_skill.as_deref(), Some("skill-a"));
    }

    #[tokio::test]
    async fn load_turn_material_loads_non_idle_workflow_stage() {
        let dir = tempdir().unwrap();
        let prompts_dir = dir.path().join("prompts");
        tokio::fs::create_dir_all(&prompts_dir).await.unwrap();
        tokio::fs::write(
            prompts_dir.join("workflow-stages.md"),
            "## review\n\n```md\nReview {{active_agent}} changes.\n```",
        )
        .await
        .unwrap();

        let loader = PromptMaterialLoader {
            config_dir: dir.path().to_path_buf(),
            prompts_dir,
            project_context_file: None,
            developer_prompt_files: vec![],
        };
        let mut vars = HashMap::new();
        vars.insert("active_agent".to_string(), "Developer".to_string());

        let material = loader
            .load_turn_material(None, Some("review"), None, vars, false)
            .await
            .unwrap();

        assert_eq!(material.workflow_prompt.as_deref(), Some("Review Developer changes."));
    }
}
