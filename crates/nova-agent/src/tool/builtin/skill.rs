use crate::event::AgentEvent;
use crate::skill::SkillRegistry;
use crate::tool::{RegisteredToolDefinition, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct SkillTool {
    pub registry: Arc<SkillRegistry>,
}

impl SkillTool {
    pub fn new(registry: Arc<SkillRegistry>) -> Self {
        Self { registry }
    }

    pub fn input_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "skill": { "type": "string", "description": "Skill identifier to activate. Must be one of the identifiers listed in the '## Available Skills' section of the system prompt (the bold/backticked name, e.g. 'orchestrator')." },
                "args": { "type": "string", "description": "Optional arguments passed to the skill" }
            },
            "required": ["skill"]
        })
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn definition(&self) -> RegisteredToolDefinition {
        RegisteredToolDefinition {
            name: "Skill".to_string(),
            description: "Activate a specialized skill and inject its full instructions into the current session. The 'skill' parameter must be a skill identifier from the '## Available Skills' section of the system prompt.".to_string(),
            input_schema: Self::input_schema(),
            defer_loading: false,
        }
    }

    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        let skill_name = input["skill"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'skill'"))?;
        let args = input["args"].as_str().filter(|value| !value.trim().is_empty());

        if let Some(skill) = self.registry.find_by_name(skill_name) {
            if let Some(ctx) = &context {
                let _ = ctx
                    .event_tx
                    .send(AgentEvent::SkillLoaded {
                        skill_name: skill.id.clone(),
                    })
                    .await;
            }

            Ok(ToolOutput {
                content: format_skill_output(skill.display_name.as_str(), skill.instructions.as_str(), args),
                is_error: false,
                child_session: None,
                images: Vec::new(),
            })
        } else {
            let available: Vec<String> = self
                .registry
                .all_candidates()
                .into_iter()
                .map(|skill| skill.id.clone())
                .collect();
            Ok(ToolOutput {
                content: format!(
                    "Skill '{}' not found. Available skills: {}",
                    skill_name,
                    available.join(", ")
                ),
                is_error: true,
                child_session: None,
                images: Vec::new(),
            })
        }
    }
}

fn format_skill_output(skill_name: &str, instructions: &str, args: Option<&str>) -> String {
    match args {
        Some(args) => format!(
            "Skill '{}' loaded.\nArguments: {}\n\nInstructions:\n\n{}",
            skill_name, args, instructions
        ),
        None => format!("Skill '{}' loaded. Instructions:\n\n{}", skill_name, instructions),
    }
}
