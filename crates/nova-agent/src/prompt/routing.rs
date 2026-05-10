/// 文件 I/O 与项目上下文加载。
///
/// 此模块专注于文件读取操作，确保 IO 与校验解耦。

use std::path::{Path, PathBuf};
use std::fs;
use std::io;
use super::types::SectionName;

/// 项目上下文最大字符数（约 4000 token）
pub const MAX_PROJECT_CONTEXT_CHARS: usize = 16000;

/// 项目上下文文件名数组（按优先级排列）
pub const PROJECT_CONTEXT_FILES: [&str; 2] = ["PROJECT.md", "NOVA.md"];

/// 从项目目录加载开发项目提示词（异步版本，用于 async context）。
///
/// 按顺序查找文件，返回包含来源标记的合并内容。
pub async fn load_developer_project_prompt_async(
    project_dir: Option<&Path>,
    files: &[String],
) -> Option<String> {
    let project_dir = project_dir?;
    if files.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    for file_name in files {
        let path = project_dir.join(file_name);
        match tokio::fs::read_to_string(&path).await {
            Ok(content) if !content.trim().is_empty() => {
                log::info!(
                    "Loaded developer project prompt from {:?} ({} chars) [async]",
                    path,
                    content.len()
                );
                parts.push(format!("### Source: {}\n{}", file_name, content.trim_end()));
            }
            Ok(_) => {
                log::debug!("Developer project prompt file {:?} is empty, skipping", path);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
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

/// 同步加载项目上下文文件。
pub fn load_project_context(
    project_dir: Option<&Path>,
    configured_path: Option<&Path>,
) -> Option<String> {
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

/// 异步加载项目上下文文件。
pub async fn load_project_context_async(
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

fn load_single_project_context(path: &Path) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(content) if !content.trim().is_empty() => {
            log::info!(
                "Loaded project context from {:?} ({} chars)",
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
        Ok(_) => None,
        Err(_) => None,
    }
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
        Ok(_) => None,
        Err(_) => None,
    }
}
