use anyhow::Result;
use serde_json::Value;
use tokio::sync::mpsc;

pub mod builtin;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// Context for tool execution, providing access to event channels and other runtime info.
#[derive(Clone)]
pub struct ToolContext {
    /// Channel for sending intermediate events (e.g., logs).
    pub event_tx: mpsc::Sender<crate::event::AgentEvent>,
    /// The tool_use_id to associate LogDelta events with.
    pub tool_use_id: String,
    /// Reference to the task store for TaskCreate/TaskList/TaskUpdate.
    pub task_store: Option<Arc<tokio::sync::Mutex<builtin::task::TaskStore>>>,
    /// Reference to the skill registry.
    pub skill_registry: Option<Arc<crate::skill::SkillRegistry>>,
    /// Session-level state: files that have been read (for Write pre-read enforcement).
    pub read_files: Arc<tokio::sync::Mutex<HashSet<String>>>,
}

/// Definition of a tool, including name, description, and input schema.
#[derive(Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    /// If true, the tool schema is deferred and must be fetched via ToolSearch.
    pub defer_loading: bool,
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
    /// Executes the tool.
    async fn execute(&self, input: Value, _context: Option<ToolContext>) -> Result<ToolOutput>;
}

/// Registry for storing and accessing tools.
pub struct ToolRegistry {
    tools: Mutex<Vec<Arc<dyn Tool>>>,
    deferred: Mutex<Vec<DeferredToolEntry>>,
}

pub struct DeferredToolEntry {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub factory: Box<dyn Fn() -> Arc<dyn Tool> + Send + Sync>,
}

impl ToolRegistry {
    /// Creates a new empty `ToolRegistry`.
    pub fn new() -> Self {
        Self {
            tools: Mutex::new(Vec::new()),
            deferred: Mutex::new(Vec::new()),
        }
    }
    /// Registers a single tool.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools
            .lock()
            .expect("tool registry lock poisoned")
            .push(Arc::from(tool));
    }
    /// Registers multiple tools at once.
    pub fn register_many(&mut self, tools: Vec<Box<dyn Tool>>) {
        let mut guard = self.tools.lock().expect("tool registry lock poisoned");
        for tool in tools {
            guard.push(Arc::from(tool));
        }
    }
    /// Registers a deferred tool.
    pub fn register_deferred(
        &mut self,
        name: String,
        description: String,
        input_schema: Value,
        factory: Box<dyn Fn() -> Arc<dyn Tool> + Send + Sync>,
    ) {
        self.deferred
            .lock()
            .expect("tool registry lock poisoned")
            .push(DeferredToolEntry {
                name,
                description,
                input_schema,
                factory,
            });
    }
    /// Returns the definitions of all registered tools, including deferred ones as stubs.
    pub fn tool_definitions(&self) -> Vec<crate::provider::types::ToolDefinition> {
        let mut defs: Vec<_> = self
            .tools
            .lock()
            .expect("tool registry lock poisoned")
            .iter()
            .map(|t| {
                let d = t.definition();
                crate::provider::types::ToolDefinition {
                    name: d.name,
                    description: d.description,
                    input_schema: d.input_schema,
                }
            })
            .collect();

        if !self.deferred.lock().expect("tool registry lock poisoned").is_empty() {
            let d = builtin::tool_search::tool_definition();
            defs.push(crate::provider::types::ToolDefinition {
                name: d.name,
                description: d.description,
                input_schema: d.input_schema,
            });
        }

        defs
    }

    pub fn loaded_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .lock()
            .expect("tool registry lock poisoned")
            .iter()
            .map(|tool| tool.definition())
            .collect()
    }

    pub fn deferred_definitions(&self) -> Vec<ToolDefinition> {
        self.deferred
            .lock()
            .expect("tool registry lock poisoned")
            .iter()
            .map(|entry| ToolDefinition {
                name: entry.name.clone(),
                description: entry.description.clone(),
                input_schema: entry.input_schema.clone(),
                defer_loading: true,
            })
            .collect()
    }

    pub fn has_loaded_tool(&self, name: &str) -> bool {
        self.tools
            .lock()
            .expect("tool registry lock poisoned")
            .iter()
            .any(|tool| tool.definition().name == name)
    }

    /// Resolves a deferred tool by name, loading it into the active tools list.
    pub fn resolve_deferred(&self, name: &str) -> bool {
        let entry = {
            let mut deferred = self.deferred.lock().expect("tool registry lock poisoned");
            deferred
                .iter()
                .position(|d| d.name == name)
                .map(|pos| deferred.remove(pos))
        };

        if let Some(entry) = entry {
            let tool = (entry.factory)();
            self.tools.lock().expect("tool registry lock poisoned").push(tool);
            return true;
        }
        false
    }
    /// Executes a tool by name with the given input and context.
    pub async fn execute(
        &self,
        name: &str,
        input: serde_json::Value,
        context: Option<ToolContext>,
    ) -> anyhow::Result<ToolOutput> {
        if name == builtin::tool_search::TOOL_NAME {
            return builtin::tool_search::execute(self, input).await;
        }

        let canonical_name = match name {
            "bash" => "Bash",
            "read_file" => "Read",
            "write_file" => "Write",
            "spawn_subagent" => "Agent",
            other => other,
        };

        let tool = self
            .tools
            .lock()
            .expect("tool registry lock poisoned")
            .iter()
            .find(|tool| tool.definition().name == canonical_name)
            .cloned();

        if let Some(tool) = tool {
            return tool.execute(input, context).await;
        }

        Ok(ToolOutput {
            content: format!("Tool '{}' not found", canonical_name),
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

#[cfg(test)]
mod tests {
    use super::{Tool, ToolContext, ToolDefinition, ToolOutput, ToolRegistry};
    use anyhow::Result;
    use serde_json::json;
    use std::sync::Arc;

    struct StaticTool {
        name: &'static str,
    }

    #[async_trait::async_trait]
    impl Tool for StaticTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: self.name.to_string(),
                description: format!("{} description", self.name),
                input_schema: json!({"type": "object"}),
                defer_loading: false,
            }
        }

        async fn execute(&self, _input: serde_json::Value, _context: Option<ToolContext>) -> Result<ToolOutput> {
            Ok(ToolOutput {
                content: self.name.to_string(),
                is_error: false,
            })
        }
    }

    #[tokio::test]
    async fn execute_supports_legacy_tool_names() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(StaticTool { name: "Bash" }));

        let output = registry.execute("bash", json!({}), None).await.unwrap();
        assert_eq!(output.content, "Bash");
    }

    #[tokio::test]
    async fn tool_search_can_load_deferred_tool() {
        let mut registry = ToolRegistry::new();
        registry.register_deferred(
            "DeferredTool".to_string(),
            "Useful deferred tool".to_string(),
            json!({"type": "object"}),
            Box::new(|| Arc::new(StaticTool { name: "DeferredTool" })),
        );

        let search_output = registry
            .execute("ToolSearch", json!({"query": "select:DeferredTool"}), None)
            .await
            .unwrap();
        assert!(search_output.content.contains("Loaded tool: DeferredTool"));
        assert!(registry.has_loaded_tool("DeferredTool"));
    }
}
