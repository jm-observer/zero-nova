use crate::event::AgentEvent;
use crate::prompt::EnvironmentSnapshot;
use crate::provider::types::ToolDefinition as ProviderToolDefinition;
use crate::skill::{CapabilityPolicy, SkillRegistry};
use crate::tool::{builtin, read_cache};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use serde_json::Value;
use std::path::PathBuf;

/// Context for tool execution, providing access to event channels and other runtime info.
#[derive(Clone)]
pub struct ToolContext {
    /// Channel for sending intermediate events (e.g., logs).
    pub event_tx: mpsc::Sender<AgentEvent>,
    /// The tool_use_id to associate LogDelta events with.
    pub tool_use_id: String,
    /// The session_id associated with this tool execution.
    pub session_id: String,
    /// Reference to the task store for TaskCreate/TaskList/TaskUpdate.
    pub task_store: Option<Arc<Mutex<builtin::task::TaskStore>>>,
    /// Reference to the skill registry.
    pub skill_registry: Option<Arc<SkillRegistry>>,
    /// Session-level state: files that have been read (for Write pre-read enforcement).
    pub read_files: Arc<Mutex<HashSet<String>>>,
    /// Turn-level read history (for duplicate-read convergence).
    pub turn_read_state: Option<Arc<RwLock<read_cache::TurnReadState>>>,
    /// 运行时环境快照
    pub environment: Option<EnvironmentSnapshot>,
    /// 同一 turn 内共享的可变环境快照（用于实时同步 project_dir 等变更）。
    pub shared_environment: Option<Arc<RwLock<EnvironmentSnapshot>>>,
    /// Cancellation token for cooperative cancellation of long-running tools (e.g., orchestration).
    pub cancellation_token: Option<CancellationToken>,
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
pub trait ProjectDirService: Send + Sync {
    async fn get_project_dir(&self, session_id: &str) -> Result<Option<PathBuf>>;
    async fn set_project_dir(&self, session_id: &str, project_dir: PathBuf) -> Result<PathBuf>;
}

pub struct UnavailableProjectDirService {
    reason: &'static str,
}

impl UnavailableProjectDirService {
    pub fn new(reason: &'static str) -> Self {
        Self { reason }
    }
}

#[async_trait::async_trait]
impl ProjectDirService for UnavailableProjectDirService {
    async fn get_project_dir(&self, _session_id: &str) -> Result<Option<PathBuf>> {
        anyhow::bail!("{}", self.reason)
    }

    async fn set_project_dir(&self, _session_id: &str, _project_dir: PathBuf) -> Result<PathBuf> {
        anyhow::bail!("{}", self.reason)
    }
}

#[async_trait::async_trait]
/// Trait representing a callable tool.
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    /// Executes the tool.
    async fn execute(&self, input: Value, _context: Option<ToolContext>) -> Result<ToolOutput>;
}

/// Registry for storing and accessing tools.
///
/// Uses `tokio::sync::Mutex` so that tool registration and resolution can occur
/// in async contexts without blocking child tasks.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeferredToolRepresentation {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub category: DeferredToolCategory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnToolView {
    pub loaded: Vec<ProviderToolDefinition>,
    pub deferred: Vec<DeferredToolRepresentation>,
    pub tool_search_enabled: bool,
    pub skill_tool_enabled: bool,
    pub task_tools_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tiny {
    pub agent_id: String,
    pub max_tools: usize,
    pub allowed_tools: Vec<String>,
}

impl TurnToolView {
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

    /// Acquires the tools lock using `try_lock` to avoid blocking the async runtime.
    /// The tool registry is a short-lived hot path; collision is rare (<1ms hold time).
    fn lock_tools(&self) -> tokio::sync::MutexGuard<'_, Vec<Arc<dyn Tool>>> {
        self.tools
            .try_lock()
            .expect("Tool registry tools lock should not be contended")
    }

    /// Acquires the deferred lock using `try_lock` for the same reason as `lock_tools`.
    fn lock_deferred(&self) -> tokio::sync::MutexGuard<'_, Vec<DeferredToolEntry>> {
        self.deferred
            .try_lock()
            .expect("Tool registry deferred lock should not be contended")
    }

    /// Registers a single tool.
    pub fn register(&self, tool: Box<dyn Tool>) {
        self.lock_tools().push(Arc::from(tool));
    }
    /// Registers multiple tools at once.
    pub fn register_many(&self, tools: Vec<Box<dyn Tool>>) {
        let mut guard = self.lock_tools();
        for tool in tools {
            guard.push(Arc::from(tool));
        }
    }
    /// Registers a deferred tool.
    pub fn register_deferred(
        &self,
        name: String,
        description: String,
        input_schema: Value,
        factory: Box<dyn Fn() -> Arc<dyn Tool> + Send + Sync>,
    ) {
        self.register_deferred_with_category(name, description, input_schema, factory, DeferredToolCategory::System);
    }

    /// Registers a deferred tool with a specific category.
    pub fn register_deferred_with_category(
        &self,
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
    pub fn tool_definitions(&self) -> Vec<ProviderToolDefinition> {
        let mut defs: Vec<_> = self
            .lock_tools()
            .iter()
            .map(|t| {
                let d = t.definition();
                ProviderToolDefinition {
                    name: d.name,
                    description: d.description,
                    input_schema: d.input_schema,
                }
            })
            .collect();

        if !self.lock_deferred().is_empty() {
            let d = builtin::tool_search::tool_definition();
            defs.push(ProviderToolDefinition {
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

    /// 鑾峰彇褰撳墠杞鐨勫伐鍏疯鍥撅紙TurnToolView锛夈€?    ///
    /// 瀵?LLM 鍙鐨勫伐鍏峰寘鎷細
    /// - 宸插姞杞界殑 loaded 宸ュ叿
    /// - 鏍规嵁 capability_policy 杩囨护鍚庣殑 deferred 宸ュ叿
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
                ProviderToolDefinition {
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
                // 濡傛灉 task_tools_enabled=false锛岃繃婊ゆ帀 Task 绫诲埆鐨?deferred 宸ュ叿
                if !task_tools_enabled && matches!(entry.category, DeferredToolCategory::Task) {
                    return false;
                }
                true
            })
            .map(|e| e.to_representation())
            .collect();

        if tool_search_enabled {
            // 娣诲姞 ToolSearch 鏈韩浣滀负 deferred 鍏ュ彛
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

    pub fn filter_deferred_by_policy(&self, policy: &CapabilityPolicy) -> Vec<DeferredToolRepresentation> {
        self.lock_deferred()
            .iter()
            .filter(|entry| {
                // 鏍规嵁 policy 涓殑 deferred_tools 鍜岀櫧鍚嶅崟杩囨护
                if policy.deferred_tools.is_empty() {
                    return true;
                }
                policy.deferred_tools.contains(&entry.name)
            })
            .map(|e| e.to_representation())
            .collect()
    }

    pub fn deferred_tools_by_category(&self, category: &DeferredToolCategory) -> Vec<DeferredToolRepresentation> {
        self.lock_deferred()
            .iter()
            .filter(|e| &e.category == category)
            .map(|e| e.to_representation())
            .collect()
    }

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
        mut input: serde_json::Value,
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

        if matches!(canonical_name, "Read" | "Write" | "Edit") {
            if let Err(error_output) =
                crate::tool::path_preprocess::preprocess_file_tool_input(canonical_name, &mut input, context.as_ref())
            {
                return Ok(error_output);
            }
        }

        let tool = self
            .lock_tools()
            .iter()
            .find(|tool| tool.definition().name == canonical_name)
            .cloned();

        if let Some(tool) = tool {
            let definition = tool.definition();
            if let Err(error_output) = crate::tool::schema_validation::validate_input_against_schema(
                canonical_name,
                &input,
                &definition.input_schema,
            ) {
                return Ok(error_output);
            }
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
    use crate::prompt::EnvironmentSnapshot;
    use anyhow::Result;
    use serde_json::{json, Value};
    use std::collections::HashSet;
    use std::sync::Arc;
    use tokio::sync::{mpsc, Mutex};

    struct StaticTool {
        name: &'static str,
    }

    struct SchemaTool {
        name: &'static str,
        schema: Value,
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

    #[async_trait::async_trait]
    impl Tool for SchemaTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: self.name.to_string(),
                description: format!("{} description", self.name),
                input_schema: self.schema.clone(),
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
        let registry = ToolRegistry::new();
        registry.register(Box::new(StaticTool { name: "Bash" }));

        let output = registry.execute("bash", json!({}), None).await.unwrap();
        assert_eq!(output.content, "Bash");
    }

    #[tokio::test]
    async fn tool_search_can_load_deferred_tool() {
        let registry = ToolRegistry::new();
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

    #[tokio::test]
    async fn execute_rejects_unknown_fields_by_schema() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(SchemaTool {
            name: "SchemaRead",
            schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string" }
                },
                "required": ["file_path"]
            }),
        }));

        let output = registry
            .execute(
                "SchemaRead",
                json!({
                    "file_path": "src/lib.rs",
                    "unknown": true
                }),
                None,
            )
            .await
            .unwrap();

        assert!(output.is_error);
        assert!(output.content.contains("unknown field 'unknown'"));
    }

    #[tokio::test]
    async fn execute_rejects_missing_required_fields_by_schema() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(SchemaTool {
            name: "SchemaWrite",
            schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string" }
                },
                "required": ["file_path"]
            }),
        }));

        let output = registry.execute("SchemaWrite", json!({}), None).await.unwrap();

        assert!(output.is_error);
        assert!(output.content.contains("missing required field"));
    }

    #[tokio::test]
    async fn execute_rejects_type_mismatch_by_schema() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(SchemaTool {
            name: "SchemaBash",
            schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "timeout_ms": { "type": "integer" }
                },
                "required": ["command"]
            }),
        }));

        let output = registry
            .execute(
                "SchemaBash",
                json!({
                    "command": "echo ok",
                    "timeout_ms": "1000"
                }),
                None,
            )
            .await
            .unwrap();

        assert!(output.is_error);
        assert!(output.content.contains("field 'timeout_ms' must be type 'integer'"));
    }

    #[tokio::test]
    async fn relative_file_tool_path_requires_project_dir_when_session_has_none() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(StaticTool { name: "Read" }));
        let (event_tx, _event_rx) = mpsc::channel(1);
        let output = registry
            .execute(
                "Read",
                json!({"file_path":"src/lib.rs"}),
                Some(ToolContext {
                    event_tx,
                    tool_use_id: "tool-1".to_string(),
                    session_id: "session-1".to_string(),
                    task_store: None,
                    skill_registry: None,
                    read_files: Arc::new(Mutex::new(HashSet::new())),
                    turn_read_state: None,
                    environment: Some(EnvironmentSnapshot {
                        config_dir: "D:/config".to_string(),
                        project_dir: None,
                        platform: "windows".to_string(),
                        shell: "powershell".to_string(),
                        git_branch: None,
                        git_status_summary: None,
                        recent_commits: None,
                        model_id: None,
                        current_date: "2026-05-04".to_string(),
                    }),
                    shared_environment: None,
                    cancellation_token: None,
                }),
            )
            .await
            .unwrap();

        assert!(output.is_error);
        assert_eq!(
            output.content,
            crate::tool::path_preprocess::NO_PROJECT_RELATIVE_PATH_ERROR
        );
    }
}
