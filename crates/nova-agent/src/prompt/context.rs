/// 环境快照与项目上下文加载。
///
/// 此模块负责：
/// - 采集运行时环境信息（git 分支、shell 类型等）
/// - 异步/同步加载项目上下文和项目提示词文件
/// - Shell 命令检测与归一化

use crate::message::{ContentBlock, Message};
use super::templates::{PROJECT_CONTEXT_FILES, MAX_PROJECT_CONTEXT_CHARS};
use std::path::{Path, PathBuf};
use tokio::runtime::Handle;

// ---------------------------------------------------------------------------
//  环境快照 — EnvironmentSnapshot
// ---------------------------------------------------------------------------

/// 运行时环境快照，在会话创建时采集一次。
#[derive(Debug, Clone, Default)]
pub struct EnvironmentSnapshot {
    /// 配置目录
    pub config_dir: String,
    /// 项目目录
    pub project_dir: Option<String>,
    /// 操作系统平台
    pub platform: String,
    /// Shell 类型
    pub shell: String,
    /// Git 当前分支（非 git 目录时为 None）
    pub git_branch: Option<String>,
    /// Git 状态摘要
    pub git_status_summary: Option<String>,
    /// 最近提交摘要（oneline 格式，最多 5 条）
    pub recent_commits: Option<String>,
    /// 当前使用的模型 ID
    pub model_id: Option<String>,
    /// 当前日期
    pub current_date: String,
}

impl EnvironmentSnapshot {
    /// 采集当前运行环境信息。
    ///
    /// git 命令失败时（非 git 目录或无 git 可执行文件）静默跳过，
    /// 确保在任何环境下都能正常工作。
    pub async fn collect(config_dir: &Path, project_dir: Option<&Path>) -> Self {
        let config_dir_path = config_dir;
        let config_dir = config_dir_path.to_string_lossy().to_string();
        let project_dir_path = project_dir;
        let project_dir = project_dir_path.map(|path| path.to_string_lossy().to_string());

        let platform = std::env::consts::OS.to_string();

        let shell = detect_shell_command();

        let git_branch = if let Some(project_dir_path) = project_dir_path {
            Self::run_git(project_dir_path, &["rev-parse", "--abbrev-ref", "HEAD"]).await
        } else {
            None
        };

        let git_status_summary = if let Some(project_dir_path) = project_dir_path {
            Self::run_git(project_dir_path, &["status", "--short"]).await.map(|s| {
                let count = s.lines().filter(|l| !l.is_empty()).count();
                if count == 0 {
                    "clean".to_string()
                } else {
                    format!("{} changed files", count)
                }
            })
        } else {
            None
        };

        let recent_commits = if let Some(project_dir_path) = project_dir_path {
            Self::run_git(project_dir_path, &["log", "--oneline", "-5"]).await
        } else {
            None
        };

        let current_date = chrono::Local::now().format("%Y-%m-%d").to_string();

        Self {
            config_dir,
            project_dir,
            platform,
            shell,
            git_branch,
            git_status_summary,
            recent_commits,
            model_id: None,
            current_date,
        }
    }

    /// 运行 git 命令并返回 stdout 输出。
    /// 失败时返回 None（不报错）。
    pub async fn run_git(config_dir: &Path, args: &[&str]) -> Option<String> {
        let result = tokio::process::Command::new("git")
            .args(args)
            .current_dir(config_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .await;

        match result {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            }
            _ => None,
        }
    }

    /// 生成 prompt section 文本。
    pub fn to_prompt_text(&self) -> String {
        let mut lines = vec![
            format!("Config directory: {}", self.config_dir),
            format!(
                "Project directory: {}",
                self.project_dir.as_deref().unwrap_or("(not set)")
            ),
            format!("Platform: {}", self.platform),
            format!("Shell: {}", self.shell),
            format!("Date: {}", self.current_date),
        ];

        if let Some(branch) = &self.git_branch {
            lines.push(format!("Git branch: {}", branch));
        }
        if let Some(status) = &self.git_status_summary {
            lines.push(format!("Git status: {}", status));
        }
        if let Some(commits) = &self.recent_commits {
            lines.push(String::new()); // 空行分隔
            lines.push("Recent commits:".to_string());
            lines.push(commits.clone());
        }
        if let Some(model) = &self.model_id {
            lines.push(format!("Model: {}", model));
        }

        lines.join("\n")
    }
}

// ---------------------------------------------------------------------------
//  Shell 检测
// ---------------------------------------------------------------------------

fn normalize_shell_command(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lowered = trimmed.to_lowercase();
    if lowered.contains("pwsh") {
        return Some("pwsh".to_string());
    }
    if lowered.contains("powershell") {
        return Some("powershell".to_string());
    }
    if lowered.contains("cmd.exe") || lowered.ends_with("cmd") {
        return Some("cmd".to_string());
    }
    if lowered.contains("bash") {
        return Some("bash".to_string());
    }
    if lowered.contains("zsh") {
        return Some("zsh".to_string());
    }
    if lowered.contains("fish") {
        return Some("fish".to_string());
    }
    if lowered.contains("sh") {
        return Some("sh".to_string());
    }

    let stem = std::path::Path::new(trimmed)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(trimmed)
        .to_lowercase();

    if stem.is_empty() {
        None
    } else {
        Some(stem)
    }
}

/// 检测当前 shell 命令（基于平台和环境变量）。
pub fn detect_shell_command() -> String {
    #[cfg(target_os = "windows")]
    {
        if which::which("pwsh").is_ok() {
            return "pwsh".to_string();
        }
        if let Ok(comspec) = std::env::var("COMSPEC") {
            if let Some(normalized) = normalize_shell_command(&comspec) {
                return normalized;
            }
        }
        "cmd".to_string()
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(shell) = std::env::var("SHELL") {
            if let Some(normalized) = normalize_shell_command(&shell) {
                return normalized;
            }
        }
        "sh".to_string()
    }
}

// ---------------------------------------------------------------------------
//  项目上下文加载
// ---------------------------------------------------------------------------

/// 从工作区加载项目上下文文件。
///
/// 按优先级查找 PROJECT.md → NOVA.md，找到第一个非空文件即返回。
/// 所有文件都不存在或为空时返回 None。
pub async fn load_project_context_async(project_dir: Option<&Path>) -> Option<String> {
    load_project_context_with_config_async(project_dir, None).await
}

/// 异步从工作区加载项目上下文文件，支持显式路径。
pub async fn load_project_context_with_config_async(
    project_dir: Option<&Path>,
    configured_path: Option<&Path>,
) -> Option<String> {
    if let Some(path) = configured_path {
        return load_single_project_context_async(path).await;
    }

    let project_dir = project_dir?;

    for filename in PROJECT_CONTEXT_FILES {
        let path = project_dir.join(filename);
        if let Some(content) = load_single_project_context_async(&path).await {
            return Some(content);
        }
    }
    None
}

async fn load_single_project_context_async(path: &Path) -> Option<String> {
    match tokio::fs::read_to_string(path).await {
        Ok(content) if !content.trim().is_empty() => {
            log::info!(
                "Loaded project context from {:?} ({} chars) [async]",
                path,
                content.len()
            );
            if content.len() > MAX_PROJECT_CONTEXT_CHARS {
                let truncated = &content[..MAX_PROJECT_CONTEXT_CHARS];
                let last_newline = truncated.rfind('\n').unwrap_or(MAX_PROJECT_CONTEXT_CHARS);
                let mut result = truncated[..last_newline].to_string();
                result.push_str("\n\n[... truncated due to size limit ...]");
                return Some(result);
            }
            Some(content)
        }
        _ => None,
    }
}

/// 同步加载项目上下文文件（用于非 async 上下文）。
pub fn load_project_context(project_dir: Option<&Path>) -> Option<String> {
    load_project_context_with_config(project_dir, None)
}

/// 从工作区加载项目上下文文件，支持显式配置文件路径。
pub fn load_project_context_with_config(project_dir: Option<&Path>, configured_path: Option<&Path>) -> Option<String> {
    if let Some(path) = configured_path {
        return load_single_project_context(path);
    }

    let project_dir = project_dir?;

    for filename in PROJECT_CONTEXT_FILES {
        let path = project_dir.join(filename);
        if let Some(content) = load_single_project_context(&path) {
            return Some(content);
        }
    }
    None
}

fn load_single_project_context(path: &Path) -> Option<String> {
    match read_to_string_runtime_aware(path) {
        Ok(content) if !content.trim().is_empty() => {
            log::info!("Loaded project context from {:?} ({} chars)", path, content.len());
            if content.len() > MAX_PROJECT_CONTEXT_CHARS {
                let truncated = &content[..MAX_PROJECT_CONTEXT_CHARS];
                let last_newline = truncated.rfind('\n').unwrap_or(MAX_PROJECT_CONTEXT_CHARS);
                let mut result = truncated[..last_newline].to_string();
                result.push_str("\n\n[... truncated due to size limit ...]");
                return Some(result);
            }
            Some(content)
        }
        Ok(_) => {
            log::debug!("Project context file {:?} is empty, skipping", path);
            None
        }
        Err(_) => None,
    }
}

// ---------------------------------------------------------------------------
//  开发项目提示词加载（Plan 2）
// ---------------------------------------------------------------------------

/// 异步从项目根目录加载开发项目提示词。
///
/// 处理规则：
/// 1. `project_dir` 为空则直接返回 `None`
/// 2. 按 `files` 顺序逐个检查 `<project_dir>/<file>`
/// 3. 文件不存在则跳过
/// 4. 文件存在但内容为空白则跳过
/// 5. 文件读取失败则记录 `warn!` 并继续
/// 6. 命中多个文件时按顺序拼接
pub async fn load_developer_project_prompt_async(project_dir: Option<&Path>, files: &[String]) -> Option<String> {
    let Some(project_dir) = project_dir else {
        log::info!("Skip loading developer project prompt: session project_dir is not set");
        return None;
    };
    if files.is_empty() {
        log::info!("Skip loading developer project prompt: developer_prompt_files is empty");
        return None;
    }
    let mut parts = Vec::new();

    for file_name in files {
        let path = project_dir.join(file_name);
        match tokio::fs::read_to_string(&path).await {
            Ok(content) if !content.trim().is_empty() => {
                log::info!(
                    "Loaded developer project prompt from {:?} ({} chars)",
                    path,
                    content.len()
                );
                parts.push(format!("### Source: {}\n{}", file_name, content.trim_end()));
            }
            Ok(_) => {
                log::debug!("Developer project prompt file {:?} is empty, skipping", path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                log::warn!("Failed to read developer project prompt file {:?}: {}", path, e);
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n---\n\n"))
    }
}

/// 同步从项目根目录加载开发项目提示词。
pub fn load_developer_project_prompt(project_dir: Option<&Path>, files: &[String]) -> Option<String> {
    let project_dir = project_dir?;
    let mut parts = Vec::new();

    for file_name in files {
        let path = project_dir.join(file_name);
        match read_to_string_runtime_aware(&path) {
            Ok(content) if !content.trim().is_empty() => {
                log::info!(
                    "Loaded developer project prompt from {:?} ({} chars)",
                    path,
                    content.len()
                );
                parts.push(format!("### Source: {}\n{}", file_name, content.trim_end()));
            }
            Ok(_) => {
                log::debug!("Developer project prompt file {:?} is empty, skipping", path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                log::warn!("Failed to read developer project prompt file {:?}: {}", path, e);
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n---\n\n"))
    }
}

fn read_to_string_runtime_aware(path: &Path) -> std::io::Result<String> {
    if let Ok(handle) = Handle::try_current() {
        // 在 Tokio 运行时内，使用 block_in_place 让调度器可迁移其它任务，避免热路径直接阻塞。
        tokio::task::block_in_place(|| handle.block_on(tokio::fs::read_to_string(path)))
    } else {
        std::fs::read_to_string(path)
    }
}
