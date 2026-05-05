use serde::Serialize;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;
use tauri::Manager;

/// ============================================================
/// Phase 3: 异步统一 — 使用 spawn_blocking 转换阻塞 I/O
/// ============================================================
/// 读取文件内容（从线程池执行，避免阻塞 tokio 异步运行时）
#[tauri::command]
pub async fn file_read_large(file_path: String, max_buffer: u64) -> Result<FileBuffer, String> {
    let file_path_for_mime = file_path.clone();
    let data = tokio::task::spawn_blocking(move || {
        // 阻塞操作在线程池中执行，不占用 tokio worker
        std::fs::read(&file_path)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
    .map_err(|e| e.to_string())?;

    // 检查文件大小限制
    if data.len() as u64 > max_buffer {
        return Err(format!("File exceeds max buffer size: {} > {}", data.len(), max_buffer));
    }

    Ok(FileBuffer {
        size: data.len() as u64,
        mime: detect_mime(&file_path_for_mime),
    })
}

/// FileBuffer 响应格式
#[derive(Serialize)]
pub struct FileBuffer {
    pub size: u64,
    pub mime: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDirEntry {
    pub name: String,
    pub relative_path: String,
    pub is_dir: bool,
}

#[tauri::command]
pub async fn project_dir_list(
    app: tauri::AppHandle,
    relative_path: Option<String>,
    base_dir: Option<String>,
) -> Result<Vec<ProjectDirEntry>, String> {
    // 仅用于桌面端本地目录交互，不作为会话级文件树的数据源。
    // `@` 选择器应始终走后端 session.file_tree.list，避免本地/远端双权威。
    let base_dir = resolve_project_dir(&app, base_dir.as_deref())?;
    let safe_relative = sanitize_relative_path(relative_path.as_deref().unwrap_or(""))?;
    tokio::task::spawn_blocking(move || list_project_dir_entries(base_dir, safe_relative))
        .await
        .map_err(|e| format!("目录读取任务失败: {}", e))?
}

fn list_project_dir_entries(base_dir: PathBuf, safe_relative: PathBuf) -> Result<Vec<ProjectDirEntry>, String> {
    let target_dir = if safe_relative.as_os_str().is_empty() {
        base_dir.clone()
    } else {
        base_dir.join(&safe_relative)
    };
    let target_dir = std::fs::canonicalize(&target_dir).map_err(|e| format!("目录不可访问: {}", e))?;
    if !target_dir.starts_with(&base_dir) {
        return Err("路径越界：仅允许访问项目目录内文件".to_string());
    }
    if !target_dir.is_dir() {
        return Err("目标路径不是目录".to_string());
    }

    let mut entries = Vec::new();
    for entry_result in std::fs::read_dir(&target_dir).map_err(|e| format!("读取目录失败: {}", e))? {
        let entry = entry_result.map_err(|e| format!("读取目录项失败: {}", e))?;
        let file_type = entry.file_type().map_err(|e| format!("读取目录项类型失败: {}", e))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let full_path = entry.path();
        let relative = full_path
            .strip_prefix(&base_dir)
            .map_err(|_| "计算相对路径失败".to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        entries.push(ProjectDirEntry {
            name,
            relative_path: relative,
            is_dir: file_type.is_dir(),
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}

fn resolve_project_dir(app: &tauri::AppHandle, override_base_dir: Option<&str>) -> Result<PathBuf, String> {
    if let Some(base_dir) = override_base_dir {
        return canonicalize_project_dir(base_dir);
    }

    let config = app.state::<crate::config::AppConfig>();
    let workspace = config
        .sidecar
        .workspace
        .as_ref()
        .cloned()
        .unwrap_or_else(|| config.config_dir.clone());
    let candidate = if workspace.file_name().is_some_and(|n| n == ".nova") {
        workspace.parent().map(Path::to_path_buf).unwrap_or(workspace)
    } else {
        workspace
    };
    canonicalize_project_dir(candidate)
}

fn canonicalize_project_dir(path: impl AsRef<Path>) -> Result<PathBuf, String> {
    std::fs::canonicalize(path.as_ref()).map_err(|e| format!("项目目录不可访问: {}", e))
}

fn sanitize_relative_path(input: &str) -> Result<PathBuf, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(PathBuf::new());
    }
    let raw = Path::new(trimmed);
    if raw.is_absolute() {
        return Err("relativePath 必须是相对路径".to_string());
    }
    if raw.components().any(|part| matches!(part, Component::ParentDir)) {
        return Err("relativePath 不能包含 ..".to_string());
    }

    let mut cleaned = PathBuf::new();
    for part in raw.components() {
        match part {
            Component::CurDir => {}
            Component::Normal(seg) => cleaned.push(seg),
            _ => return Err("relativePath 包含非法路径片段".to_string()),
        }
    }
    Ok(cleaned)
}

/// 检测 MIME 类型（从文件路径）
fn detect_mime(path: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "text/javascript",
        "ts" => "text/typescript",
        "md" => "text/markdown",
        "txt" => "text/plain",
        "xml" => "text/xml",
        "yaml" | "yml" => "text/yaml",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// 异步化的文件读取（使用 spawn_blocking 避免阻塞）
/// 使用 String 而非 &Path 以保证 'static 生命周期
async fn async_file_read(file_path: String) -> Result<Vec<u8>, String> {
    tokio::task::spawn_blocking(move || std::fs::read(&file_path))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
        .map_err(|e| e.to_string())
}

/// 异步化的文本文件读取
async fn async_file_read_to_string(file_path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || std::fs::read_to_string(&file_path))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
        .map_err(|e| e.to_string())
}

/// 异步化的文件元数据获取
async fn async_file_metadata(file_path: String) -> Result<std::fs::Metadata, String> {
    tokio::task::spawn_blocking(move || Path::new(&file_path).symlink_metadata())
        .await
        .map_err(|e| format!("Task join error: {}", e))?
        .map_err(|e| e.to_string())
}

/// 检查文件是否存在
#[tauri::command]
pub async fn file_exists(file_path: String) -> Result<bool, String> {
    Ok(Path::new(&file_path).exists())
}

/// 文件状态信息
#[derive(Serialize)]
pub struct FileInfo {
    pub size: u64,
    pub is_dir: bool,
    pub modified: String,
}

/// 获取文件状态信息（大小、是否目录、修改时间）
#[tauri::command]
pub async fn file_stat(file_path: String) -> Result<FileInfo, String> {
    // 使用 spawn_blocking 避免阻塞 tokio 运行时
    let metadata = async_file_metadata(file_path).await?;

    // 计算修改时间（简单方案：使用系统时间 - 文件创建到现在的间隔）
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok().map(|dur| dur.as_secs()))
        .map(|epoch_secs| {
            // 简单格式化：直接使用秒数日志
            format!("epoch:{}s", epoch_secs)
        })
        .unwrap_or_else(|| "unknown".to_string());

    Ok(FileInfo {
        size: metadata.len(),
        is_dir: metadata.is_dir(),
        modified,
    })
}

/// 读取文件内容
/// 对于文本文件返回 UTF-8 字符串，对于二进制文件返回 base64
#[derive(Serialize)]
pub struct FileReadResult {
    pub content: String,
    pub mime_type: String,
    pub is_binary: bool,
    pub size: u64,
}

#[tauri::command]
pub async fn file_read(file_path: String) -> Result<FileReadResult, String> {
    let path = Path::new(&file_path);
    if !path.exists() {
        return Err(format!("文件不存在: {}", file_path));
    }

    let metadata = async_file_metadata(file_path.clone()).await?;
    let size = metadata.len();

    // 根据扩展名判断是否为二进制
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    let binary_exts = [
        "png", "jpg", "jpeg", "gif", "bmp", "webp", "ico", "svg", "mp4", "avi", "mkv", "mov", "wmv", "flv", "webm",
        "mp3", "wav", "ogg", "flac", "aac", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "zip", "rar", "7z",
        "tar", "gz", "exe", "dll", "so", "dylib",
    ];

    let image_exts = ["png", "jpg", "jpeg", "gif", "bmp", "webp", "ico", "svg"];

    let is_binary = binary_exts.contains(&ext.as_str());

    let mime_type = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xls" => "application/vnd.ms-excel",
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "text/javascript",
        "ts" => "text/typescript",
        "md" => "text/markdown",
        "txt" => "text/plain",
        "xml" => "text/xml",
        "yaml" | "yml" => "text/yaml",
        _ => {
            if is_binary {
                "application/octet-stream"
            } else {
                "text/plain"
            }
        }
    }
    .to_string();

    if is_binary {
        // 图片文件：返回 base64 data URI
        if image_exts.contains(&ext.as_str()) {
            let data = async_file_read(file_path.clone()).await?;
            let mut encoded = String::new();
            encoded.push_str(&format!("data:{};base64,", mime_type));
            let b64 = base64_encode(&data);
            encoded.push_str(&b64);
            Ok(FileReadResult {
                content: encoded,
                mime_type,
                is_binary: true,
                size,
            })
        } else if ext == "xlsx" || ext == "xls" {
            // Excel 文件：返回 base64 编码的原始数据，前端用 SheetJS 解析
            let data = async_file_read(file_path.clone()).await?;
            let b64 = base64_encode(&data);
            Ok(FileReadResult {
                content: b64,
                mime_type,
                is_binary: true,
                size,
            })
        } else if ext == "docx" {
            // DOCX 文件：返回 base64 编码的原始数据，前端用 mammoth.js 解析
            let data = async_file_read(file_path.clone()).await?;
            let b64 = base64_encode(&data);
            Ok(FileReadResult {
                content: b64,
                mime_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document".to_string(),
                is_binary: true,
                size,
            })
        } else if ext == "pptx" || ext == "ppt" {
            // PPTX 文件：返回 base64 编码的原始数据，前端解析预览
            let data = async_file_read(file_path.clone()).await?;
            let b64 = base64_encode(&data);
            Ok(FileReadResult {
                content: b64,
                mime_type: "application/vnd.openxmlformats-officedocument.presentationml.presentation".to_string(),
                is_binary: true,
                size,
            })
        } else if ext == "pdf" {
            // PDF 文件：返回 base64 编码的原始数据，前端用 iframe 预览
            let data = async_file_read(file_path.clone()).await?;
            let b64 = base64_encode(&data);
            Ok(FileReadResult {
                content: b64,
                mime_type,
                is_binary: true,
                size,
            })
        } else {
            // 其他二进制文件不读取内容
            Ok(FileReadResult {
                content: String::new(),
                mime_type,
                is_binary: true,
                size,
            })
        }
    } else {
        // 文本文件 — 使用 spawn_blocking 避免阻塞
        let content = async_file_read_to_string(file_path.clone()).await?;
        Ok(FileReadResult {
            content,
            mime_type,
            is_binary: false,
            size,
        })
    }
}

/// 读取纯文本文件内容（使用 spawn_blocking 异步化）
#[tauri::command]
pub async fn file_read_text(file_path: String) -> Result<String, String> {
    async_file_read_to_string(file_path).await
}

/// 用系统默认程序打开文件
#[tauri::command]
pub async fn file_open(file_path: String) -> Result<(), String> {
    open::that(&file_path).map_err(|e| e.to_string())
}

/// 在文件管理器中定位文件
#[tauri::command]
pub async fn file_reveal(file_path: String) -> Result<(), String> {
    let path = Path::new(&file_path);
    #[allow(unused_variables)]
    if let Some(parent) = path.parent() {
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("explorer")
                .args(["/select,", &file_path])
                .spawn()
                .map_err(|e| e.to_string())?;
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .args(["-R", &file_path])
                .spawn()
                .map_err(|e| e.to_string())?;
        }
        #[cfg(target_os = "linux")]
        {
            // Linux 使用 xdg-open 打开文件所在目录
            let parent_str = parent.to_str().unwrap_or("");
            if parent_str.is_empty() {
                return Err("无法获取父目录路径".to_string());
            }
            std::process::Command::new("xdg-open")
                .arg(parent_str)
                .spawn()
                .map_err(|e| {
                    // 备用方案：尝试 nautilus --select
                    std::process::Command::new("nautilus")
                        .args(["--select", &file_path])
                        .spawn()
                        .map_err(|_| format!("Linux 文件显示失败: {}", e))
                })?;
        }
        Ok(())
    } else {
        Err("无法获取父目录".to_string())
    }
}

/// 文件另存为（使用 spawn_blocking 避免阻塞）
#[tauri::command]
pub async fn file_save_as(source_path: String, dest_path: String) -> Result<(), String> {
    let source = source_path.clone();
    let dest = dest_path.clone();
    tokio::task::spawn_blocking(move || std::fs::copy(&source, &dest))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 简易 Base64 编码（无外部依赖）
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_project_dir, list_project_dir_entries, sanitize_relative_path};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn make_temp_dir() -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("openflux-project-dir-test-{}", nanos));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sanitize_relative_path_rejects_absolute_and_parent_dir() {
        assert!(sanitize_relative_path("../a").is_err());
        assert!(sanitize_relative_path("a/../../b").is_err());

        #[cfg(target_os = "windows")]
        assert!(sanitize_relative_path("C:/temp").is_err());
        #[cfg(not(target_os = "windows"))]
        assert!(sanitize_relative_path("/tmp").is_err());
    }

    #[test]
    fn sanitize_relative_path_normalizes_current_dir() {
        let cleaned = sanitize_relative_path("./src/./ui").unwrap();
        assert_eq!(cleaned.to_string_lossy().replace('\\', "/"), "src/ui");
    }

    #[test]
    fn list_project_dir_entries_sorts_dir_first_then_name() {
        let base = make_temp_dir();
        let alpha_dir = base.join("Alpha");
        let beta_file = base.join("beta.txt");
        let gamma_file = base.join("Gamma.txt");

        fs::create_dir_all(&alpha_dir).unwrap();
        fs::write(&beta_file, "beta").unwrap();
        fs::write(&gamma_file, "gamma").unwrap();

        let canonical_base = fs::canonicalize(&base).unwrap();
        let entries = list_project_dir_entries(canonical_base, PathBuf::new()).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "Alpha");
        assert_eq!(entries[1].name, "beta.txt");
        assert_eq!(entries[2].name, "Gamma.txt");

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn canonicalize_project_dir_uses_override_when_provided() {
        let base = make_temp_dir();
        let expected = fs::canonicalize(&base).unwrap();

        let actual = canonicalize_project_dir(&base).unwrap();

        assert_eq!(actual, expected);

        let _ = fs::remove_dir_all(base);
    }
}
