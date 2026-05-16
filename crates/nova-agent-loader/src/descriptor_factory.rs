use crate::prompt_loader::PromptMaterialLoader;
use anyhow::{Context, Result};
use nova_agent::agent_catalog::AgentDescriptor;
use nova_agent::prompt::{SystemPromptBuilder, TurnPromptMaterial};
use nova_agent::skill::SkillRegistry;
use nova_agent_config::{AgentSpec, ResolvedAgentBinding};
use std::collections::HashMap;

pub struct AgentMaterialInputs {
    pub environment_snapshot: Option<nova_agent::prompt::EnvironmentSnapshot>,
    pub agent_catalog: Option<String>,
    pub initial_template_vars: HashMap<String, String>,
}

pub struct AgentDescriptorFactory {
    prompt_loader: PromptMaterialLoader,
}

impl AgentDescriptorFactory {
    pub fn new(prompt_loader: PromptMaterialLoader) -> Self {
        Self { prompt_loader }
    }

    pub async fn build_descriptor(
        &self,
        spec: &AgentSpec,
        binding: &ResolvedAgentBinding,
        material_inputs: AgentMaterialInputs,
        skills: &SkillRegistry,
    ) -> Result<AgentDescriptor> {
        let prompt_material = self
            .prompt_loader
            .load_agent_material(
                spec,
                material_inputs.environment_snapshot,
                material_inputs.agent_catalog,
                material_inputs.initial_template_vars.clone(),
            )
            .await?;

        let system_prompt_template =
            SystemPromptBuilder::from_material(&prompt_material, &TurnPromptMaterial::default(), skills).build();

        Ok(AgentDescriptor {
            id: spec.id.clone(),
            display_name: spec.display_name.clone(),
            description: spec.description.clone(),
            aliases: spec.aliases.clone(),
            system_prompt_template,
            system_prompt_base: prompt_material.agent_prompt,
            initial_template_vars: material_inputs.initial_template_vars,
            model_config: Some(spec.model_config.clone().into()),
            provider_id: binding.provider_id.clone(),
            llm_id: binding
                .llm_id
                .clone()
                .context("configured agent binding must always resolve to a concrete llm")?,
            enable_project_developer_prompt: spec.enable_project_developer_prompt,
        })
    }
}
