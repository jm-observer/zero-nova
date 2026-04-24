use crate::agent::{AgentConfig, AgentRuntime};
use crate::config::{AgentSpec, AppConfig};
use crate::message::{ContentBlock, Message, Role};
use crate::provider::openai_compat::OpenAiCompatClient;
use crate::tool::builtin::register_builtin_tools;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput, ToolRegistry};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Tool to spawn a subagent for specialized task execution.
pub struct AgentTool {
    config: AppConfig,
    agent_types: HashMap<String, AgentSpec>,
}

impl AgentTool {
    pub fn new(config: AppConfig) -> Self {
        let mut agent_types = HashMap::new();
        for agent in &config.gateway.agents {
            agent_types.insert(agent.id.clone(), agent.clone());
        }
        Self { config, agent_types }
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Agent".to_string(),
            description:
                "Spawn a specialized agent to perform a task. Supports multiple agent types and isolated execution."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "Specific task for the agent to perform" },
                    "description": { "type": "string", "description": "3-5 word summary of what the agent is doing" },
                    "subagent_type": { "type": "string", "description": "The type of agent to spawn (e.g., 'Explore', 'Plan', 'Coder')" },
                    "run_in_background": { "type": "boolean", "default": false, "description": "Whether to run the agent in the background" },
                    "isolation": { "type": "string", "enum": ["none", "worktree"], "default": "none", "description": "Isolation mode for the agent" },
                    "model": { "type": "string", "description": "Optional model override" }
                },
                "required": ["prompt", "description"]
            }),
            defer_loading: false,
        }
    }

    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        let prompt = input["prompt"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'prompt'"))?;
        let description = input["description"].as_str().unwrap_or("Executing task");
        let subagent_type = input["subagent_type"].as_str();
        let run_in_background = input["run_in_background"].as_bool().unwrap_or(false);
        let isolation = input["isolation"].as_str().unwrap_or("none");
        let model_override = input["model"].as_str();
        let mut warnings = Vec::new();

        if run_in_background {
            warnings
                .push("run_in_background is not implemented yet; the agent was executed synchronously.".to_string());
        }

        if isolation == "worktree" {
            warnings
                .push("worktree isolation is not implemented yet; the agent ran in the current workspace.".to_string());
        }

        log::info!(
            "[Agent] Starting {} agent: {}. Model: {:?}",
            subagent_type.unwrap_or("default"),
            description,
            model_override
        );

        // 1. Determine agent spec
        let spec = subagent_type.and_then(|t| self.agent_types.get(t));

        // 2. Setup Client
        let client = OpenAiCompatClient::new(self.config.llm.api_key.clone(), self.config.llm.base_url.clone());

        // 3. Setup Tool Registry for subagent
        let mut sub_registry = ToolRegistry::new();
        if let Some(ctx) = &context {
            if let (Some(task_store), Some(skill_registry)) = (ctx.task_store.as_ref(), ctx.skill_registry.as_ref()) {
                register_builtin_tools(
                    &mut sub_registry,
                    &self.config,
                    task_store.clone(),
                    skill_registry.clone(),
                    spec.and_then(|agent| agent.tool_whitelist.as_deref()),
                );
            }
        }

        // 4. Setup Runtime
        let mut model_config = if let Some(s) = spec {
            if let Some(m) = &s.model_config {
                // Map AgentModelConfig to provider::ModelConfig
                crate::provider::ModelConfig {
                    model: m.model.clone(),
                    max_tokens: m.max_tokens.unwrap_or(8192),
                    temperature: Some(m.temperature as f64),
                    top_p: Some(m.top_p as f64),
                    thinking_budget: None,
                    reasoning_effort: None,
                }
            } else {
                self.config.llm.model_config.clone()
            }
        } else {
            self.config.llm.model_config.clone()
        };

        if let Some(m) = model_override {
            model_config.model = m.to_string();
        }

        let agent_config = AgentConfig {
            max_iterations: self.config.gateway.max_iterations,
            model_config,
            tool_timeout: std::time::Duration::from_secs(self.config.gateway.subagent_timeout_secs),
            max_tokens: self.config.gateway.max_tokens,
        };

        let mut runtime = AgentRuntime::new(client, sub_registry, agent_config);

        // Inherit stores from parent context if available
        if let Some(ctx) = &context {
            runtime.task_store = ctx.task_store.clone();
            runtime.skill_registry = ctx.skill_registry.clone();
            runtime.read_files = ctx.read_files.clone();
        }

        // 5. Build System Prompt
        let mut system_prompt = if let Some(s) = spec {
            s.system_prompt_template.clone().unwrap_or_default()
        } else {
            "You are a helpful assistant.".to_string()
        };

        if system_prompt.is_empty() {
            system_prompt = "You are a helpful assistant.".to_string();
        }

        let history = vec![Message {
            role: Role::System,
            content: vec![ContentBlock::Text { text: system_prompt }],
        }];

        // 6. Execute
        let start_time = Instant::now();
        let (tx, mut rx) = mpsc::channel(100);
        let logs_collector = Arc::new(Mutex::new(Vec::new()));

        // Forwarding logs to parent
        let forwarding_handle = if let Some(ref ctx) = context {
            let parent_tx = ctx.event_tx.clone();
            let parent_tool_id = ctx.tool_use_id.clone();
            let logs = logs_collector.clone();

            Some(tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    match event {
                        crate::event::AgentEvent::TextDelta(text) => {
                            let _ = parent_tx
                                .send(crate::event::AgentEvent::LogDelta {
                                    id: parent_tool_id.clone(),
                                    name: "Agent".to_string(),
                                    log: text.clone(),
                                    stream: "stdout".to_string(),
                                })
                                .await;
                            logs.lock().await.push(text);
                        }
                        crate::event::AgentEvent::ToolStart { name, input, .. } => {
                            let log = format!("\n[Agent] 🚀 Executing {}: {}\n", name, input);
                            let _ = parent_tx
                                .send(crate::event::AgentEvent::LogDelta {
                                    id: parent_tool_id.clone(),
                                    name: "Agent".to_string(),
                                    log: log.clone(),
                                    stream: "stderr".to_string(),
                                })
                                .await;
                            logs.lock().await.push(log);
                        }
                        crate::event::AgentEvent::ToolEnd {
                            name,
                            output: _,
                            is_error,
                            ..
                        } => {
                            let status = if is_error { "❌ FAILED" } else { "✅ SUCCESS" };
                            let log = format!("[Agent] {} finished: {}\n", name, status);
                            let _ = parent_tx
                                .send(crate::event::AgentEvent::LogDelta {
                                    id: parent_tool_id.clone(),
                                    name: "Agent".to_string(),
                                    log: log.clone(),
                                    stream: "stderr".to_string(),
                                })
                                .await;
                            logs.lock().await.push(log);
                        }
                        _ => {}
                    }
                }
            }))
        } else {
            None
        };

        let result = runtime.run_turn(&history, prompt, tx, None).await?;

        if let Some(handle) = forwarding_handle {
            handle.await?;
        }

        let final_assistant_msg = result
            .messages
            .iter()
            .rev()
            .find(|m| m.role == Role::Assistant)
            .and_then(|m| {
                m.content.iter().find_map(|b| {
                    if let ContentBlock::Text { text } = b {
                        Some(text.clone())
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_default();

        let output_json = json!({
            "output": final_assistant_msg,
            "usage": {
                "total_tokens": result.usage.input_tokens + result.usage.output_tokens,
                "duration_ms": start_time.elapsed().as_millis(),
            },
            "warnings": warnings,
        });

        Ok(ToolOutput {
            content: serde_json::to_string_pretty(&output_json)?,
            is_error: false,
        })
    }
}
