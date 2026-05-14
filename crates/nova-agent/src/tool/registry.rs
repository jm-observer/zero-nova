use crate::event::AgentEvent;
use crate::prompt::EnvironmentSnapshot;
use crate::provider::types::ToolDefinition as ProviderToolDefinition;
use crate::skill::{CapabilityPolicy, SkillRegistry};
use crate::tool::{builtin, read_cache};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::{any::Any, panic::AssertUnwindSafe};
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
    /// Session-level task store handle for TaskCreate/TaskList/TaskUpdate.
    pub task_store: Option<builtin::task::TaskStoreHandle>,
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
    /// 当前轮可见的工具名集合，用于 ToolInfo 等查询工具的可见性过滤。
    pub visible_tool_names: Arc<HashSet<String>>,
}

/// Definition of a tool, including name, description, and input schema.
#[derive(Clone)]
pub struct RegisteredToolDefinition {
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
    fn definition(&self) -> RegisteredToolDefinition;
    /// Executes the tool.
    async fn execute(&self, input: Value, _context: Option<ToolContext>) -> Result<ToolOutput>;
}

/// Registry for storing and accessing tools.
///
/// Uses `tokio::sync::Mutex` so that tool registration and resolution can occur
/// in async contexts without blocking child tasks.
pub struct ToolRegistry {
    state: Mutex<RegistryState>,
    snapshot: RwLock<Arc<RegistrySnapshot>>,
}

struct RegistryState {
    tools: Vec<Arc<dyn Tool>>,
    deferred: Vec<DeferredToolEntry>,
}

impl RegistryState {
    fn new() -> Self {
        Self {
            tools: Vec::new(),
            deferred: Vec::new(),
        }
    }
}

#[derive(Clone)]
struct RegistrySnapshot {
    loaded_provider_definitions: Vec<ProviderToolDefinition>,
    loaded_definitions: Vec<RegisteredToolDefinition>,
    deferred_definitions: Vec<RegisteredToolDefinition>,
    deferred_representations: Vec<DeferredToolRepresentation>,
}

impl RegistrySnapshot {
    fn from_state(state: &RegistryState) -> Self {
        let loaded_definitions: Vec<RegisteredToolDefinition> =
            state.tools.iter().map(|tool| tool.definition()).collect();
        let loaded_provider_definitions: Vec<ProviderToolDefinition> = loaded_definitions
            .iter()
            .map(|d| ProviderToolDefinition {
                name: d.name.clone(),
                description: d.description.clone(),
                input_schema: d.input_schema.clone(),
            })
            .collect();
        let deferred_definitions: Vec<RegisteredToolDefinition> = state
            .deferred
            .iter()
            .map(|entry| RegisteredToolDefinition {
                name: entry.name.clone(),
                description: entry.description.clone(),
                input_schema: entry.input_schema.clone(),
                defer_loading: true,
            })
            .collect();
        let deferred_representations: Vec<DeferredToolRepresentation> = state
            .deferred
            .iter()
            .map(DeferredToolEntry::to_representation)
            .collect();

        Self {
            loaded_provider_definitions,
            loaded_definitions,
            deferred_definitions,
            deferred_representations,
        }
    }
}

pub struct DeferredToolEntry {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub factory: Box<dyn Fn() -> Arc<dyn Tool> + Send + Sync>,
    pub category: DeferredToolCategory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeferredResolveOutcome {
    Loaded,
    AlreadyLoaded,
    NotFound,
    FactoryFailed { message: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeferredCategoryLoadOutcome {
    pub requested: usize,
    pub loaded: usize,
    pub already_loaded: usize,
    pub not_found: usize,
    pub failed: usize,
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

/// 工具元信息的统一视图，供 prompt 展示、诊断和查询工具复用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMetadataView {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub loaded: bool,
    pub deferred: bool,
    pub category: Option<DeferredToolCategory>,
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
        let state = RegistryState::new();
        let snapshot = RegistrySnapshot::from_state(&state);
        Self {
            state: Mutex::new(state),
            snapshot: RwLock::new(Arc::new(snapshot)),
        }
    }

    fn lock_state_sync(&self) -> tokio::sync::MutexGuard<'_, RegistryState> {
        loop {
            if let Ok(guard) = self.state.try_lock() {
                return guard;
            }
            std::thread::yield_now();
        }
    }

    async fn lock_state_async(&self) -> tokio::sync::MutexGuard<'_, RegistryState> {
        self.state.lock().await
    }

    fn lock_snapshot_sync(&self) -> tokio::sync::RwLockReadGuard<'_, Arc<RegistrySnapshot>> {
        loop {
            if let Ok(guard) = self.snapshot.try_read() {
                return guard;
            }
            std::thread::yield_now();
        }
    }

    async fn lock_snapshot_async(&self) -> tokio::sync::RwLockReadGuard<'_, Arc<RegistrySnapshot>> {
        self.snapshot.read().await
    }

    fn refresh_snapshot_locked_sync(
        &self,
        state: &RegistryState,
    ) -> tokio::sync::RwLockWriteGuard<'_, Arc<RegistrySnapshot>> {
        let next = Arc::new(RegistrySnapshot::from_state(state));
        loop {
            if let Ok(mut snapshot) = self.snapshot.try_write() {
                *snapshot = next;
                return snapshot;
            }
            std::thread::yield_now();
        }
    }

    async fn refresh_snapshot_locked_async(
        &self,
        state: &RegistryState,
    ) -> tokio::sync::RwLockWriteGuard<'_, Arc<RegistrySnapshot>> {
        let next = Arc::new(RegistrySnapshot::from_state(state));
        let mut snapshot = self.snapshot.write().await;
        *snapshot = next;
        snapshot
    }

    /// Registers a single tool.
    pub fn register(&self, tool: Box<dyn Tool>) {
        let mut state = self.lock_state_sync();
        state.tools.push(Arc::from(tool));
        let _ = self.refresh_snapshot_locked_sync(&state);
    }
    /// Registers multiple tools at once.
    pub fn register_many(&self, tools: Vec<Box<dyn Tool>>) {
        let mut guard = self.lock_state_sync();
        for tool in tools {
            guard.tools.push(Arc::from(tool));
        }
        let _ = self.refresh_snapshot_locked_sync(&guard);
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
        let mut state = self.lock_state_sync();
        state.deferred.push(DeferredToolEntry {
            name,
            description,
            input_schema,
            factory,
            category,
        });
        let _ = self.refresh_snapshot_locked_sync(&state);
    }
    /// Returns the definitions of all registered tools, including deferred ones as stubs.
    pub fn tool_definitions(&self) -> Vec<ProviderToolDefinition> {
        let snapshot = self.lock_snapshot_sync();
        let mut defs = snapshot.loaded_provider_definitions.clone();
        if !snapshot.deferred_representations.is_empty() {
            let d = builtin::tool_search::tool_definition();
            defs.push(ProviderToolDefinition {
                name: d.name,
                description: d.description,
                input_schema: d.input_schema,
            });
        }

        defs
    }

    pub async fn tool_definitions_async(&self) -> Vec<ProviderToolDefinition> {
        let snapshot = self.lock_snapshot_async().await;
        let mut defs = snapshot.loaded_provider_definitions.clone();
        if !snapshot.deferred_representations.is_empty() {
            let d = builtin::tool_search::tool_definition();
            defs.push(ProviderToolDefinition {
                name: d.name,
                description: d.description,
                input_schema: d.input_schema,
            });
        }

        defs
    }

    pub fn loaded_definitions(&self) -> Vec<RegisteredToolDefinition> {
        self.lock_snapshot_sync().loaded_definitions.clone()
    }

    pub async fn loaded_definitions_async(&self) -> Vec<RegisteredToolDefinition> {
        self.lock_snapshot_async().await.loaded_definitions.clone()
    }

    pub fn deferred_definitions_snapshot(&self) -> Vec<RegisteredToolDefinition> {
        self.lock_snapshot_sync().deferred_definitions.clone()
    }

    pub async fn has_loaded_tool(&self, name: &str) -> bool {
        self.lock_state_async()
            .await
            .tools
            .iter()
            .any(|tool| tool.definition().name == name)
    }

    /// Resolves a deferred tool by name, loading it into the active tools list.
    pub async fn resolve_deferred(&self, name: &str) -> bool {
        matches!(
            self.resolve_deferred_with_outcome(name).await,
            DeferredResolveOutcome::Loaded
        )
    }

    pub async fn resolve_deferred_with_outcome(&self, name: &str) -> DeferredResolveOutcome {
        let mut state = self.lock_state_async().await;
        if state.tools.iter().any(|tool| tool.definition().name == name) {
            return DeferredResolveOutcome::AlreadyLoaded;
        }
        let Some(pos) = state.deferred.iter().position(|d| d.name == name) else {
            return DeferredResolveOutcome::NotFound;
        };
        let entry = state.deferred.remove(pos);
        let tool = match std::panic::catch_unwind(AssertUnwindSafe(|| (entry.factory)())) {
            Ok(tool) => tool,
            Err(payload) => {
                let message = panic_payload_to_message(payload);
                state.deferred.push(entry);
                let _ = self.refresh_snapshot_locked_async(&state).await;
                return DeferredResolveOutcome::FactoryFailed { message };
            }
        };
        state.tools.push(tool);
        let _ = self.refresh_snapshot_locked_async(&state).await;
        DeferredResolveOutcome::Loaded
    }

    /// 获取当前轮次的工具视图（`TurnToolView`）。
    ///
    /// 对 LLM 可见的工具包括：
    /// - 已加载的 `loaded` 工具
    /// - 根据 capability policy 过滤后的 `deferred` 工具
    pub fn get_turn_view(
        &self,
        tool_search_enabled: bool,
        skill_tool_enabled: bool,
        task_tools_enabled: bool,
    ) -> TurnToolView {
        let snapshot = self.lock_snapshot_sync();
        let loaded = snapshot.loaded_provider_definitions.clone();
        let mut deferred: Vec<_> = snapshot
            .deferred_representations
            .iter()
            .filter(|entry| {
                // 如果 task_tools_enabled=false，则过滤掉 Task 类别的 deferred 工具。
                if !task_tools_enabled && matches!(entry.category, DeferredToolCategory::Task) {
                    return false;
                }
                true
            })
            .cloned()
            .collect();

        if tool_search_enabled {
            deferred.push(tool_search_representation());
        }

        TurnToolView {
            loaded,
            deferred,
            tool_search_enabled,
            skill_tool_enabled,
            task_tools_enabled,
        }
    }

    pub async fn get_turn_view_async(
        &self,
        tool_search_enabled: bool,
        skill_tool_enabled: bool,
        task_tools_enabled: bool,
    ) -> TurnToolView {
        let snapshot = self.lock_snapshot_async().await;
        let loaded = snapshot.loaded_provider_definitions.clone();
        let mut deferred: Vec<_> = snapshot
            .deferred_representations
            .iter()
            .filter(|entry| {
                if !task_tools_enabled && matches!(entry.category, DeferredToolCategory::Task) {
                    return false;
                }
                true
            })
            .cloned()
            .collect();

        if tool_search_enabled {
            deferred.push(tool_search_representation());
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
        self.lock_snapshot_sync()
            .deferred_representations
            .iter()
            .filter(|entry| {
                // 根据 policy 中的 deferred_tools 白名单进行过滤。
                if policy.deferred_tools.is_empty() {
                    return true;
                }
                policy.deferred_tools.contains(&entry.name)
            })
            .cloned()
            .collect()
    }

    pub fn deferred_tools_by_category(&self, category: &DeferredToolCategory) -> Vec<DeferredToolRepresentation> {
        self.lock_snapshot_sync()
            .deferred_representations
            .iter()
            .filter(|entry| &entry.category == category)
            .cloned()
            .collect()
    }

    pub async fn deferred_definitions(&self) -> Vec<RegisteredToolDefinition> {
        self.lock_snapshot_async().await.deferred_definitions.clone()
    }

    pub async fn load_deferred_by_category(
        &self,
        category: &DeferredToolCategory,
        enabled: bool,
    ) -> DeferredCategoryLoadOutcome {
        if !enabled {
            return DeferredCategoryLoadOutcome::default();
        }
        let entries: Vec<_> = self
            .lock_state_async()
            .await
            .deferred
            .iter()
            .filter(|e| &e.category == category)
            .map(|e| e.name.clone())
            .collect();
        let mut outcome = DeferredCategoryLoadOutcome {
            requested: entries.len(),
            ..DeferredCategoryLoadOutcome::default()
        };
        for name in entries {
            match self.resolve_deferred_with_outcome(&name).await {
                DeferredResolveOutcome::Loaded => outcome.loaded += 1,
                DeferredResolveOutcome::AlreadyLoaded => outcome.already_loaded += 1,
                DeferredResolveOutcome::NotFound => outcome.not_found += 1,
                DeferredResolveOutcome::FactoryFailed { .. } => outcome.failed += 1,
            }
        }
        outcome
    }

    /// 查询单个工具的元信息视图。
    ///
    /// 若同名工具同时存在于 loaded 和 deferred 中，优先返回 loaded 版本。
    pub async fn tool_metadata(&self, name: &str) -> Option<ToolMetadataView> {
        let state = self.lock_state_async().await;

        if let Some(tool) = state.tools.iter().find(|tool| tool.definition().name == name) {
            let def = tool.definition();
            return Some(ToolMetadataView {
                name: def.name.clone(),
                description: def.description.clone(),
                input_schema: def.input_schema.clone(),
                loaded: true,
                deferred: false,
                category: None,
            });
        }

        if let Some(entry) = state.deferred.iter().find(|entry| entry.name == name) {
            return Some(ToolMetadataView {
                name: entry.name.clone(),
                description: entry.description.clone(),
                input_schema: entry.input_schema.clone(),
                loaded: false,
                deferred: true,
                category: Some(entry.category.clone()),
            });
        }

        None
    }

    /// 查询所有已注册工具的元信息视图。
    pub async fn all_tool_metadata(&self) -> Vec<ToolMetadataView> {
        let state = self.lock_state_async().await;
        let mut result: Vec<ToolMetadataView> = state
            .tools
            .iter()
            .map(|tool| {
                let def = tool.definition();
                ToolMetadataView {
                    name: def.name.clone(),
                    description: def.description.clone(),
                    input_schema: def.input_schema.clone(),
                    loaded: true,
                    deferred: false,
                    category: None,
                }
            })
            .collect();

        for entry in &state.deferred {
            if result.iter().any(|v| v.name == entry.name) {
                continue;
            }
            result.push(ToolMetadataView {
                name: entry.name.clone(),
                description: entry.description.clone(),
                input_schema: entry.input_schema.clone(),
                loaded: false,
                deferred: true,
                category: Some(entry.category.clone()),
            });
        }

        result
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

        if name == builtin::tool_info::TOOL_NAME {
            let definition = builtin::tool_info::tool_definition();
            if let Err(error_output) = crate::tool::schema_validation::validate_input_against_schema(
                builtin::tool_info::TOOL_NAME,
                &input,
                &definition.input_schema,
            ) {
                return Ok(error_output);
            }
            return builtin::tool_info::execute(self, input, context.as_ref()).await;
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
            .lock_state_async()
            .await
            .tools
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

fn tool_search_representation() -> DeferredToolRepresentation {
    let definition = builtin::tool_search::tool_definition();
    DeferredToolRepresentation {
        name: definition.name,
        description: definition.description,
        input_schema: definition.input_schema,
        category: DeferredToolCategory::Search,
    }
}

fn panic_payload_to_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "unknown panic payload".to_string()
}

/// Provides a default empty `ToolRegistry`.
impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DeferredResolveOutcome, DeferredToolCategory, RegisteredToolDefinition, Tool, ToolContext, ToolOutput,
        ToolRegistry,
    };
    use crate::prompt::EnvironmentSnapshot;
    use anyhow::Result;
    use serde_json::{json, Value};
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant;
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
        fn definition(&self) -> RegisteredToolDefinition {
            RegisteredToolDefinition {
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
        fn definition(&self) -> RegisteredToolDefinition {
            RegisteredToolDefinition {
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
        assert!(registry.has_loaded_tool("DeferredTool").await);
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
                    visible_tool_names: Arc::new(std::collections::HashSet::new()),
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

    #[tokio::test]
    async fn resolve_deferred_async_returns_factory_failed_and_keeps_retryable() {
        let registry = ToolRegistry::new();
        registry.register_deferred_with_category(
            "PanicFactory".to_string(),
            "panic".to_string(),
            json!({"type":"object"}),
            Box::new(|| panic!("factory failed")),
            DeferredToolCategory::System,
        );

        let first = registry.resolve_deferred_with_outcome("PanicFactory").await;
        assert!(matches!(first, DeferredResolveOutcome::FactoryFailed { .. }));

        let deferred = registry.deferred_definitions().await;
        assert!(deferred.iter().any(|d| d.name == "PanicFactory"));
    }

    #[tokio::test]
    async fn load_deferred_by_category_returns_structured_outcome() {
        let registry = ToolRegistry::new();
        let panic_counter = Arc::new(AtomicUsize::new(0));
        registry.register_deferred_with_category(
            "TaskA".to_string(),
            "ok".to_string(),
            json!({"type":"object"}),
            Box::new(|| Arc::new(StaticTool { name: "TaskA" })),
            DeferredToolCategory::Task,
        );
        registry.register_deferred_with_category(
            "TaskB".to_string(),
            "panic".to_string(),
            json!({"type":"object"}),
            Box::new({
                let panic_counter = panic_counter.clone();
                move || {
                    panic_counter.fetch_add(1, Ordering::SeqCst);
                    panic!("taskb factory failed");
                }
            }),
            DeferredToolCategory::Task,
        );

        let outcome = registry
            .load_deferred_by_category(&DeferredToolCategory::Task, true)
            .await;
        assert_eq!(outcome.requested, 2);
        assert_eq!(outcome.loaded, 1);
        assert_eq!(outcome.failed, 1);
        assert_eq!(panic_counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn concurrent_resolve_same_tool_executes_factory_once() {
        let registry = Arc::new(ToolRegistry::new());
        let factory_counter = Arc::new(AtomicUsize::new(0));

        registry.register_deferred_with_category(
            "ConcurrentTool".to_string(),
            "concurrency".to_string(),
            json!({"type":"object"}),
            Box::new({
                let factory_counter = factory_counter.clone();
                move || {
                    factory_counter.fetch_add(1, Ordering::SeqCst);
                    Arc::new(StaticTool { name: "ConcurrentTool" })
                }
            }),
            DeferredToolCategory::System,
        );

        let mut tasks = Vec::new();
        for _ in 0..100 {
            let registry = registry.clone();
            tasks.push(tokio::spawn(async move {
                registry.resolve_deferred_with_outcome("ConcurrentTool").await
            }));
        }

        let mut loaded = 0usize;
        let mut already_loaded = 0usize;
        let mut not_found = 0usize;
        let mut failed = 0usize;
        for task in tasks {
            match task.await.unwrap() {
                DeferredResolveOutcome::Loaded => loaded += 1,
                DeferredResolveOutcome::AlreadyLoaded => already_loaded += 1,
                DeferredResolveOutcome::NotFound => not_found += 1,
                DeferredResolveOutcome::FactoryFailed { .. } => failed += 1,
            }
        }

        assert_eq!(factory_counter.load(Ordering::SeqCst), 1);
        assert_eq!(loaded, 1);
        assert_eq!(already_loaded, 99);
        assert_eq!(not_found, 0);
        assert_eq!(failed, 0);
    }

    #[tokio::test]
    #[ignore]
    async fn bench_registry_read_heavy_mixed_write() {
        let registry = Arc::new(ToolRegistry::new());
        for i in 0..40 {
            let tool_name = format!("DeferredTask{i}");
            registry.register_deferred_with_category(
                tool_name.clone(),
                format!("tool-{i}"),
                json!({"type":"object","properties":{"v":{"type":"integer"}}}),
                Box::new(move || Arc::new(StaticTool { name: "DeferredTask0" })),
                DeferredToolCategory::Task,
            );
        }

        let read_start = Instant::now();
        let mut reads = Vec::new();
        for _ in 0..500 {
            let registry = registry.clone();
            reads.push(tokio::spawn(async move {
                let _ = registry.tool_definitions();
                let _ = registry.get_turn_view(true, true, true);
                let _ = registry.deferred_definitions().await;
            }));
        }
        for read in reads {
            read.await.unwrap();
        }
        let read_elapsed = read_start.elapsed();

        let mixed_start = Instant::now();
        let mut mixed = Vec::new();
        for i in 0..200 {
            let registry = registry.clone();
            mixed.push(tokio::spawn(async move {
                if i % 10 == 0 {
                    let _ = registry.resolve_deferred_with_outcome("DeferredTask0").await;
                } else {
                    let _ = registry.tool_definitions();
                    let _ = registry.get_turn_view(true, true, true);
                }
            }));
        }
        for task in mixed {
            task.await.unwrap();
        }
        let mixed_elapsed = mixed_start.elapsed();

        println!(
            "[registry-bench] read_heavy={}ms mixed={}ms",
            read_elapsed.as_millis(),
            mixed_elapsed.as_millis()
        );
    }

    #[tokio::test]
    async fn tool_metadata_returns_loaded_tool() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(StaticTool { name: "Bash" }));

        let meta = registry.tool_metadata("Bash").await;
        assert!(meta.is_some());
        let meta = meta.unwrap();
        assert_eq!(meta.name, "Bash");
        assert!(meta.loaded);
        assert!(!meta.deferred);
        assert!(meta.input_schema.is_object());
    }

    #[tokio::test]
    async fn tool_metadata_returns_deferred_tool() {
        let registry = ToolRegistry::new();
        registry.register_deferred_with_category(
            "DeferredTool".to_string(),
            "deferred description".to_string(),
            json!({"type": "object", "properties": {"key": {"type": "string"}}}),
            Box::new(|| Arc::new(StaticTool { name: "DeferredTool" })),
            DeferredToolCategory::Task,
        );

        let meta = registry.tool_metadata("DeferredTool").await;
        assert!(meta.is_some());
        let meta = meta.unwrap();
        assert_eq!(meta.name, "DeferredTool");
        assert!(!meta.loaded);
        assert!(meta.deferred);
        assert_eq!(meta.category, Some(DeferredToolCategory::Task));
        assert!(meta.input_schema.is_object());
    }

    #[tokio::test]
    async fn tool_metadata_loaded_takes_priority_over_deferred() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(StaticTool { name: "SameName" }));
        registry.register_deferred(
            "SameName".to_string(),
            "deferred version".to_string(),
            json!({"type": "object"}),
            Box::new(|| Arc::new(StaticTool { name: "SameName" })),
        );

        let meta = registry.tool_metadata("SameName").await;
        assert!(meta.is_some());
        let meta = meta.unwrap();
        assert!(meta.loaded);
        assert!(!meta.deferred);
        assert_eq!(meta.description, "SameName description");
    }

    #[tokio::test]
    async fn tool_metadata_returns_none_for_unknown() {
        let registry = ToolRegistry::new();
        assert!(registry.tool_metadata("UnknownTool").await.is_none());
    }

    #[tokio::test]
    async fn all_tool_metadata_includes_loaded_and_deferred() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(StaticTool { name: "Bash" }));
        registry.register(Box::new(StaticTool { name: "Read" }));
        registry.register_deferred_with_category(
            "DeferredTool".to_string(),
            "deferred".to_string(),
            json!({"type": "object"}),
            Box::new(|| Arc::new(StaticTool { name: "DeferredTool" })),
            DeferredToolCategory::Skill,
        );

        let metas = registry.all_tool_metadata().await;
        let names: Vec<&str> = metas.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"Bash"));
        assert!(names.contains(&"Read"));
        assert!(names.contains(&"DeferredTool"));
        assert_eq!(metas.len(), 3);
        let deferred = metas.into_iter().find(|meta| meta.name == "DeferredTool").unwrap();
        assert_eq!(deferred.category, Some(DeferredToolCategory::Skill));
    }

    #[tokio::test]
    async fn execute_tool_info_uses_schema_validation() {
        let registry = ToolRegistry::new();

        let output = registry.execute("ToolInfo", json!("Bash"), None).await.unwrap();

        assert!(output.is_error);
        assert!(output.content.contains("input must be a JSON object"));
    }
}
