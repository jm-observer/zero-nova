use super::ShellBackend;
use crate::config::BashConfig;
use tokio::process::Command;
use which::which;

/// PowerShell 后端 (pwsh 或 powershell)
#[derive(Clone)]
pub struct PowerShellBackend {
    executable: String, // "pwsh" 或 "powershell"
}

impl PowerShellBackend {
    /// 检测可用的 PowerShell 可执行文件
    pub fn detect() -> Option<Self> {
        // 优先检测 pwsh (PowerShell 7+, 跨平台, UTF-8)
        if which("pwsh").is_ok() {
            return Some(Self {
                executable: "pwsh".into(),
            });
        }
        // 降级到 Windows PowerShell 5.x
        if which("powershell").is_ok() {
            return Some(Self {
                executable: "powershell".into(),
            });
        }
        None
    }

    pub(super) fn name(&self) -> &str {
        &self.executable
    }

    pub(super) fn build_command(&self, command_str: &str) -> Command {
        // 使用 Tokio Command 以支持异步
        let mut cmd = Command::new(&self.executable);
        if self.executable == "powershell" {
            // Windows PowerShell 5.x 需要额外设置编码
            let wrapped = format!(
                "$OutputEncoding = [System.Text.Encoding]::UTF8; [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; $PSDefaultParameterValues['*:Encoding'] = 'utf8'; {}",
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

/// cmd.exe 后端
#[derive(Clone)]
pub struct CmdBackend;

impl CmdBackend {
    pub(super) fn name(&self) -> &str {
        "cmd"
    }

    pub(super) fn build_command(&self, command_str: &str) -> Command {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", command_str]);
        cmd
    }
}

/// Windows 平台的 shell 选择逻辑
pub fn select_shell(config: &BashConfig) -> ShellBackend {
    // 1. 配置覆盖
    if let Some(shell) = &config.shell {
        match shell.to_lowercase().as_str() {
            "pwsh" | "powershell" => {
                if let Some(ps) = PowerShellBackend::detect() {
                    return ShellBackend::PowerShell(ps);
                }
            }
            "cmd" => return ShellBackend::Cmd(CmdBackend),
            _ => {} // 忽略无效值，走自动检测
        }
    }

    // 2. 自动检测: pwsh > powershell > cmd
    if let Some(ps) = PowerShellBackend::detect() {
        return ShellBackend::PowerShell(ps);
    }
    ShellBackend::Cmd(CmdBackend)
}
