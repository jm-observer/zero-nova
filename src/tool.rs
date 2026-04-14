use anyhow::Result;
use serde_json::Value;

pub mod builtin;

pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, input: Value) -> Result<ToolOutput>;
}

pub struct ToolRegistry {
    pub tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }
    pub fn register_many(&mut self, tools: Vec<Box<dyn Tool>>) {
        self.tools.extend(tools);
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
    pub async fn execute(&self, name: &str, input: serde_json::Value) -> anyhow::Result<ToolOutput> {
        for tool in &self.tools {
            if tool.definition().name == name {
                return tool.execute(input).await;
            }
        }
        Ok(ToolOutput {
            content: format!("Tool '{}' not found", name),
            is_error: true,
        })
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
