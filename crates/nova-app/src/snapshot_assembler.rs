use chrono::Utc;
use nova_conversation::control::ControlState;
use nova_agent::prompt::TurnContext;
use nova_protocol::observability::*;

pub struct RuntimeSnapshotAssembler;

impl RuntimeSnapshotAssembler {
    pub fn assemble_session_runtime(session_id: &str, control: &ControlState) -> SessionRuntimeSnapshot {
        SessionRuntimeSnapshot {
            session_id: session_id.to_string(),
            active_agent: control.active_agent.clone(),
            model_override: SessionModelOverride {
                orchestration: control
                    .model_override
                    .orchestration
                    .as_ref()
                    .map(|m| nova_protocol::ModelRef {
                        provider: m.provider.clone(),
                        model: m.model.clone(),
                    }),
                execution: control
                    .model_override
                    .execution
                    .as_ref()
                    .map(|m| nova_protocol::ModelRef {
                        provider: m.provider.clone(),
                        model: m.model.clone(),
                    }),
                updated_at: control.model_override.updated_at,
            },
            last_turn: control.last_turn_snapshot.as_ref().map(|s| LastTurnSnapshot {
                turn_id: s.turn_id.clone(),
                prepared_at: s.prepared_at,
                prompt_preview: s
                    .prompt_preview
                    .as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok()),
                tools: s
                    .tools
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect(),
                skills: s
                    .skills
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect(),
                memory_hits: s
                    .memory_hits
                    .as_ref()
                    .map(|hits| {
                        hits.iter()
                            .filter_map(|v| serde_json::from_value(v.clone()).ok())
                            .collect()
                    })
                    .unwrap_or_default(),
                usage: s
                    .usage
                    .as_ref()
                    .and_then(|v| serde_json::from_value::<TurnUsage>(v.clone()).ok()),
            }),
            token_counters: SessionTokenCounters {
                input_tokens: control.token_counters.input_tokens,
                output_tokens: control.token_counters.output_tokens,
                cache_creation_input_tokens: control.token_counters.cache_creation_input_tokens,
                cache_read_input_tokens: control.token_counters.cache_read_input_tokens,
                updated_at: control.token_counters.updated_at,
            },
            updated_at: Utc::now().timestamp_millis(),
        }
    }

    pub fn turn_context_to_snapshot(turn_id: String, ctx: &TurnContext) -> LastTurnSnapshot {
        let prompt_preview = PromptPreviewSnapshot {
            system_prompt: ctx.system_prompt.clone(),
            tool_sections: Vec::new(), // TODO: Extract from builder if possible
            skill_sections: Vec::new(),
            conversation_summary: None,
            history_message_count: ctx.history.len(),
            active_skill: ctx.active_skill.as_ref().map(|s| s.skill_id.clone()),
            capability_policy_summary: Some(format!("{:?}", ctx.capability_policy)),
            max_tokens: Some(ctx.max_tokens as u32),
            iteration_budget: Some(ctx.iteration_budget as u32),
            rendered_prompt: None,
        };

        let tools = ctx
            .tool_definitions
            .iter()
            .map(|td| ToolAvailabilitySnapshot {
                name: td.name.clone(),
                source: "unknown".to_string(), // Need better source mapping
                description: Some(td.description.clone()),
                schema_summary: td.input_schema.clone(),
                enabled: true,
                unlocked_by: None,
            })
            .collect();

        let skills = if let Some(ref skill) = ctx.active_skill {
            vec![SkillBindingSnapshot {
                skill_id: skill.skill_id.clone(),
                name: skill.skill_id.clone(), // Need better name mapping
                status: "active".to_string(),
                description: None,
            }]
        } else {
            Vec::new()
        };

        LastTurnSnapshot {
            turn_id,
            prepared_at: Utc::now().timestamp_millis(),
            prompt_preview: Some(prompt_preview),
            tools,
            skills,
            memory_hits: Vec::new(),
            usage: None,
        }
    }
}
