use crate::tool::{Tool, ToolDefinition, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::Path;
use tokio::fs;

pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
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

    async fn execute(&self, input: Value) -> Result<ToolOutput> {
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

pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
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

    async fn execute(&self, input: Value) -> Result<ToolOutput> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' field"))?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' field"))?;

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

fn truncate_line(s: &str, max_len: usize) -> &str {
    if s.len() > max_len { &s[..max_len] } else { s }
}
