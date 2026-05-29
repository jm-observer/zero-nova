// Platform-specific modules - declared here so they are submodules of bash
#[cfg(unix)]
mod bash_linux;
#[cfg(target_os = "windows")]
mod bash_windows;

use crate::config::BashConfig;
use crate::event::AgentEvent;
use crate::tool::{RegisteredToolDefinition, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use log::{info, warn};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::{timeout, Instant};

// Platform-specific re-exports for external use
#[cfg(unix)]
pub use bash_linux::{select_shell, UnixBash, UnixSh};
#[cfg(target_os = "windows")]
pub use bash_windows::{select_shell, CmdBackend, PowerShellBackend};

///封闭的后端集合，覆盖所有支持的 shell 类型
#[derive(Clone)]
pub enum ShellBackend {
    #[cfg(target_os = "windows")]
    PowerShell(bash_windows::PowerShellBackend),
    #[cfg(target_os = "windows")]
    Cmd(CmdBackend),
    #[cfg(unix)]
    UnixSh(UnixSh),
    #[cfg(unix)]
    UnixBash(UnixBash),
}

impl ShellBackend {
    fn name(&self) -> &str {
        match self {
            #[cfg(target_os = "windows")]
            Self::PowerShell(b) => b.name(),
            #[cfg(target_os = "windows")]
            Self::Cmd(b) => b.name(),
            #[cfg(unix)]
            Self::UnixSh(b) => b.name(),
            #[cfg(unix)]
            Self::UnixBash(b) => b.name(),
        }
    }

    fn build_command(&self, command_str: &str) -> Command {
        match self {
            #[cfg(target_os = "windows")]
            Self::PowerShell(b) => b.build_command(command_str),
            #[cfg(target_os = "windows")]
            Self::Cmd(b) => b.build_command(command_str),
            #[cfg(unix)]
            Self::UnixSh(b) => b.build_command(command_str),
            #[cfg(unix)]
            Self::UnixBash(b) => b.build_command(command_str),
        }
    }
}

fn is_cross_shell_nested_command(command_str: &str, shell_name: &str) -> bool {
    let normalized = command_str.trim().to_lowercase();
    let is_prefixed = |prefixes: &[&str]| prefixes.iter().any(|p| normalized.starts_with(p));

    match shell_name {
        "pwsh" | "powershell" => is_prefixed(&["bash ", "sh ", "cmd ", "cmd.exe "]),
        "bash" | "sh" => is_prefixed(&["pwsh ", "powershell ", "cmd ", "cmd.exe "]),
        "cmd" => is_prefixed(&["pwsh ", "powershell ", "bash ", "sh "]),
        _ => false,
    }
}

/// Tool for executing shell commands.
pub struct BashTool {
    shell: ShellBackend,
    /// Optional workspace directory to execute commands in.
    workspace: Option<PathBuf>,
}

impl BashTool {
    pub fn new(config: &BashConfig) -> Self {
        let shell = select_shell(config);
        info!("BashTool initialized using shell: {}", shell.name());
        Self { shell, workspace: None }
    }

    /// Creates a new `BashTool` with a specific workspace directory.
    pub fn with_workspace(config: &BashConfig, workspace: PathBuf) -> Self {
        Self {
            shell: select_shell(config),
            workspace: Some(workspace),
        }
    }

    fn resolve_working_dir(&self, context: Option<&ToolContext>) -> Option<PathBuf> {
        if let Some(workspace) = &self.workspace {
            return Some(workspace.clone());
        }
        context
            .and_then(|ctx| ctx.environment.as_ref())
            .and_then(|env| env.project_dir.as_deref())
            .filter(|project_dir| !project_dir.trim().is_empty())
            .map(PathBuf::from)
    }
}

#[async_trait]
/// Implementation of the `Tool` trait for BashTool.
impl Tool for BashTool {
    /// Returns the tool definition for BashTool.
    fn definition(&self) -> RegisteredToolDefinition {
        RegisteredToolDefinition {
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
                    "timeout_ms": { "type": "integer", "description": "Timeout in milliseconds (default 3600000)" }
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
        if is_cross_shell_nested_command(command_str, self.shell.name()) {
            return Ok(ToolOutput {
                content: format!(
                    "Cross-shell nesting is not allowed. Current shell: '{}', command starts with another shell launcher.",
                    self.shell.name()
                ),
                is_error: true,
                child_session: None,
                images: Vec::new(),
            });
        }
        let timeout_ms = input["timeout_ms"].as_u64().unwrap_or(3600000);
        let run_in_background = input["run_in_background"].as_bool().unwrap_or(false);

        if run_in_background {
            let shell = self.shell.clone();
            let cmd_str_owned = command_str.to_string();
            let workspace = self.resolve_working_dir(context.as_ref());
            let ctx = context.clone();

            tokio::spawn(async move {
                let mut cmd = shell.build_command(&cmd_str_owned);
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
                child_session: None,
                images: Vec::new(),
            });
        }

        let mut cmd = self.shell.build_command(command_str);
        if let Some(ws) = self.resolve_working_dir(context.as_ref()) {
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

        let mut stdout_bytes = Vec::new();
        let mut stderr_bytes = Vec::new();

        const LOG_FLUSH_INTERVAL_MS: u128 = 200;

        let read_fut = async {
            let mut stdout_reader = BufReader::new(stdout);
            let mut stderr_reader = BufReader::new(stderr);

            let mut stdout_done = false;
            let mut stderr_done = false;

            let mut pending_stdout = String::new();
            let mut pending_stderr = String::new();
            let mut last_flush = Instant::now();

            while !stdout_done || !stderr_done {
                tokio::select! {
                    read_res = async {
                        let mut buf = Vec::new();
                        stdout_reader.read_until(b'\n', &mut buf).await.map(|n| (n, buf))
                    }, if !stdout_done => {
                        match read_res {
                            Ok((0, _)) => stdout_done = true,
                            Ok((_, chunk)) => {
                                stdout_bytes.extend_from_slice(&chunk);
                                let (decoded, _had_lossy) = decode_lossy_with_flag(&chunk);
                                pending_stdout.push_str(&decoded);

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
                            Err(e) => {
                                stderr_bytes.extend_from_slice(format!("Error reading stdout: {}\n", e).as_bytes());
                                stdout_done = true;
                            }
                        }
                    }
                    read_res = async {
                        let mut buf = Vec::new();
                        stderr_reader.read_until(b'\n', &mut buf).await.map(|n| (n, buf))
                    }, if !stderr_done => {
                        match read_res {
                            Ok((0, _)) => stderr_done = true,
                            Ok((_, chunk)) => {
                                stderr_bytes.extend_from_slice(&chunk);
                                let (decoded, _had_lossy) = decode_lossy_with_flag(&chunk);
                                pending_stderr.push_str(&decoded);

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
                            Err(e) => {
                                stderr_bytes.extend_from_slice(format!("Error reading stderr: {}\n", e).as_bytes());
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
                let (stdout_text, stdout_encoding) = decode_command_output(&stdout_bytes);
                let (stderr_text, stderr_encoding) = decode_command_output(&stderr_bytes);
                let content = format!(
                    "exit_code: {}\nstdout_encoding: {}\nstderr_encoding: {}\nstdout:\n{}\nstderr:\n{}",
                    exit_code,
                    stdout_encoding,
                    stderr_encoding,
                    truncate(&stdout_text, 100_000),
                    truncate(&stderr_text, 10_000)
                );
                Ok(ToolOutput {
                    content,
                    is_error: !status.success(),
                    child_session: None,
                    images: Vec::new(),
                })
            }
            Ok(Err(e)) => Ok(ToolOutput {
                content: format!("Failed to execute command: {}", e),
                is_error: true,
                child_session: None,
                images: Vec::new(),
            }),
            Err(_) => {
                let _ = child.kill().await;
                let (stdout_text, _) = decode_command_output(&stdout_bytes);
                let (stderr_text, _) = decode_command_output(&stderr_bytes);
                let content = format!(
                    "Command timed out after {}ms\nstdout so far:\n{}\nstderr so far:\n{}",
                    timeout_ms,
                    truncate(&stdout_text, 100_000),
                    truncate(&stderr_text, 10_000)
                );
                warn!("{content}");
                Ok(ToolOutput {
                    content,
                    is_error: true,
                    child_session: None,
                    images: Vec::new(),
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

fn decode_lossy_with_flag(bytes: &[u8]) -> (String, bool) {
    let decoded = String::from_utf8_lossy(bytes);
    let had_lossy = matches!(decoded, Cow::Owned(_));
    (decoded.into_owned(), had_lossy)
}

fn decode_command_output(bytes: &[u8]) -> (String, &'static str) {
    #[cfg(target_os = "windows")]
    if let Some(decoded) = decode_utf16le(bytes) {
        return (decoded, "utf16le");
    }

    if let Ok(decoded) = String::from_utf8(bytes.to_vec()) {
        return (decoded, "utf8");
    }

    let (decoded, _) = decode_lossy_with_flag(bytes);
    (decoded, "lossy")
}

#[cfg(target_os = "windows")]
fn decode_utf16le(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 || !bytes.len().is_multiple_of(2) {
        return None;
    }

    let zero_byte_count = bytes.iter().filter(|&&byte| byte == 0).count();
    if zero_byte_count * 4 < bytes.len() {
        return None;
    }

    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();

    String::from_utf16(&units).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

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

    #[test]
    fn decode_lossy_detects_invalid_utf8() {
        let (decoded_utf8, utf8_lossy) = decode_lossy_with_flag(b"hello\n");
        assert_eq!(decoded_utf8, "hello\n");
        assert!(!utf8_lossy);

        let invalid = [0x66, 0x6F, 0x80, 0x6F];
        let (decoded_invalid, invalid_lossy) = decode_lossy_with_flag(&invalid);
        assert!(decoded_invalid.contains('\u{FFFD}'));
        assert!(invalid_lossy);
    }

    #[test]
    fn decode_command_output_prefers_complete_utf8() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice("多 Agent".as_bytes());
        let (decoded, encoding) = decode_command_output(&bytes);
        assert_eq!(decoded, "多 Agent");
        assert_eq!(encoding, "utf8");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn decode_command_output_supports_utf16le() {
        let utf16: Vec<u8> = "多 Agent\r\n"
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        let (decoded, encoding) = decode_command_output(&utf16);
        assert_eq!(decoded, "多 Agent\r\n");
        assert_eq!(encoding, "utf16le");
    }

    #[test]
    fn cross_shell_nesting_is_detected() {
        assert!(is_cross_shell_nested_command("powershell -Command \"echo hi\"", "bash"));
        assert!(is_cross_shell_nested_command("bash -lc \"echo hi\"", "pwsh"));
        assert!(is_cross_shell_nested_command("cmd /c dir", "sh"));
        assert!(!is_cross_shell_nested_command("echo hello", "bash"));
    }

    #[test]
    fn resolve_working_dir_prefers_workspace_when_present() {
        let config = BashConfig::default();
        let tool = BashTool::with_workspace(&config, PathBuf::from("D:/fallback"));
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let ctx = ToolContext {
            event_tx: tx,
            tool_use_id: "tool-1".to_string(),
            session_id: "session-1".to_string(),
            task_store: None,
            skill_registry: None,
            read_files: Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new())),
            turn_read_state: None,
            environment: Some(crate::prompt::EnvironmentSnapshot {
                config_dir: "D:/config".to_string(),
                project_dir: Some("D:/project".to_string()),
                platform: "windows".to_string(),
                shell: "pwsh".to_string(),
                git_branch: None,
                git_status_summary: None,
                recent_commits: None,
                model_id: None,
                current_date: "2026-05-05".to_string(),
            }),
            shared_environment: None,
            cancellation_token: None,
            visible_tool_names: Arc::new(std::collections::HashSet::new()),
        };
        assert_eq!(tool.resolve_working_dir(Some(&ctx)), Some(PathBuf::from("D:/fallback")));
    }

    #[test]
    fn resolve_working_dir_falls_back_to_context_project_dir() {
        let config = BashConfig::default();
        let tool = BashTool::new(&config);
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let ctx = ToolContext {
            event_tx: tx,
            tool_use_id: "tool-1".to_string(),
            session_id: "session-1".to_string(),
            task_store: None,
            skill_registry: None,
            read_files: Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new())),
            turn_read_state: None,
            environment: Some(crate::prompt::EnvironmentSnapshot {
                config_dir: "D:/config".to_string(),
                project_dir: Some("D:/project".to_string()),
                platform: "windows".to_string(),
                shell: "pwsh".to_string(),
                git_branch: None,
                git_status_summary: None,
                recent_commits: None,
                model_id: None,
                current_date: "2026-05-05".to_string(),
            }),
            shared_environment: None,
            cancellation_token: None,
            visible_tool_names: Arc::new(std::collections::HashSet::new()),
        };
        assert_eq!(tool.resolve_working_dir(Some(&ctx)), Some(PathBuf::from("D:/project")));
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
