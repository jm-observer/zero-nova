use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use tokio::fs;

pub struct WriteTool {
    pub root_dir: Option<std::path::PathBuf>,
}

impl WriteTool {
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
}

#[async_trait]
impl Tool for WriteTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Write".to_string(),
            description: "Write content to a file. Requires the file to have been read first if it exists.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "The absolute path to the file to write" },
                    "content": { "type": "string", "description": "The content to write to the file" }
                },
                "required": ["file_path", "content"]
            }),
            defer_loading: false,
        }
    }

    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        let file_path_str = input["file_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'file_path'"))?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'content'"))?;

        let full_path = match self.validate_path(file_path_str) {
            Ok(p) => p,
            Err(out) => return Ok(out),
        };

        // Pre-read enforcement for existing files
        if full_path.exists() {
            if let Some(ctx) = &context {
                let read_files = ctx.read_files.lock().await;
                if !read_files.contains(file_path_str) {
                    return Ok(ToolOutput {
                        content: format!(
                            "Error: You must read the file before writing to it. Use the Read tool first on {}",
                            file_path_str
                        ),
                        is_error: true,
                    });
                }
            }
        }

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            if let Err(e) = fs::create_dir_all(parent).await {
                return Ok(ToolOutput {
                    content: format!("Failed to create directory structure: {}", e),
                    is_error: true,
                });
            }
        }

        match fs::write(&full_path, content).await {
            Ok(_) => Ok(ToolOutput {
                content: format!("Successfully written to {}", file_path_str),
                is_error: false,
            }),
            Err(e) => Ok(ToolOutput {
                content: format!("Failed to write file: {}", e),
                is_error: true,
            }),
        }
    }
}
