use crate::agent::{AgentConfig, AgentRuntime};
use crate::config::AppConfig;
use crate::message::{ContentBlock, Message, Role};
use crate::provider::openai_compat::OpenAiCompatClient;
use crate::tool::{Tool, ToolDefinition, ToolOutput, ToolRegistry};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::Instant;

/// Tool to spawn a subagent for isolated task execution.
pub struct SpawnSubagentTool {
    config: AppConfig,
}

impl SpawnSubagentTool {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for SpawnSubagentTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "spawn_subagent".to_string(),
            description: "Spawn a separate agent to perform a task in isolation. Returns the execution summary and resource usage (token counts and duration).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "Specific task for the subagent to perform"
                    },
                    "system_prompt_patch": {
                        "type": "string",
                        "description": "Optional instructions to append to the system prompt (e.g. Skill instructions)"
                    },
                    "workspace": {
                        "type": "string",
                        "description": "Absolute path to an isolated workspace directory"
                    }
                },
                "required": ["task"]
            }),
        }
    }

    async fn execute(&self, input: Value, _context: Option<crate::tool::ToolContext>) -> Result<ToolOutput> {
        let task = input["task"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'task'"))?;
        let system_prompt_patch = input["system_prompt_patch"].as_str().unwrap_or("");
        let workspace_str = input["workspace"].as_str();

        // 1. Setup workspace
        let workspace = if let Some(ws) = workspace_str {
            let path = std::env::current_dir()?.join(ws);
            if !path.exists() {
                tokio::fs::create_dir_all(&path).await?;
            }
            Some(std::fs::canonicalize(path)?)
        } else {
            None
        };

        // 2. Setup Client
        let client = OpenAiCompatClient::new(self.config.llm.api_key.clone(), self.config.llm.base_url.clone());

        // 3. Setup Tool Registry for subagent (Isolation)
        let mut sub_registry = ToolRegistry::new();

        sub_registry.register(Box::new(crate::tool::builtin::bash::BashTool::with_workspace(
            &self.config.tool.bash,
            workspace
                .clone()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
        )));
        sub_registry.register(Box::new(crate::tool::builtin::file_ops::ReadFileTool::new(
            workspace.clone(),
        )));
        sub_registry.register(Box::new(crate::tool::builtin::file_ops::WriteFileTool::new(
            workspace.clone(),
        )));

        // 4. Setup Runtime
        let mut model_config = self.config.llm.model_config.clone();
        if let Some(m) = input["model"].as_str() {
            model_config.model = m.to_string();
        }

        let agent_config = AgentConfig {
            max_iterations: input["max_iterations"].as_u64().unwrap_or(10) as usize,
            model_config,
            tool_timeout: std::time::Duration::from_secs(300),
        };

        let runtime = AgentRuntime::new(client, sub_registry, agent_config);

        // 5. Build System Prompt
        let base_system = r#"You are an autonomous subagent. 
Your goal is to complete the assigned task by directly AGENTICALLY using the provided tools.
- If the task involves creating or modifying code, use `write_file`.
- If the task involves testing or running code, use `bash`.
- ALWAYS prioritize taking action over describing what you would do.
- You are operating within an isolated workspace. All file paths should be relative to your current directory.
- **Cross-Platform Compatibility**: You might be running on Windows or Linux. 
  - Prefer using common commands that work in both (e.g., `ls` is aliased in PowerShell, but `dir` fails on Linux).
  - Use `python` as the command unless specifically told otherwise.
  - If a command fails, interpret the error and try the alternative platform's equivalent.
"#;

        let full_system = if system_prompt_patch.is_empty() {
            base_system.to_string()
        } else {
            format!("{}\n\nAdditional Instructions:\n{}", base_system, system_prompt_patch)
        };

        let history = vec![Message {
            role: Role::System,
            content: vec![ContentBlock::Text { text: full_system }],
        }];

        // 6. Execute (with ID Tunneling Forwarding)
        let start_time = Instant::now();
        let (tx, mut rx) = mpsc::channel(100);
        let logs_collector = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let logs_collector_cloned = logs_collector.clone();

        let forwarding_handle = if let Some(ctx) = _context {
            let parent_tx = ctx.event_tx.clone();
            let parent_tool_id = ctx.tool_use_id.clone();

            // Initial log: Confirming Skill loading to the user
            if !system_prompt_patch.is_empty() {
                let log = format!(
                    "\n[System] 🧠 技能已装载 | 指令长度: {} 字符\n",
                    system_prompt_patch.len()
                );
                let _ = parent_tx.try_send(crate::event::AgentEvent::LogDelta {
                    id: parent_tool_id.clone(),
                    name: "subagent".to_string(),
                    log: log.clone(),
                    stream: "stderr".to_string(),
                });
                let logs = logs_collector_cloned.clone();
                tokio::spawn(async move {
                    logs.lock().await.push(log);
                });
            }

            let logs_collector_for_loop = logs_collector_cloned.clone();
            Some(tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    match event {
                        crate::event::AgentEvent::TextDelta(text) => {
                            let _ = parent_tx
                                .send(crate::event::AgentEvent::LogDelta {
                                    id: parent_tool_id.clone(),
                                    name: "subagent".to_string(),
                                    log: text.clone(),
                                    stream: "stdout".to_string(),
                                })
                                .await;
                            logs_collector_for_loop.lock().await.push(text);
                        }
                        crate::event::AgentEvent::ToolStart { name, input, .. } => {
                            let log = format!("\n[Subagent] 🚀 正在执行: {} (参数: {})\n", name, input);
                            let _ = parent_tx
                                .send(crate::event::AgentEvent::LogDelta {
                                    id: parent_tool_id.clone(),
                                    name: "subagent".to_string(),
                                    log: log.clone(),
                                    stream: "stderr".to_string(),
                                })
                                .await;
                            logs_collector_for_loop.lock().await.push(log);
                        }
                        crate::event::AgentEvent::ToolEnd {
                            name, output, is_error, ..
                        } => {
                            let status = if is_error { "❌ 失败" } else { "✅ 成功" };
                            let log = format!(
                                "[Subagent] {} 执行完成: {} | 输出: {}\n",
                                name,
                                status,
                                truncate_output(&output, 60)
                            );
                            let _ = parent_tx
                                .send(crate::event::AgentEvent::LogDelta {
                                    id: parent_tool_id.clone(),
                                    name: "subagent".to_string(),
                                    log: log.clone(),
                                    stream: "stderr".to_string(),
                                })
                                .await;
                            logs_collector_for_loop.lock().await.push(log);
                        }
                        _ => {}
                    }
                }
            }))
        } else {
            None
        };

        let result = runtime.run_turn(&history, task, tx, None).await?;

        if let Some(handle) = forwarding_handle {
            // Wait a bit for remaining logs to be processed if any
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            handle.abort();
        }

        let duration = start_time.elapsed();
        let logs = logs_collector.lock().await.clone();

        // 7. Scan workspace
        let mut files_created = Vec::new();
        if let Some(ws) = &workspace {
            let mut entries = tokio::fs::read_dir(ws).await?;
            while let Some(entry) = entries.next_entry().await? {
                if entry.file_type().await?.is_file() {
                    files_created.push(entry.file_name().to_string_lossy().to_string());
                }
            }
        }

        // 8. Result
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
            "output_summary": final_assistant_msg,
            "logs": logs,
            "usage": {
                "total_tokens": result.usage.input_tokens + result.usage.output_tokens,
                "input_tokens": result.usage.input_tokens,
                "output_tokens": result.usage.output_tokens,
                "duration_ms": duration.as_millis(),
                "iterations": result.messages.len() / 2,
            },
            "workspace_files": files_created,
            "workspace_path": workspace.map(|w| w.to_string_lossy().to_string())
        });

        Ok(ToolOutput {
            content: serde_json::to_string_pretty(&output_json)?,
            is_error: false,
        })
    }
}

fn truncate_output(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s.chars().take(max_len).collect::<String>())
    } else {
        s.to_string()
    }
}
