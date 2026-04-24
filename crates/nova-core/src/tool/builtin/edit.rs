use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use tokio::fs;

pub struct EditTool {
    pub root_dir: Option<std::path::PathBuf>,
}

impl EditTool {
    pub fn new(root_dir: Option<std::path::PathBuf>) -> Self {
        Self { root_dir }
    }

    fn validate_path(&self, path_str: &str) -> Result<std::path::PathBuf, ToolOutput> {
        let path = Path::new(path_str);

        // Ensure absolute path if root_dir is not set, or validate against root_dir
        if let Some(root) = &self.root_dir {
            let full_path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                root.join(path)
            };

            // Basic directory traversal protection
            if path_str.contains("..") {
                return Err(ToolOutput {
                    content: "Access denied: directory traversal detected".to_string(),
                    is_error: true,
                });
            }

            if !full_path.starts_with(root) {
                return Err(ToolOutput {
                    content: "Access denied: path is outside of allowed workspace".to_string(),
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
impl Tool for EditTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Edit".to_string(),
            description: "Performs exact string replacements in files. Best for precise modifications.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "The absolute path to the file to modify" },
                    "old_string": { "type": "string", "description": "The text to replace" },
                    "new_string": { "type": "string", "description": "The text to replace it with" },
                    "replace_all": { "type": "boolean", "default": false, "description": "Replace all occurrences. If false, old_string must be unique." }
                },
                "required": ["file_path", "old_string", "new_string"]
            }),
            defer_loading: false,
        }
    }

    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        let file_path_str = input["file_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'file_path'"))?;
        let old_string = input["old_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'old_string'"))?;
        let new_string = input["new_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'new_string'"))?;
        let replace_all = input["replace_all"].as_bool().unwrap_or(false);

        let full_path = match self.validate_path(file_path_str) {
            Ok(p) => p,
            Err(out) => return Ok(out),
        };

        // Pre-read enforcement
        if let Some(ctx) = &context {
            let read_files = ctx.read_files.lock().await;
            if !read_files.contains(file_path_str) {
                return Ok(ToolOutput {
                    content: format!(
                        "Error: You must read the file before editing it. Use the Read tool first on {}",
                        file_path_str
                    ),
                    is_error: true,
                });
            }
        }

        if !full_path.exists() {
            return Ok(ToolOutput {
                content: format!("File not found: {}", file_path_str),
                is_error: true,
            });
        }

        let content = match fs::read_to_string(&full_path).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolOutput {
                    content: format!("Failed to read file: {}", e),
                    is_error: true,
                })
            }
        };

        let occurrences = content.matches(old_string).count();

        if occurrences == 0 {
            return Ok(ToolOutput {
                content: "Error: 'old_string' not found in file.".to_string(),
                is_error: true,
            });
        }

        if !replace_all && occurrences > 1 {
            return Ok(ToolOutput {
                content: format!("Error: 'old_string' is not unique (found {} occurrences). Use 'replace_all: true' if intended, or provide a more specific string.", occurrences),
                is_error: true,
            });
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        match fs::write(&full_path, new_content).await {
            Ok(_) => Ok(ToolOutput {
                content: format!(
                    "Successfully edited {}. Replaced {} occurrence(s).",
                    file_path_str, occurrences
                ),
                is_error: false,
            }),
            Err(e) => Ok(ToolOutput {
                content: format!("Failed to write file: {}", e),
                is_error: true,
            }),
        }
    }
}
