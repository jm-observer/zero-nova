use serde::Serialize;
use std::path::Path;
use std::time::SystemTime;

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
