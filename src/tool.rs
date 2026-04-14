use anyhow::Result;
use serde_json::Value;

pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub struct ToolOutput {
    pub output: String,
    pub is_error: bool,
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, input: Value) -> Result<ToolOutput>;
}

pub struct ToolRegistry {
    pub tools: Vec<Box<dyn Tool>>, // public for with_tools access
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }
    pub fn tool_definitions(&self) -> Vec<crate::provider::types::ToolDefinition> {
        self.tools
            .iter()
            .map(|t| {
                let d = t.definition();
                crate::provider::types::ToolDefinition {
                    name: d.name,
                    description: d.description,
                    input_schema: d.input_schema,
                }
            })
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
