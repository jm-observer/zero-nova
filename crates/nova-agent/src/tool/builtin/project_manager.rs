use crate::tool::{ProjectDirService, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

pub struct ProjectManagerTool {
    project_dir_service: Arc<dyn ProjectDirService>,
}

impl ProjectManagerTool {
    pub fn new(project_dir_service: Arc<dyn ProjectDirService>) -> Self {
        Self { project_dir_service }
    }
}

#[async_trait]
impl Tool for ProjectManagerTool {
    fn definition(&self) -> crate::tool::ToolDefinition {
        crate::tool::ToolDefinition {
            name: "ProjectManager".to_string(),
            description:
                "Gets or changes the current session project directory. Use this when the user asks to switch the project or working directory."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["get", "set"],
                        "description": "The action to perform on the project directory."
                    },
                    "path": {
                        "type": "string",
                        "description": "The new project directory path. Required when action is 'set'."
                    }
                },
                "required": ["action"]
            }),
            defer_loading: false,
        }
    }

    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        let ctx = context.ok_or_else(|| anyhow::anyhow!("Missing tool context"))?;
        let session_id = &ctx.session_id;

        let action = input["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'action'"))?;

        match action {
            "get" => match self.project_dir_service.get_project_dir(session_id).await {
                Ok(Some(path)) => Ok(ToolOutput {
                    content: format!(
                        "Current project directory: {}\nDirectory exists: {}",
                        path.display(),
                        if path.exists() { "yes" } else { "no" }
                    ),
                    is_error: false,
                }),
                Ok(None) => Ok(ToolOutput {
                    content: "Project directory: not set (using process working directory as fallback)".to_string(),
                    is_error: false,
                }),
                Err(e) => Ok(ToolOutput {
                    content: format!("Failed to get project directory: {}", e),
                    is_error: true,
                }),
            },
            "set" => {
                let path_str = input["path"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'path' for set action"))?;
                let path = PathBuf::from(path_str);

                if !path.exists() {
                    return Ok(ToolOutput {
                        content: format!(
                            "Failed to set project directory: path '{}' does not exist",
                            path.display()
                        ),
                        is_error: true,
                    });
                }
                if !path.is_dir() {
                    return Ok(ToolOutput {
                        content: format!(
                            "Failed to set project directory: path '{}' is not a directory",
                            path.display()
                        ),
                        is_error: true,
                    });
                }

                match self.project_dir_service.set_project_dir(session_id, path).await {
                    Ok(new_path) => {
                        if let Some(shared_env) = ctx.shared_environment.as_ref() {
                            let mut env = shared_env.write().await;
                            env.project_dir = Some(new_path.to_string_lossy().to_string());
                        }

                        let exists = new_path.exists();
                        let mut content = format!(
                            "Project directory changed to: {}\nDirectory exists: {}\nAffected tools: Bash (CWD), Read/Write/Edit (relative path base)",
                            new_path.display(),
                            if exists { "yes" } else { "NO - commands may fail" }
                        );
                        if !exists {
                            content.push_str("\nWarning: The specified directory does not exist on disk.");
                        }

                        Ok(ToolOutput {
                            content,
                            is_error: false,
                        })
                    }
                    Err(e) => Ok(ToolOutput {
                        content: format!("Failed to set project directory: {}", e),
                        is_error: true,
                    }),
                }
            }
            _ => Ok(ToolOutput {
                content: format!("Unknown action: {}", action),
                is_error: true,
            }),
        }
    }
}
