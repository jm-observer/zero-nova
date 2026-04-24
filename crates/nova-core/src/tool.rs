use anyhow::Result;
use log::error;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::mpsc;

use crate::skill::CapabilityPolicy;

use serde_json::Value;

pub mod builtin;

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
    pub category: DeferredToolCategory,
}

/// 延迟工具类别（用于 ToolSearch 过滤）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeferredToolCategory {
    Task,
    Skill,
    Search,
    System,
}

impl std::fmt::Display for DeferredToolCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Task => write!(f, "task"),
            Self::Skill => write!(f, "skill"),
            Self::Search => write!(f, "search"),
            Self::System => write!(f, "system"),
        }
    }
}

impl DeferredToolEntry {
    pub fn to_representation(&self) -> DeferredToolRepresentation {
        DeferredToolRepresentation {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
            category: self.category.clone(),
        }
    }
}

/// 延迟工具的完整表示（包含类别和匹配原因）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeferredToolRepresentation {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub category: DeferredToolCategory,
}

/// TurnToolView 表示当前轮次对 LLM 可见的工具视图。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnToolView {
    pub loaded: Vec<crate::provider::types::ToolDefinition>,
    pub deferred: Vec<DeferredToolRepresentation>,
    pub tool_search_enabled: bool,
    pub skill_tool_enabled: bool,
    pub task_tools_enabled: bool,
}

/// 子代理工具子集描述。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tiny {
    pub agent_id: String,
    pub max_tools: usize,
    pub allowed_tools: Vec<String>,
}

impl TurnToolView {
    /// 根据 agent 规格计算子代理的工具子集。
    pub fn get_agent_tool_subset(&self, policy: &CapabilityPolicy) -> Tiny {
        let allowed_tools: Vec<String> =
            if policy.tool_search_enabled || policy.skill_tool_enabled || policy.task_tools_enabled {
                let mut tool_names: Vec<String> = self.loaded.iter().map(|t| t.name.clone()).collect();
                for def in &self.deferred {
                    let category_match = match policy.task_tools_enabled {
                        true => true,
                        false => !matches!(def.category, DeferredToolCategory::Task),
                    };
                    if category_match {
                        tool_names.push(def.name.clone());
                    }
                }
                tool_names
            } else {
                self.loaded.iter().map(|t| t.name.clone()).collect()
            };

        Tiny {
            agent_id: String::new(),
            max_tools: policy.always_enabled_tools.len() + policy.deferred_tools.len(),
            allowed_tools,
        }
    }
}

impl ToolRegistry {
    /// Creates a new empty `ToolRegistry`.
    pub fn new() -> Self {
        Self {
            tools: Mutex::new(Vec::new()),
            deferred: Mutex::new(Vec::new()),
        }
    }

    /// Acquires the tools lock, recovering from poison errors.
    fn lock_tools(&self) -> MutexGuard<'_, Vec<Arc<dyn Tool>>> {
        self.tools.lock().unwrap_or_else(|poisoned| {
            error!("Tool registry tools lock was poisoned, recovering");
            poisoned.into_inner()
        })
    }

    /// Acquires the deferred lock, recovering from poison errors.
    fn lock_deferred(&self) -> MutexGuard<'_, Vec<DeferredToolEntry>> {
        self.deferred.lock().unwrap_or_else(|poisoned| {
            error!("Tool registry deferred lock was poisoned, recovering");
            poisoned.into_inner()
        })
    }

    /// Registers a single tool.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.lock_tools().push(Arc::from(tool));
    }
    /// Registers multiple tools at once.
    pub fn register_many(&mut self, tools: Vec<Box<dyn Tool>>) {
        let mut guard = self.lock_tools();
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
        self.register_deferred_with_category(name, description, input_schema, factory, DeferredToolCategory::System);
    }

    /// Registers a deferred tool with a specific category.
    pub fn register_deferred_with_category(
        &mut self,
        name: String,
        description: String,
        input_schema: Value,
        factory: Box<dyn Fn() -> Arc<dyn Tool> + Send + Sync>,
        category: DeferredToolCategory,
    ) {
        self.lock_deferred().push(DeferredToolEntry {
            name,
            description,
            input_schema,
            factory,
            category,
        });
    }
    /// Returns the definitions of all registered tools, including deferred ones as stubs.
    pub fn tool_definitions(&self) -> Vec<crate::provider::types::ToolDefinition> {
        let mut defs: Vec<_> = self
            .lock_tools()
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

        if !self.lock_deferred().is_empty() {
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
        self.lock_tools().iter().map(|tool| tool.definition()).collect()
    }

    pub fn deferred_definitions(&self) -> Vec<ToolDefinition> {
        self.lock_deferred()
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
        self.lock_tools().iter().any(|tool| tool.definition().name == name)
    }

    /// Resolves a deferred tool by name, loading it into the active tools list.
    pub fn resolve_deferred(&self, name: &str) -> bool {
        let entry = {
            let mut deferred = self.lock_deferred();
            deferred
                .iter()
                .position(|d| d.name == name)
                .map(|pos| deferred.remove(pos))
        };

        if let Some(entry) = entry {
            let tool = (entry.factory)();
            self.lock_tools().push(tool);
            return true;
        }
        false
    }

    /// 获取当前轮次的工具视图（TurnToolView）。
    ///
    /// 对 LLM 可见的工具包括：
    /// - 已加载的 loaded 工具
    /// - 根据 capability_policy 过滤后的 deferred 工具
    pub fn get_turn_view(
        &self,
        tool_search_enabled: bool,
        skill_tool_enabled: bool,
        task_tools_enabled: bool,
    ) -> TurnToolView {
        let loaded: Vec<_> = self
            .lock_tools()
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

        let mut deferred: Vec<_> = self
            .lock_deferred()
            .iter()
            .filter(|entry| {
                // 如果 task_tools_enabled=false，过滤掉 Task 类别的 deferred 工具
                if !task_tools_enabled && matches!(entry.category, DeferredToolCategory::Task) {
                    return false;
                }
                true
            })
            .map(|e| e.to_representation())
            .collect();

        if tool_search_enabled {
            // 添加 ToolSearch 本身作为 deferred 入口
            let search_entry = DeferredToolEntry {
                name: builtin::tool_search::TOOL_NAME.to_string(),
                description: "Search deferred tools and load their schemas on demand.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query or 'select:ToolName1,ToolName2' to load specific tools" },
                        "max_results": { "type": "integer", "default": 5 }
                    },
                    "required": ["query"]
                }),
                factory: Box::new(|| Arc::new(builtin::tool_search::ToolSearchTool {})),
                category: DeferredToolCategory::Search,
            };
            deferred.push(search_entry.to_representation());
        }

        TurnToolView {
            loaded,
            deferred,
            tool_search_enabled,
            skill_tool_enabled,
            task_tools_enabled,
        }
    }

    /// 根据 capability_policy 过滤 deferred 工具列表。
    pub fn filter_deferred_by_policy(&self, policy: &CapabilityPolicy) -> Vec<DeferredToolRepresentation> {
        self.lock_deferred()
            .iter()
            .filter(|entry| {
                // 根据 policy 中的 deferred_tools 和白名单过滤
                if policy.deferred_tools.is_empty() {
                    return true;
                }
                policy.deferred_tools.contains(&entry.name)
            })
            .map(|e| e.to_representation())
            .collect()
    }

    /// 获取指定类别的 deferred 工具（用于 ToolSearch 类别过滤）。
    pub fn deferred_tools_by_category(&self, category: &DeferredToolCategory) -> Vec<DeferredToolRepresentation> {
        self.lock_deferred()
            .iter()
            .filter(|e| &e.category == category)
            .map(|e| e.to_representation())
            .collect()
    }

    /// 根据类别设置动态加载 deferred 工具。
    pub fn load_deferred_by_category(&self, category: &DeferredToolCategory, enabled: bool) {
        if !enabled {
            return;
        }
        let entries: Vec<_> = self
            .lock_deferred()
            .iter()
            .filter(|e| &e.category == category)
            .map(|e| e.name.clone())
            .collect();
        for name in entries {
            self.resolve_deferred(&name);
        }
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
            "web_fetch" => "WebFetch",
            "web_search" => "WebSearch",
            other => other,
        };

        let tool = self
            .lock_tools()
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
