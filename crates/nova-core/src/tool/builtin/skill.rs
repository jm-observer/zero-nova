use crate::skill::SkillRegistry;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};
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
}

#[async_trait]
impl Tool for SkillTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Skill".to_string(),
            description: "Loads and injects specialized skills into the current session.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill": { "type": "string", "description": "The name of the skill to load" },
                    "args": { "type": "string", "description": "Optional arguments for the skill" }
                },
                "required": ["skill"]
            }),
            defer_loading: false,
        }
    }

    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        let skill_name = input["skill"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'skill'"))?;

        // Find skill in registry
        let skill = self.registry.skills.iter().find(|s| s.name == skill_name);

        if let Some(s) = skill {
            if let Some(ctx) = &context {
                let _ = ctx
                    .event_tx
                    .send(crate::event::AgentEvent::SkillLoaded {
                        skill_name: skill_name.to_string(),
                    })
                    .await;
            }

            Ok(ToolOutput {
                content: format!("Skill '{}' loaded. Instructions:\n\n{}", s.name, s.body),
                is_error: false,
            })
        } else {
            let available: Vec<String> = self.registry.skills.iter().map(|s| s.name.clone()).collect();
            Ok(ToolOutput {
                content: format!(
                    "Skill '{}' not found. Available skills: {}",
                    skill_name,
                    available.join(", ")
                ),
                is_error: true,
            })
        }
    }
}
