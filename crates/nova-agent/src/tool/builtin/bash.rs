use crate::config::BashConfig;
use crate::event::AgentEvent;
use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use log::{info, warn};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::{timeout, Instant};
use which::which;

/// Shell 执行后端接口
trait ShellBackend: Send + Sync {
    /// 返回 shell 名称（用于日志/调试）
    fn name(&self) -> &str;

    /// 构建 Command，将 command_str 作为参数传入
    fn build_command(&self, command_str: &str) -> Command;
}

struct UnixSh;

impl ShellBackend for UnixSh {
    fn name(&self) -> &str {
        "sh"
    }

    fn build_command(&self, command_str: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", command_str]);
        cmd
    }
}

struct PowerShellBackend {
    executable: String, // "pwsh" 或 "powershell"
}

impl PowerShellBackend {
    fn detect() -> Option<Self> {
        // 优先检测 pwsh (PowerShell 7+, 跨平台, UTF-8)
        if which("pwsh").is_ok() {
            return Some(Self {
                executable: "pwsh".into(),
            });
        }
        // 降级到 Windows PowerShell 5.x
        if cfg!(windows) && which("powershell").is_ok() {
            return Some(Self {
                executable: "powershell".into(),
            });
        }
        None
    }
}

impl ShellBackend for PowerShellBackend {
    fn name(&self) -> &str {
        &self.executable
    }

    fn build_command(&self, command_str: &str) -> Command {
        let mut cmd = Command::new(&self.executable);
        if self.executable == "powershell" {
            // Windows PowerShell 5.x 需要额外设置编码
            let wrapped = format!(
                "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; {}",
                command_str
            );
            cmd.args(["-NoProfile", "-NonInteractive", "-Command", &wrapped]);
        } else {
            cmd.args([
                "-NoProfile",      // 跳过配置文件加载，加速启动
                "-NonInteractive", // 非交互模式
                "-Command",        // 执行命令字符串
                command_str,
            ]);
        }
        // 强制 UTF-8 输出
        cmd.env("PYTHONIOENCODING", "utf-8");
        cmd
    }
}

struct CmdBackend;

impl ShellBackend for CmdBackend {
    fn name(&self) -> &str {
        "cmd"
    }

    fn build_command(&self, command_str: &str) -> Command {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", command_str]);
        cmd
    }
}

fn select_shell(config: &BashConfig) -> Box<dyn ShellBackend> {
    // 1. 配置覆盖
    if let Some(shell) = &config.shell {
        match shell.to_lowercase().as_str() {
            "sh" | "bash" => return Box::new(UnixSh),
            "pwsh" | "powershell" => {
                if let Some(ps) = PowerShellBackend::detect() {
                    return Box::new(ps);
                }
            }
            "cmd" => return Box::new(CmdBackend),
            _ => {} // 忽略无效值，走自动检测
        }
    }

    // 2. 平台自动检测
    if cfg!(windows) {
        // Windows: pwsh > powershell > cmd
        if let Some(ps) = PowerShellBackend::detect() {
            return Box::new(ps);
        }
        Box::new(CmdBackend)
    } else {
        // Unix: sh
        Box::new(UnixSh)
    }
}

/// Tool for executing shell commands.
pub struct BashTool {
    shell: Arc<dyn ShellBackend>,
    /// Optional workspace directory to execute commands in.
    workspace: Option<PathBuf>,
}

impl BashTool {
    pub fn new(config: &BashConfig) -> Self {
        let shell: Arc<dyn ShellBackend> = select_shell(config).into();
        info!("BashTool initialized using shell: {}", shell.name());
        Self { shell, workspace: None }
    }

    /// Creates a new `BashTool` with a specific workspace directory.
    pub fn with_workspace(config: &BashConfig, workspace: PathBuf) -> Self {
        let shell: Arc<dyn ShellBackend> = select_shell(config).into();
        Self {
            shell,
            workspace: Some(workspace),
        }
    }
}

#[async_trait]
/// Implementation of the `Tool` trait for BashTool.
impl Tool for BashTool {
    /// Returns the tool definition for BashTool.
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Bash".to_string(),
            description: format!(
                "Execute a shell command (using {}). Returns stdout, stderr and exit code. On Windows PowerShell, prefer PowerShell syntax such as `Get-ChildItem -Force` instead of Unix flags like `-la`.",
                self.shell.name()
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The shell command to execute" },
                    "description": { "type": "string", "description": "Clear, concise description of what this command does" },
                    "run_in_background": { "type": "boolean", "description": "Run in background, return immediately" },
                    "timeout_ms": { "type": "integer", "description": "Timeout in milliseconds (default 3600000)" },
                    "dangerouslyDisableSandbox": { "type": "boolean", "description": "Override sandbox mode" }
                },
                "required": ["command"]
            }),
            defer_loading: false,
        }
    }

    /// Executes the bash command as defined in the input JSON.
    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        let command_str = input["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' field"))?;
        let timeout_ms = input["timeout_ms"].as_u64().unwrap_or(3600000);
        let run_in_background = input["run_in_background"].as_bool().unwrap_or(false);

        if run_in_background {
            let shell = self.shell.clone();
            let command_str_owned = command_str.to_string();
            let workspace = self.workspace.clone();
            let ctx = context.clone();

            tokio::spawn(async move {
                let mut cmd = shell.build_command(&command_str_owned);
                if let Some(ws) = workspace {
                    cmd.current_dir(ws);
                }
                let _ = cmd.status().await;
                if let Some(c) = ctx {
                    let _ = c
                        .event_tx
                        .send(AgentEvent::BackgroundTaskComplete {
                            id: c.tool_use_id,
                            name: "Bash".to_string(),
                        })
                        .await;
                }
            });

            return Ok(ToolOutput {
                content: "Command started in background. You will be notified when it completes.".to_string(),
                is_error: false,
            });
        }

        let mut cmd = self.shell.build_command(command_str);
        if let Some(ws) = &self.workspace {
            cmd.current_dir(ws);
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn command: {}", e))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture stderr"))?;

        let mut stdout_buf = String::new();
        let mut stderr_buf = String::new();

        const LOG_FLUSH_INTERVAL_MS: u128 = 200;

        let read_fut = async {
            let mut stdout_reader = BufReader::new(stdout).lines();
            let mut stderr_reader = BufReader::new(stderr).lines();

            let mut stdout_done = false;
            let mut stderr_done = false;

            let mut pending_stdout = String::new();
            let mut pending_stderr = String::new();
            let mut last_flush = Instant::now();

            while !stdout_done || !stderr_done {
                tokio::select! {
                    line = stdout_reader.next_line(), if !stdout_done => {
                        match line {
                            Ok(Some(line)) => {
                                pending_stdout.push_str(&line);
                                pending_stdout.push('\n');
                                stdout_buf.push_str(&line);
                                stdout_buf.push('\n');

                                if last_flush.elapsed().as_millis() >= LOG_FLUSH_INTERVAL_MS {
                                    if let Some(ctx) = &context {
                                        let _ = ctx.event_tx.send(AgentEvent::LogDelta {
                                            id: ctx.tool_use_id.clone(),
                                            name: "Bash".to_string(),
                                            log: std::mem::take(&mut pending_stdout),
                                            stream: "stdout".to_string(),
                                        }).await;
                                    }
                                    last_flush = Instant::now();
                                }
                            }
                            Ok(None) => stdout_done = true,
                            Err(e) => {
                                stderr_buf.push_str(&format!("Error reading stdout: {}\n", e));
                                stdout_done = true;
                            }
                        }
                    }
                    line = stderr_reader.next_line(), if !stderr_done => {
                        match line {
                            Ok(Some(line)) => {
                                pending_stderr.push_str(&line);
                                pending_stderr.push('\n');
                                stderr_buf.push_str(&line);
                                stderr_buf.push('\n');

                                if last_flush.elapsed().as_millis() >= LOG_FLUSH_INTERVAL_MS {
                                    if let Some(ctx) = &context {
                                        let _ = ctx.event_tx.send(AgentEvent::LogDelta {
                                            id: ctx.tool_use_id.clone(),
                                            name: "Bash".to_string(),
                                            log: std::mem::take(&mut pending_stderr),
                                            stream: "stderr".to_string(),
                                        }).await;
                                    }
                                    last_flush = Instant::now();
                                }
                            }
                            Ok(None) => stderr_done = true,
                            Err(e) => {
                                stderr_buf.push_str(&format!("Error reading stderr: {}\n", e));
                                stderr_done = true;
                            }
                        }
                    }
                }
            }

            // Final flush
            if !pending_stdout.is_empty() {
                if let Some(ctx) = &context {
                    let _ = ctx
                        .event_tx
                        .send(AgentEvent::LogDelta {
                            id: ctx.tool_use_id.clone(),
                            name: "Bash".to_string(),
                            log: pending_stdout,
                            stream: "stdout".to_string(),
                        })
                        .await;
                }
            }
            if !pending_stderr.is_empty() {
                if let Some(ctx) = &context {
                    let _ = ctx
                        .event_tx
                        .send(AgentEvent::LogDelta {
                            id: ctx.tool_use_id.clone(),
                            name: "Bash".to_string(),
                            log: pending_stderr,
                            stream: "stderr".to_string(),
                        })
                        .await;
                }
            }

            child.wait().await
        };

        match timeout(Duration::from_millis(timeout_ms), read_fut).await {
            Ok(Ok(status)) => {
                let exit_code = status.code().unwrap_or(-1);
                let content = format!(
                    "exit_code: {}\nstdout:\n{}\nstderr:\n{}",
                    exit_code,
                    truncate(&stdout_buf, 100_000),
                    truncate(&stderr_buf, 10_000)
                );
                Ok(ToolOutput {
                    content,
                    is_error: !status.success(),
                })
            }
            Ok(Err(e)) => Ok(ToolOutput {
                content: format!("Failed to execute command: {}", e),
                is_error: true,
            }),
            Err(_) => {
                let _ = child.kill().await;
                let content = format!(
                    "Command timed out after {}ms\nstdout so far:\n{}\nstderr so far:\n{}",
                    timeout_ms,
                    truncate(&stdout_buf, 100_000),
                    truncate(&stderr_buf, 10_000)
                );
                warn!("{content}");
                Ok(ToolOutput {
                    content,
                    is_error: true,
                })
            }
        }
    }
}

/// Truncates a string to `max_len` characters safely at a char boundary.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}... [truncated]", &s[..end])
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_selection_default() {
        let config = BashConfig::default();
        let shell = select_shell(&config);
        if cfg!(windows) {
            // Check if one of the expected Windows shells is selected
            let name = shell.name();
            assert!(
                name == "pwsh" || name == "powershell" || name == "cmd",
                "Unexpected shell name on Windows: {}",
                name
            );
        } else {
            assert_eq!(shell.name(), "sh");
        }
    }

    #[test]
    fn test_truncate_safe() {
        let s = "你好世界"; // 4 chars, 12 bytes
        assert_eq!(truncate(s, 12), "你好世界");
        assert_eq!(truncate(s, 11), "你好世... [truncated]"); // Truncated at 9 bytes (3 chars)
        assert_eq!(truncate(s, 9), "你好世... [truncated]");
        assert_eq!(truncate(s, 6), "你好... [truncated]");
        assert_eq!(truncate(s, 3), "你... [truncated]");
        assert_eq!(truncate(s, 0), "... [truncated]");
    }

    #[tokio::test]
    async fn test_shell_execution() {
        let config = BashConfig::default();
        let tool = BashTool::new(&config);
        let input = json!({
            "command": "echo hello",
            "timeout_ms": 5000
        });
        let result = tool.execute(input, None).await.unwrap();
        assert!(result.content.contains("hello"));
        assert!(!result.is_error);
    }
}
