use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use tokio::fs;

pub struct ReadTool {
    pub root_dir: Option<std::path::PathBuf>,
}

impl ReadTool {
    pub fn new(root_dir: Option<std::path::PathBuf>) -> Self {
        Self { root_dir }
    }

    fn validate_path(&self, path_str: &str) -> Result<std::path::PathBuf, ToolOutput> {
        let path = Path::new(path_str);
        if let Some(root) = &self.root_dir {
            let full_path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                root.join(path)
            };
            if path_str.contains("..") || !full_path.starts_with(root) {
                return Err(ToolOutput {
                    content: "Access denied: path is invalid or outside of workspace".to_string(),
                    is_error: true,
                });
            }
            Ok(full_path)
        } else {
            if !path.is_absolute() {
                return Err(ToolOutput {
                    content: format!("Error: 'file_path' must be an absolute path: {}", path_str),
                    is_error: true,
                });
            }
            Ok(path.to_path_buf())
        }
    }

    async fn read_text(&self, path: &Path, offset: usize, limit: usize) -> Result<ToolOutput> {
        match fs::read_to_string(path).await {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let start = offset.saturating_sub(1);
                if start >= lines.len() && !lines.is_empty() {
                    return Ok(ToolOutput {
                        content: format!("Offset {} is beyond file length ({} lines)", offset, lines.len()),
                        is_error: true,
                    });
                }

                let end = (start + limit).min(lines.len());
                let result_lines = &lines[start..end];

                let mut output = String::new();
                for (i, line) in result_lines.iter().enumerate() {
                    let line_num = start + i + 1;
                    // Tab-separated line numbers as per spec
                    output.push_str(&format!("{}\t{}\n", line_num, line));
                }

                Ok(ToolOutput {
                    content: output,
                    is_error: false,
                })
            }
            Err(e) => Ok(ToolOutput {
                content: format!("Failed to read file: {}", e),
                is_error: true,
            }),
        }
    }

    // Placeholder for multimodal support
    async fn read_image(&self, _path: &Path) -> Result<ToolOutput> {
        Ok(ToolOutput {
            content: "Image reading not yet supported in this version.".to_string(),
            is_error: true,
        })
    }
}

#[async_trait]
impl Tool for ReadTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Read".to_string(),
            description: "Read the contents of a file. Supports paging, PDF (placeholder), and images.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "The absolute path to the file to read" },
                    "offset": { "type": "integer", "description": "Line number to start reading from (1-based, default 1)" },
                    "limit": { "type": "integer", "description": "Number of lines to read (max 2000, default 2000)" },
                    "pages": { "type": "string", "description": "Page range for PDF files (e.g., '1-5') - Currently not implemented" }
                },
                "required": ["file_path"]
            }),
            defer_loading: false,
        }
    }

    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        let file_path_str = input["file_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'file_path'"))?;
        let offset = input["offset"].as_u64().unwrap_or(1) as usize;
        let limit = input["limit"].as_u64().unwrap_or(2000).min(2000) as usize;

        let full_path = match self.validate_path(file_path_str) {
            Ok(p) => p,
            Err(out) => return Ok(out),
        };

        // Track that this file has been read
        if let Some(ctx) = &context {
            ctx.read_files.lock().await.insert(file_path_str.to_string());
        }

        if !full_path.exists() {
            return Ok(ToolOutput {
                content: format!("File not found: {}", file_path_str),
                is_error: true,
            });
        }

        let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext.to_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" => self.read_image(&full_path).await,
            // Add other types here
            _ => self.read_text(&full_path, offset, limit).await,
        }
    }
}
