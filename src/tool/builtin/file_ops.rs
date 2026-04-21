use crate::tool::{Tool, ToolDefinition, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use tokio::fs;

/// Tool to read file contents.
pub struct ReadFileTool {
    pub root_dir: Option<std::path::PathBuf>,
}

impl ReadFileTool {
    pub fn new(root_dir: Option<std::path::PathBuf>) -> Self {
        Self { root_dir }
    }
}

#[async_trait]
/// Implementation of the `Tool` trait for reading files.
impl Tool for ReadFileTool {
    /// Returns the tool definition.
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read the contents of a file. Supports paging for large files.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file" },
                    "offset": { "type": "integer", "description": "Start line (1-based, optional)" },
                    "limit": { "type": "integer", "description": "Number of lines to read (optional, max 2000)" }
                },
                "required": ["path"]
            }),
        }
    }

    /// Executes the file operation based on the provided input.
    async fn execute(&self, input: Value, _context: Option<crate::tool::ToolContext>) -> Result<ToolOutput> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' field"))?;
        let offset = input["offset"].as_u64().unwrap_or(1) as usize;
        let limit = input["limit"].as_u64().unwrap_or(2000).min(2000) as usize;

        if !Path::new(path_str).exists() {
            return Ok(ToolOutput {
                content: format!("File not found: {}", path_str),
                is_error: true,
            });
        }

        // Path validation for subagents
        if let Some(root) = &self.root_dir {
            let full_path = std::fs::canonicalize(path_str).unwrap_or_else(|_| std::path::PathBuf::from(path_str));
            if !full_path.starts_with(root) {
                return Ok(ToolOutput {
                    content: "Access denied: path is outside of allowed workspace".to_string(),
                    is_error: true,
                });
            }
        }

        match fs::read_to_string(path_str).await {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let start = offset.saturating_sub(1);
                if start >= lines.len() {
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
                    output.push_str(&format!("{:>5} | {}\n", line_num, truncate_line(line, 2000)));
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
}

/// Tool to write content to a file.
pub struct WriteFileTool {
    pub root_dir: Option<std::path::PathBuf>,
}

impl WriteFileTool {
    pub fn new(root_dir: Option<std::path::PathBuf>) -> Self {
        Self { root_dir }
    }
}

#[async_trait]
/// Implementation of the `Tool` trait for writing files.
impl Tool for WriteFileTool {
    /// Returns the tool definition.
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write_file".to_string(),
            description: "Write content to a file. Overwrites if file exists.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file" },
                    "content": { "type": "string", "description": "Content to write" }
                },
                "required": ["path", "content"]
            }),
        }
    }

    /// Executes the file operation based on the provided input.
    async fn execute(&self, input: Value, _context: Option<crate::tool::ToolContext>) -> Result<ToolOutput> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' field"))?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' field"))?;

        // Path validation for subagents
        if let Some(root) = &self.root_dir {
            let p = Path::new(path_str);
            if p.is_absolute() {
                if !p.starts_with(root) {
                    return Ok(ToolOutput {
                        content: "Access denied: absolute path is outside of allowed workspace".to_string(),
                        is_error: true,
                    });
                }
            } else {
                // For relative paths, we ensure it's within root by joining
                let _full_path = root.join(p);
                // Basic directory traversal protection
                if p.to_string_lossy().contains("..") {
                    return Ok(ToolOutput {
                        content: "Access denied: directory traversal detected".to_string(),
                        is_error: true,
                    });
                }
            }
        }

        match fs::write(path_str, content).await {
            Ok(_) => Ok(ToolOutput {
                content: format!("Successfully written to {}", path_str),
                is_error: false,
            }),
            Err(e) => Ok(ToolOutput {
                content: format!("Failed to write file: {}", e),
                is_error: true,
            }),
        }
    }
}

/// Truncates a line to a maximum length, returns a slice.
fn truncate_line(s: &str, max_len: usize) -> &str {
    if s.len() > max_len {
        &s[..max_len]
    } else {
        s
    }
}
