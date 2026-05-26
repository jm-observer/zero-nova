use super::schema::{CommandExecution, ExternalToolDefinition};
use crate::tool::{RegisteredToolDefinition, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use serde_json::Value;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_OUTPUT_BYTES: usize = 100_000;

pub struct ExternalCommandTool {
    name: String,
    description: String,
    input_schema: Value,
    execution: CommandExecution,
}

impl ExternalCommandTool {
    pub fn from_definition(def: ExternalToolDefinition) -> Self {
        Self {
            name: def.name,
            description: def.description,
            input_schema: def.input_schema,
            execution: def.execution,
        }
    }

    fn build_args(&self, input: &Value) -> Vec<String> {
        let mut args: Vec<String> = self.execution.subcommands.clone();
        let obj = input.as_object();
        for mapping in &self.execution.param_mappings {
            let Some(val) = obj.and_then(|o| o.get(&mapping.name)) else {
                continue;
            };
            match val {
                Value::Bool(true) => {
                    args.push(mapping.arg.clone());
                }
                Value::Bool(false) => {}
                Value::String(s) => {
                    args.push(mapping.arg.clone());
                    args.push(s.clone());
                }
                Value::Number(n) => {
                    args.push(mapping.arg.clone());
                    args.push(n.to_string());
                }
                _ => {}
            }
        }
        args
    }
}

#[async_trait::async_trait]
impl Tool for ExternalCommandTool {
    fn definition(&self) -> RegisteredToolDefinition {
        RegisteredToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
            defer_loading: true,
        }
    }

    async fn execute(&self, input: Value, _context: Option<ToolContext>) -> Result<ToolOutput> {
        let args = self.build_args(&input);

        let mut cmd = Command::new(&self.execution.command);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = tokio::time::timeout(DEFAULT_TIMEOUT, cmd.output())
            .await
            .map_err(|_| anyhow::anyhow!("tool '{}' timed out after {}s", self.name, DEFAULT_TIMEOUT.as_secs()))?
            .map_err(|e| anyhow::anyhow!("failed to execute tool '{}': {}", self.name, e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let content = if output.status.success() {
            truncate_output(&stdout, MAX_OUTPUT_BYTES)
        } else {
            let mut msg = format!("Exit code: {}\n", output.status.code().unwrap_or(-1));
            if !stderr.is_empty() {
                msg.push_str(&truncate_output(&stderr, MAX_OUTPUT_BYTES));
            } else {
                msg.push_str(&truncate_output(&stdout, MAX_OUTPUT_BYTES));
            }
            msg
        };

        Ok(ToolOutput {
            content,
            is_error: !output.status.success(),
            child_session: None,
        })
    }
}

fn truncate_output(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        let truncated = &s[..max_bytes];
        format!("{}\n\n... (output truncated, {} bytes total)", truncated, s.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::external::schema::ParamMapping;
    use serde_json::json;

    fn make_tool(mappings: Vec<ParamMapping>) -> ExternalCommandTool {
        ExternalCommandTool {
            name: "test".to_string(),
            description: "test tool".to_string(),
            input_schema: json!({}),
            execution: CommandExecution {
                command: "echo".to_string(),
                subcommands: vec!["sub".to_string()],
                cwd: false,
                param_mappings: mappings,
            },
        }
    }

    #[test]
    fn test_build_args_string() {
        let tool = make_tool(vec![ParamMapping {
            name: "url".to_string(),
            param_type: "string".to_string(),
            arg: "--url".to_string(),
        }]);
        let args = tool.build_args(&json!({"url": "https://example.com"}));
        assert_eq!(args, vec!["sub", "--url", "https://example.com"]);
    }

    #[test]
    fn test_build_args_bool() {
        let tool = make_tool(vec![ParamMapping {
            name: "release".to_string(),
            param_type: "boolean".to_string(),
            arg: "--release".to_string(),
        }]);

        let args_true = tool.build_args(&json!({"release": true}));
        assert_eq!(args_true, vec!["sub", "--release"]);

        let args_false = tool.build_args(&json!({"release": false}));
        assert_eq!(args_false, vec!["sub"]);
    }

    #[test]
    fn test_build_args_mixed() {
        let tool = make_tool(vec![
            ParamMapping {
                name: "package".to_string(),
                param_type: "string".to_string(),
                arg: "-p".to_string(),
            },
            ParamMapping {
                name: "jobs".to_string(),
                param_type: "integer".to_string(),
                arg: "-j".to_string(),
            },
            ParamMapping {
                name: "release".to_string(),
                param_type: "boolean".to_string(),
                arg: "--release".to_string(),
            },
        ]);
        let args = tool.build_args(&json!({"package": "nova-agent", "jobs": 4, "release": true}));
        assert_eq!(args, vec!["sub", "-p", "nova-agent", "-j", "4", "--release"]);
    }

    #[test]
    fn test_build_args_missing_params() {
        let tool = make_tool(vec![
            ParamMapping {
                name: "url".to_string(),
                param_type: "string".to_string(),
                arg: "--url".to_string(),
            },
            ParamMapping {
                name: "branch".to_string(),
                param_type: "string".to_string(),
                arg: "--branch".to_string(),
            },
        ]);
        let args = tool.build_args(&json!({"url": "https://example.com"}));
        assert_eq!(args, vec!["sub", "--url", "https://example.com"]);
    }
}
