use anyhow::Result;
use serde_json::Value;

pub mod builtin;

/// Definition of a tool, including name, description, and input schema.
#[derive(Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Result produced by a tool execution.
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}

#[async_trait::async_trait]
/// Trait representing a callable tool.
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, input: Value) -> Result<ToolOutput>;
}

/// Registry for storing and accessing tools.
pub struct ToolRegistry {
    pub tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    /// Creates a new empty `ToolRegistry`.
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }
    /// Registers a single tool.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }
    /// Registers multiple tools at once.
    pub fn register_many(&mut self, tools: Vec<Box<dyn Tool>>) {
        self.tools.extend(tools);
    }
    /// Returns the definitions of all registered tools.
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
    /// Executes a tool by name with the given input.
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

/// Provides a default empty `ToolRegistry`.
impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
