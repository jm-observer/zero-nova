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

    fn build_args(&self, input: &Value) -> Result<Vec<String>> {
        let mut args: Vec<String> = self.execution.subcommands.clone();
        let obj = input.as_object();
        for mapping in &self.execution.param_mappings {
            let val = obj.and_then(|o| o.get(&mapping.name));
            let val = match val {
                Some(v) if !v.is_null() => v,
                _ => {
                    if mapping.required {
                        return Err(anyhow::anyhow!(
                            "tool '{}': missing required parameter '{}'",
                            self.name,
                            mapping.name
                        ));
                    }
                    continue;
                }
            };
            match (&mapping.arg, val) {
                // 命名 flag
                (Some(flag), Value::Bool(true)) => {
                    args.push(flag.clone());
                }
                (Some(_), Value::Bool(false)) => {}
                (Some(flag), Value::String(s)) => {
                    args.push(flag.clone());
                    args.push(s.clone());
                }
                (Some(flag), Value::Number(n)) => {
                    args.push(flag.clone());
                    args.push(n.to_string());
                }
                // 位置参数：按声明顺序透传 value，不带 flag 前缀
                (None, Value::String(s)) => {
                    args.push(s.clone());
                }
                (None, Value::Number(n)) => {
                    args.push(n.to_string());
                }
                (None, Value::Bool(b)) => {
                    args.push(b.to_string());
                }
                _ => {}
            }
        }
        Ok(args)
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
        let args = match self.build_args(&input) {
            Ok(args) => args,
            Err(err) => {
                return Ok(ToolOutput {
                    content: err.to_string(),
                    is_error: true,
                    child_session: None,
                    images: Vec::new(),
                });
            }
        };

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
            images: Vec::new(),
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

    fn flag(name: &str, ty: &str, arg: &str) -> ParamMapping {
        ParamMapping {
            name: name.to_string(),
            param_type: ty.to_string(),
            arg: Some(arg.to_string()),
            required: false,
        }
    }

    fn positional(name: &str, ty: &str, required: bool) -> ParamMapping {
        ParamMapping {
            name: name.to_string(),
            param_type: ty.to_string(),
            arg: None,
            required,
        }
    }

    #[test]
    fn test_build_args_string() {
        let tool = make_tool(vec![flag("url", "string", "--url")]);
        let args = tool.build_args(&json!({"url": "https://example.com"})).unwrap();
        assert_eq!(args, vec!["sub", "--url", "https://example.com"]);
    }

    #[test]
    fn test_build_args_bool() {
        let tool = make_tool(vec![flag("release", "boolean", "--release")]);

        let args_true = tool.build_args(&json!({"release": true})).unwrap();
        assert_eq!(args_true, vec!["sub", "--release"]);

        let args_false = tool.build_args(&json!({"release": false})).unwrap();
        assert_eq!(args_false, vec!["sub"]);
    }

    #[test]
    fn test_build_args_mixed() {
        let tool = make_tool(vec![
            flag("package", "string", "-p"),
            flag("jobs", "integer", "-j"),
            flag("release", "boolean", "--release"),
        ]);
        let args = tool
            .build_args(&json!({"package": "nova-agent", "jobs": 4, "release": true}))
            .unwrap();
        assert_eq!(args, vec!["sub", "-p", "nova-agent", "-j", "4", "--release"]);
    }

    #[test]
    fn test_build_args_missing_optional_params() {
        let tool = make_tool(vec![
            flag("url", "string", "--url"),
            flag("branch", "string", "--branch"),
        ]);
        let args = tool.build_args(&json!({"url": "https://example.com"})).unwrap();
        assert_eq!(args, vec!["sub", "--url", "https://example.com"]);
    }

    // ---- 位置参数（regression guard：alarm-cli cancel <ID> 这类调用之前被静默吞掉）----

    #[test]
    fn test_build_args_positional_only() {
        // 模拟 alarm-cli cancel <ID>：id 是必填位置参数
        let tool = make_tool(vec![positional("id", "string", true)]);
        let args = tool
            .build_args(&json!({"id": "54f01039-bdf6-44db-9352-46a6862a7fa3"}))
            .unwrap();
        assert_eq!(
            args,
            vec!["sub", "54f01039-bdf6-44db-9352-46a6862a7fa3"],
            "位置参数必须按 value 直接透传，不能被丢弃"
        );
    }

    #[test]
    fn test_build_args_positional_with_flags() {
        // 位置参数 + 命名 flag 混合：按声明顺序生成
        let tool = make_tool(vec![
            positional("id", "string", true),
            flag("workspace", "string", "--workspace"),
        ]);
        let args = tool
            .build_args(&json!({"id": "abc-123", "workspace": "/tmp/ws"}))
            .unwrap();
        assert_eq!(args, vec!["sub", "abc-123", "--workspace", "/tmp/ws"]);
    }

    #[test]
    fn test_build_args_positional_numeric() {
        let tool = make_tool(vec![positional("port", "integer", true)]);
        let args = tool.build_args(&json!({"port": 8080})).unwrap();
        assert_eq!(args, vec!["sub", "8080"]);
    }

    #[test]
    fn test_build_args_missing_required_positional_errors() {
        // 旧实现会静默生成 `alarm-cli cancel`（无 id），导致下游 clap 报模糊错误。
        // 新实现必须在 build 阶段就拒绝。
        let tool = make_tool(vec![positional("id", "string", true)]);
        let err = tool.build_args(&json!({})).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing required parameter"), "got: {msg}");
        assert!(msg.contains("'id'"), "got: {msg}");
    }

    #[test]
    fn test_build_args_missing_required_flag_errors() {
        let tool = make_tool(vec![flag_required("url", "string", "--url")]);
        let err = tool.build_args(&json!({})).unwrap_err();
        assert!(err.to_string().contains("'url'"));
    }

    fn flag_required(name: &str, ty: &str, arg: &str) -> ParamMapping {
        ParamMapping {
            name: name.to_string(),
            param_type: ty.to_string(),
            arg: Some(arg.to_string()),
            required: true,
        }
    }
}
