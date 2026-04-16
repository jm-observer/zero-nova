use crate::tool::{Tool, ToolDefinition, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use log::info;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
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

fn select_shell(config: &crate::config::BashConfig) -> Box<dyn ShellBackend> {
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
    shell: Box<dyn ShellBackend>,
}

impl BashTool {
    pub fn new(config: &crate::config::BashConfig) -> Self {
        let shell = select_shell(config);
        info!("BashTool initialized using shell: {}", shell.name());
        Self { shell }
    }
}

#[async_trait]
/// Implementation of the `Tool` trait for BashTool.
impl Tool for BashTool {
    /// Returns the tool definition for BashTool.
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "bash".to_string(),
            description: format!(
                "Execute a shell command (using {}). Returns stdout, stderr and exit code.",
                self.shell.name()
            ),
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

    /// Executes the bash command as defined in the input JSON.
    async fn execute(&self, input: Value) -> Result<ToolOutput> {
        let command_str = input["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' field"))?;
        let timeout_ms = input["timeout_ms"].as_u64().unwrap_or(30000);

        let mut cmd = self.shell.build_command(command_str);
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
        let config = crate::config::BashConfig::default();
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
        let config = crate::config::BashConfig::default();
        let tool = BashTool::new(&config);
        let input = json!({
            "command": "echo hello",
            "timeout_ms": 5000
        });
        let result = tool.execute(input).await.unwrap();
        assert!(result.content.contains("hello"));
        assert!(!result.is_error);
    }
}

