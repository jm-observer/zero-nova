// src/tool/mcp.rs
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use crate::mcp::{McpClient, McpToolDef};
use crate::tool::{Tool, ToolDefinition, ToolOutput};

/// Wrapper that adapts an MCP tool into the local `Tool` trait.
pub struct McpTool {
    client: Arc<McpClient>,
    def: ToolDefinition,
}

#[async_trait]
impl Tool for McpTool {
    fn definition(&self) -> ToolDefinition {
        self.def.clone()
    }

    async fn execute(&self, input: Value) -> Result<ToolOutput> {
        let result = self.client.call_tool(&self.def.name, input).await?;
        // Concatenate all text content parts (ignore images/resources for now)
        let text = result
            .content
            .into_iter()
            .filter_map(|c| match c {
                crate::mcp::types::McpContent::Text { text } => Some(text),
                _ => None,
            })
            .collect::<Vec<String>>()
            .join("\n");
        Ok(ToolOutput {
            content: text,
            is_error: result.is_error,
        })
    }
}

/// Bridge that turns a live `McpClient` into a vector of `Tool` objects.
pub struct McpToolBridge;

impl McpToolBridge {
    pub async fn from_client(client: Arc<McpClient>) -> Result<Vec<Box<dyn Tool>>> {
        let tool_defs: Vec<McpToolDef> = client.list_tools().await?;
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();
        for mcp_def in tool_defs {
            let def = ToolDefinition {
                name: mcp_def.name.clone(),
                description: mcp_def.description.unwrap_or_default(),
                input_schema: mcp_def.input_schema.clone(),
            };
            let tool = McpTool {
                client: Arc::clone(&client),
                def,
            };
            tools.push(Box::new(tool) as Box<dyn Tool>);
        }
        Ok(tools)
    }
}
