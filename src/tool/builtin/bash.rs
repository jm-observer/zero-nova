use crate::tool::{Tool, ToolDefinition, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "bash".to_string(),
            description: "Execute a shell command and return its stdout and stderr. Use this for system operations, running scripts, or any command-line tasks.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The shell command to execute" },
                    "timeout_ms": { "type": "integer", "description": "Timeout in milliseconds (default 30000)" }
                },
                "required": ["command"]
            }),
        }
    }

    async fn execute(&self, input: Value) -> Result<ToolOutput> {
        let command_str = input["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' field"))?;
        let timeout_ms = input["timeout_ms"].as_u64().unwrap_or(30000);

        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", command_str]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", command_str]);
            c
        };

        // Capture output
        let fut = cmd.output();

        match timeout(Duration::from_millis(timeout_ms), fut).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let exit_code = output.status.code().unwrap_or(-1);

                let content = format!(
                    "exit_code: {}\nstdout:\n{}\nstderr:\n{}",
                    exit_code,
                    truncate(&stdout, 100_000),
                    truncate(&stderr, 10_000)
                );

                Ok(ToolOutput {
                    content,
                    is_error: !output.status.success(),
                })
            }
            Ok(Err(e)) => Ok(ToolOutput {
                content: format!("Failed to execute command: {}", e),
                is_error: true,
            }),
            Err(_) => Ok(ToolOutput {
                content: format!("Command timed out after {}ms", timeout_ms),
                is_error: true,
            }),
        }
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}... [truncated]", &s[..max_len])
    } else {
        s.to_string()
    }
}
