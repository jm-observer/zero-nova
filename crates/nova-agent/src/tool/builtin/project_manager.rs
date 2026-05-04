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
            description: "Manages the current session's project directory. Supports getting and setting the directory."
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
                    content: format!("Current project directory: {}", path.display()),
                    is_error: false,
                }),
                Ok(None) => Ok(ToolOutput {
                    content: "Current project directory: (not set)".to_string(),
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
                match self.project_dir_service.set_project_dir(session_id, path).await {
                    Ok(new_path) => Ok(ToolOutput {
                        content: format!("Project directory updated to: {}", new_path.display()),
                        is_error: false,
                    }),
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
