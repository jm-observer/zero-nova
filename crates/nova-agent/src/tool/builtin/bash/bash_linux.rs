/// Unix 平台专用的 shell 后端实现。
///
/// 包含 sh 和 bash 后端。
use super::ShellBackend;
use crate::config::BashConfig;
use tokio::process::Command;
use which::which;

/// sh 后端
#[derive(Clone)]
pub struct UnixSh;

impl UnixSh {
    pub(super) fn name(&self) -> &str {
        "sh"
    }

    pub(super) fn build_command(&self, command_str: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", command_str]);
        cmd
    }
}

/// bash 后端
#[derive(Clone)]
pub struct UnixBash;

impl UnixBash {
    pub(super) fn name(&self) -> &str {
        "bash"
    }

    pub(super) fn build_command(&self, command_str: &str) -> Command {
        let mut cmd = Command::new("bash");
        cmd.args(["-lc", command_str]);
        cmd
    }
}

/// Unix 平台的 shell 选择逻辑
pub fn select_shell(config: &BashConfig) -> ShellBackend {
    // 1. 配置覆盖
    if let Some(shell) = &config.shell {
        match shell.to_lowercase().as_str() {
            "sh" => return ShellBackend::UnixSh(UnixSh),
            "bash" => {
                if which("bash").is_ok() {
                    return ShellBackend::UnixBash(UnixBash);
                }
                return ShellBackend::UnixSh(UnixSh);
            }
            _ => {} // 忽略无效值，走自动检测
        }
    }

    // 2. 自动检测: bash 优先，回退 sh
    if which("bash").is_ok() {
        ShellBackend::UnixBash(UnixBash)
    } else {
        ShellBackend::UnixSh(UnixSh)
    }
}
