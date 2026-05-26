use crate::event::AgentEvent;
use crate::prompt::EnvironmentSnapshot;
use crate::provider::types::ToolDefinition as ProviderToolDefinition;
use crate::skill::{CapabilityPolicy, SkillRegistry};
use crate::tool::{builtin, read_cache};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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

/// 工具声明"在当前 Agent 之外开启一个子会话承接后续对话"的副作用。
///
/// nova SDK 自身不消费此结构（仅透传到 [`AgentEvent::ChildSessionRequest`]）；
/// 由外部宿主（如 zero）决定具体语义——通常用于把进一步对话隔离到新会话，
/// 避免污染当前会话上下文。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChildSessionRequest {
    /// 注入新子会话的种子 user message。
    pub seed_user_message: String,
    /// 任意结构化负载，供宿主按需读取（如 `{"flagged_id": 12}`）。
    pub metadata: Value,
}

/// Result produced by a tool execution.
#[derive(Debug, Clone, Default)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
    /// 工具声明"开启子会话"副作用；`None` 表示无副作用（既有行为）。
    pub child_session: Option<ChildSessionRequest>,
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
pub struct ToolRegistry {
    state: Mutex<RegistryState>,
    snapshot: RwLock<Arc<RegistrySnapshot>>,
}

struct RegistryState {
    /// 仅 always-on 工具，注册期写入，解析行为不改写。
    tools: Vec<Arc<dyn Tool>>,
    /// 全局 deferred 工厂表，注册期写入后只读。
    deferred: Vec<DeferredToolEntry>,
    /// 会话级激活：`session_id -> (tool_name -> 已实例化工具)`。
    session_activations: HashMap<String, HashMap<String, Arc<dyn Tool>>>,
}

impl RegistryState {
    fn new() -> Self {
        Self {
            tools: Vec::new(),
            deferred: Vec::new(),
            session_activations: HashMap::new(),
        }
    }

    fn always_on_has(&self, name: &str) -> bool {
        self.tools.iter().any(|tool| tool.definition().name == name)
    }

    fn session_activated_tool(&self, session_id: &str, name: &str) -> Option<Arc<dyn Tool>> {
        self.session_activations
            .get(session_id)
            .and_then(|m| m.get(name))
            .cloned()
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
    pub fn get_agent_tool_subset(&self, _policy: &CapabilityPolicy) -> Tiny {
        let mut allowed_tools: Vec<String> = self.loaded.iter().map(|t| t.name.clone()).collect();
        allowed_tools.extend(self.deferred.iter().map(|def| def.name.clone()));

        Tiny {
            agent_id: String::new(),
            max_tools: allowed_tools.len(),
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

    async fn lock_state_async(&self) -> tokio::sync::MutexGuard<'_, RegistryState> {
        self.state.lock().await
    }

    async fn lock_snapshot_async(&self) -> tokio::sync::RwLockReadGuard<'_, Arc<RegistrySnapshot>> {
        self.snapshot.read().await
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
    pub async fn register(&self, tool: Box<dyn Tool>) {
        let mut state = self.lock_state_async().await;
        state.tools.push(Arc::from(tool));
        let _ = self.refresh_snapshot_locked_async(&state).await;
    }
    /// Registers multiple tools at once.
    pub async fn register_many(&self, tools: Vec<Box<dyn Tool>>) {
        let mut guard = self.lock_state_async().await;
        for tool in tools {
            guard.tools.push(Arc::from(tool));
        }
        let _ = self.refresh_snapshot_locked_async(&guard).await;
    }
    /// Registers a deferred tool that will only be fully loaded when activated via `resolve_deferred`.
    ///
    /// The tool's name and description are immediately available for `tool_search`,
    /// but the factory is not called until the tool is resolved.
    pub async fn register_deferred(
        &self,
        name: String,
        description: String,
        input_schema: Value,
        factory: Box<dyn Fn() -> Arc<dyn Tool> + Send + Sync>,
    ) {
        self.register_deferred_with_category(name, description, input_schema, factory, DeferredToolCategory::System)
            .await;
    }

    /// Registers a deferred tool with a specific category.
    pub async fn register_deferred_with_category(
        &self,
        name: String,
        description: String,
        input_schema: Value,
        factory: Box<dyn Fn() -> Arc<dyn Tool> + Send + Sync>,
        category: DeferredToolCategory,
    ) {
        let entry = DeferredToolEntry {
            name,
            description,
            input_schema,
            factory,
            category,
        };
        let mut state = self.lock_state_async().await;
        state.deferred.push(entry);
        let _ = self.refresh_snapshot_locked_async(&state).await;
    }
    /// Returns the definitions of all registered tools.
    pub async fn tool_definitions(&self) -> Vec<ProviderToolDefinition> {
        self.lock_snapshot_async().await.loaded_provider_definitions.clone()
    }

    pub async fn loaded_definitions(&self) -> Vec<RegisteredToolDefinition> {
        self.lock_snapshot_async().await.loaded_definitions.clone()
    }

    pub async fn has_loaded_tool(&self, name: &str) -> bool {
        self.lock_state_async()
            .await
            .tools
            .iter()
            .any(|tool| tool.definition().name == name)
    }

    /// 在指定 session 下解析一个 deferred 工具。
    pub async fn resolve_deferred(&self, session_id: &str, name: &str) -> bool {
        matches!(
            self.resolve_deferred_with_outcome(session_id, name).await,
            DeferredResolveOutcome::Loaded
        )
    }

    /// 在指定 session 下解析 deferred 工具：实例化工厂并存入该 session 的激活集合，
    /// 不改写全局 `tools`，不移除全局 `deferred` 项。
    pub async fn resolve_deferred_with_outcome(&self, session_id: &str, name: &str) -> DeferredResolveOutcome {
        let mut state = self.lock_state_async().await;
        if state.always_on_has(name) {
            return DeferredResolveOutcome::AlreadyLoaded;
        }
        if state.session_activated_tool(session_id, name).is_some() {
            return DeferredResolveOutcome::AlreadyLoaded;
        }
        let Some(pos) = state.deferred.iter().position(|d| d.name == name) else {
            return DeferredResolveOutcome::NotFound;
        };
        let tool = match std::panic::catch_unwind(AssertUnwindSafe(|| (state.deferred[pos].factory)())) {
            Ok(tool) => tool,
            Err(payload) => {
                let message = panic_payload_to_message(payload);
                return DeferredResolveOutcome::FactoryFailed { message };
            }
        };
        state
            .session_activations
            .entry(session_id.to_string())
            .or_default()
            .insert(name.to_string(), tool);
        let _ = self.refresh_snapshot_locked_async(&state).await;
        DeferredResolveOutcome::Loaded
    }

    /// 释放某 session 的全部激活工具（session 删除时调用）。
    pub async fn clear_session_activations(&self, session_id: &str) {
        let mut state = self.lock_state_async().await;
        if state.session_activations.remove(session_id).is_some() {
            let _ = self.refresh_snapshot_locked_async(&state).await;
        }
    }

    /// 获取指定 session 当前轮次的工具视图（`TurnToolView`）。
    ///
    /// - `loaded` = always-on 工具 + 该 session 已激活的 deferred 工具
    /// - `deferred` = 该 session 尚未激活的 deferred 工具（按 capability 过滤）
    pub async fn get_turn_view(
        &self,
        session_id: &str,
        tool_search_enabled: bool,
        skill_tool_enabled: bool,
        task_tools_enabled: bool,
    ) -> TurnToolView {
        let state = self.lock_state_async().await;
        let mut loaded: Vec<ProviderToolDefinition> = state
            .tools
            .iter()
            .map(|tool| {
                let def = tool.definition();
                ProviderToolDefinition {
                    name: def.name,
                    description: def.description,
                    input_schema: def.input_schema,
                }
            })
            .collect();

        let session_activated = state.session_activations.get(session_id);
        if let Some(activated) = session_activated {
            for tool in activated.values() {
                let def = tool.definition();
                if loaded.iter().any(|d| d.name == def.name) {
                    continue;
                }
                loaded.push(ProviderToolDefinition {
                    name: def.name,
                    description: def.description,
                    input_schema: def.input_schema,
                });
            }
        }

        let deferred: Vec<DeferredToolRepresentation> = state
            .deferred
            .iter()
            .filter(|entry| {
                if !task_tools_enabled && matches!(entry.category, DeferredToolCategory::Task) {
                    return false;
                }
                if state.always_on_has(&entry.name) {
                    return false;
                }
                !session_activated.map(|m| m.contains_key(&entry.name)).unwrap_or(false)
            })
            .map(DeferredToolEntry::to_representation)
            .collect();

        TurnToolView {
            loaded,
            deferred,
            tool_search_enabled,
            skill_tool_enabled,
            task_tools_enabled,
        }
    }

    pub async fn filter_deferred_by_policy(&self, policy: &CapabilityPolicy) -> Vec<DeferredToolRepresentation> {
        let _ = policy;
        self.lock_snapshot_async().await.deferred_representations.to_vec()
    }

    pub async fn deferred_tools_by_category(&self, category: &DeferredToolCategory) -> Vec<DeferredToolRepresentation> {
        self.lock_snapshot_async()
            .await
            .deferred_representations
            .iter()
            .filter(|entry| &entry.category == category)
            .cloned()
            .collect()
    }

    pub async fn deferred_definitions(&self) -> Vec<RegisteredToolDefinition> {
        self.lock_snapshot_async().await.deferred_definitions.clone()
    }

    /// 返回所有 deferred 工具的完整表示（含 category），用于外部只读 listing。
    pub async fn list_deferred_representations(&self) -> Vec<DeferredToolRepresentation> {
        self.lock_snapshot_async().await.deferred_representations.clone()
    }

    pub async fn load_deferred_by_category(
        &self,
        session_id: &str,
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
            match self.resolve_deferred_with_outcome(session_id, &name).await {
                DeferredResolveOutcome::Loaded => outcome.loaded += 1,
                DeferredResolveOutcome::AlreadyLoaded => outcome.already_loaded += 1,
                DeferredResolveOutcome::NotFound => outcome.not_found += 1,
                DeferredResolveOutcome::FactoryFailed { .. } => outcome.failed += 1,
            }
        }
        outcome
    }

    /// 查询指定 session 下单个工具的元信息视图。
    ///
    /// 命中顺序：always-on → 该 session 已激活 → deferred。
    pub async fn tool_metadata(&self, session_id: &str, name: &str) -> Option<ToolMetadataView> {
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

        if let Some(tool) = state.session_activated_tool(session_id, name) {
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

        if name == builtin::tool_search::TOOL_NAME {
            let definition = builtin::tool_search::tool_definition();
            if let Err(error_output) = crate::tool::schema_validation::validate_input_against_schema(
                builtin::tool_search::TOOL_NAME,
                &input,
                &definition.input_schema,
            ) {
                return Ok(error_output);
            }
            return builtin::tool_search::execute(self, input, context.as_ref()).await;
        }

        let canonical_name = match name {
            "bash" => "Bash",
            "read_file" => "Read",
            "write_file" => "Write",
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

        let tool = {
            let state = self.lock_state_async().await;
            let always_on = state
                .tools
                .iter()
                .find(|tool| tool.definition().name == canonical_name)
                .cloned();
            match always_on {
                Some(tool) => Some(tool),
                None => {
                    let session_id = context.as_ref().map(|c| c.session_id.as_str()).unwrap_or("");
                    state.session_activated_tool(session_id, canonical_name)
                }
            }
        };

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
            child_session: None,
        })
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
        ChildSessionRequest, DeferredResolveOutcome, DeferredToolCategory, RegisteredToolDefinition, Tool, ToolContext,
        ToolOutput, ToolRegistry,
    };
    use crate::prompt::EnvironmentSnapshot;
    use anyhow::Result;
    use serde_json::{json, Value};
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant;
    use tokio::sync::{mpsc, Mutex};

    #[test]
    fn tool_output_default_has_no_child_session() {
        let out = ToolOutput::default();
        assert!(out.content.is_empty());
        assert!(!out.is_error);
        assert!(out.child_session.is_none());
    }

    #[test]
    fn child_session_request_serde_roundtrip() {
        let original = ChildSessionRequest {
            seed_user_message: "kickoff flagged_id=12 reason=路由错".to_string(),
            metadata: json!({ "flagged_id": 12, "tag": "review" }),
        };
        let s = serde_json::to_string(&original).expect("serialize");
        let parsed: ChildSessionRequest = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(parsed.seed_user_message, "kickoff flagged_id=12 reason=路由错");
        assert_eq!(parsed.metadata["flagged_id"], 12);
        assert_eq!(parsed.metadata["tag"], "review");
    }

    #[test]
    fn tool_output_with_child_session_carries_payload() {
        let out = ToolOutput {
            content: "已开启复盘会话".to_string(),
            is_error: false,
            child_session: Some(ChildSessionRequest {
                seed_user_message: "kickoff flagged_id=7".to_string(),
                metadata: json!({ "flagged_id": 7 }),
            }),
        };
        assert!(!out.is_error);
        let cs = out.child_session.expect("child_session set");
        assert_eq!(cs.seed_user_message, "kickoff flagged_id=7");
        assert_eq!(cs.metadata["flagged_id"], 7);
    }

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
                child_session: None,
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
                child_session: None,
            })
        }
    }

    #[tokio::test]
    async fn execute_supports_legacy_tool_names() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(StaticTool { name: "Bash" })).await;

        let output = registry.execute("bash", json!({}), None).await.unwrap();
        assert_eq!(output.content, "Bash");
    }

    #[tokio::test]
    async fn register_deferred_keeps_tool_in_deferred_list() {
        let registry = ToolRegistry::new();
        registry
            .register_deferred(
                "DeferredTool".to_string(),
                "Useful deferred tool".to_string(),
                json!({"type": "object"}),
                Box::new(|| Arc::new(StaticTool { name: "DeferredTool" })),
            )
            .await;

        assert!(!registry.has_loaded_tool("DeferredTool").await);
        let view = registry.get_turn_view("s1", false, false, false).await;
        assert!(view.deferred.iter().any(|d| d.name == "DeferredTool"));
    }

    #[tokio::test]
    async fn resolve_deferred_is_session_scoped() {
        let registry = ToolRegistry::new();
        registry
            .register_deferred(
                "DeferredTool".to_string(),
                "Useful deferred tool".to_string(),
                json!({"type": "object"}),
                Box::new(|| Arc::new(StaticTool { name: "DeferredTool" })),
            )
            .await;

        let outcome = registry.resolve_deferred_with_outcome("s1", "DeferredTool").await;
        assert_eq!(outcome, DeferredResolveOutcome::Loaded);

        // s1 视图中工具进入 loaded
        let v1 = registry.get_turn_view("s1", false, false, false).await;
        assert!(v1.loaded.iter().any(|d| d.name == "DeferredTool"));
        assert!(!v1.deferred.iter().any(|d| d.name == "DeferredTool"));

        // s2 视图中工具仍在 deferred
        let v2 = registry.get_turn_view("s2", false, false, false).await;
        assert!(!v2.loaded.iter().any(|d| d.name == "DeferredTool"));
        assert!(v2.deferred.iter().any(|d| d.name == "DeferredTool"));

        // 全局 always-on 未被改写
        assert!(!registry.has_loaded_tool("DeferredTool").await);
    }

    #[tokio::test]
    async fn already_loaded_on_repeat_resolve() {
        let registry = ToolRegistry::new();
        registry
            .register_deferred(
                "DeferredTool".to_string(),
                "desc".to_string(),
                json!({"type": "object"}),
                Box::new(|| Arc::new(StaticTool { name: "DeferredTool" })),
            )
            .await;

        assert_eq!(
            registry.resolve_deferred_with_outcome("s1", "DeferredTool").await,
            DeferredResolveOutcome::Loaded
        );
        assert_eq!(
            registry.resolve_deferred_with_outcome("s1", "DeferredTool").await,
            DeferredResolveOutcome::AlreadyLoaded
        );
    }

    #[tokio::test]
    async fn execute_resolves_session_activated_tool() {
        let registry = ToolRegistry::new();
        registry
            .register_deferred(
                "DeferredTool".to_string(),
                "desc".to_string(),
                json!({"type": "object"}),
                Box::new(|| Arc::new(StaticTool { name: "DeferredTool" })),
            )
            .await;
        registry.resolve_deferred("s1", "DeferredTool").await;

        let ctx = |session: &str| {
            let (event_tx, _rx) = mpsc::channel(1);
            ToolContext {
                event_tx,
                tool_use_id: "t".to_string(),
                session_id: session.to_string(),
                task_store: None,
                skill_registry: None,
                read_files: Arc::new(Mutex::new(HashSet::new())),
                turn_read_state: None,
                environment: None,
                shared_environment: None,
                cancellation_token: None,
                visible_tool_names: Arc::new(HashSet::new()),
            }
        };

        let ok = registry
            .execute("DeferredTool", json!({}), Some(ctx("s1")))
            .await
            .unwrap();
        assert!(!ok.is_error);
        assert_eq!(ok.content, "DeferredTool");

        let miss = registry
            .execute("DeferredTool", json!({}), Some(ctx("s2")))
            .await
            .unwrap();
        assert!(miss.is_error);
        assert!(miss.content.contains("not found"));
    }

    #[tokio::test]
    async fn clear_session_activations_releases_tools() {
        let registry = ToolRegistry::new();
        registry
            .register_deferred(
                "DeferredTool".to_string(),
                "desc".to_string(),
                json!({"type": "object"}),
                Box::new(|| Arc::new(StaticTool { name: "DeferredTool" })),
            )
            .await;
        registry.resolve_deferred("s1", "DeferredTool").await;
        assert!(registry
            .get_turn_view("s1", false, false, false)
            .await
            .loaded
            .iter()
            .any(|d| d.name == "DeferredTool"));

        registry.clear_session_activations("s1").await;
        let v = registry.get_turn_view("s1", false, false, false).await;
        assert!(!v.loaded.iter().any(|d| d.name == "DeferredTool"));
        assert!(v.deferred.iter().any(|d| d.name == "DeferredTool"));
    }

    #[tokio::test]
    async fn execute_rejects_unknown_fields_by_schema() {
        let registry = ToolRegistry::new();
        registry
            .register(Box::new(SchemaTool {
                name: "SchemaRead",
                schema: json!({
                    "type": "object",
                    "properties": {
                        "file_path": { "type": "string" }
                    },
                    "required": ["file_path"]
                }),
            }))
            .await;

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
        registry
            .register(Box::new(SchemaTool {
                name: "SchemaWrite",
                schema: json!({
                    "type": "object",
                    "properties": {
                        "file_path": { "type": "string" }
                    },
                    "required": ["file_path"]
                }),
            }))
            .await;

        let output = registry.execute("SchemaWrite", json!({}), None).await.unwrap();

        assert!(output.is_error);
        assert!(output.content.contains("missing required field"));
    }

    #[tokio::test]
    async fn execute_rejects_type_mismatch_by_schema() {
        let registry = ToolRegistry::new();
        registry
            .register(Box::new(SchemaTool {
                name: "SchemaBash",
                schema: json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                        "timeout_ms": { "type": "integer" }
                    },
                    "required": ["command"]
                }),
            }))
            .await;

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
        registry.register(Box::new(StaticTool { name: "Read" })).await;
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
    async fn register_deferred_factory_panic_returns_factory_failed_on_resolve() {
        let registry = ToolRegistry::new();
        registry
            .register_deferred_with_category(
                "PanicFactory".to_string(),
                "panic".to_string(),
                json!({"type":"object"}),
                Box::new(|| panic!("factory failed")),
                DeferredToolCategory::System,
            )
            .await;

        let outcome = registry.resolve_deferred_with_outcome("s1", "PanicFactory").await;
        assert!(matches!(outcome, DeferredResolveOutcome::FactoryFailed { .. }));
    }

    #[tokio::test]
    async fn register_deferred_multiple_tools_appear_in_deferred_list() {
        let registry = ToolRegistry::new();
        registry
            .register_deferred_with_category(
                "TaskA".to_string(),
                "ok".to_string(),
                json!({"type":"object"}),
                Box::new(|| Arc::new(StaticTool { name: "TaskA" })),
                DeferredToolCategory::Task,
            )
            .await;
        registry
            .register_deferred_with_category(
                "TaskB".to_string(),
                "ok".to_string(),
                json!({"type":"object"}),
                Box::new(|| Arc::new(StaticTool { name: "TaskB" })),
                DeferredToolCategory::Task,
            )
            .await;

        assert!(!registry.has_loaded_tool("TaskA").await);
        assert!(!registry.has_loaded_tool("TaskB").await);
        let view = registry.get_turn_view("s1", false, false, true).await;
        assert!(view.deferred.iter().any(|d| d.name == "TaskA"));
        assert!(view.deferred.iter().any(|d| d.name == "TaskB"));
    }

    #[tokio::test]
    async fn register_deferred_factory_executes_only_on_resolve() {
        let registry = ToolRegistry::new();
        let factory_counter = Arc::new(AtomicUsize::new(0));

        registry
            .register_deferred_with_category(
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
            )
            .await;

        assert_eq!(factory_counter.load(Ordering::SeqCst), 0);
        assert!(!registry.has_loaded_tool("ConcurrentTool").await);

        registry.resolve_deferred("s1", "ConcurrentTool").await;
        assert_eq!(factory_counter.load(Ordering::SeqCst), 1);
        // always-on 不变；激活体现在 session 视图
        assert!(!registry.has_loaded_tool("ConcurrentTool").await);
        assert!(registry
            .get_turn_view("s1", true, true, true)
            .await
            .loaded
            .iter()
            .any(|d| d.name == "ConcurrentTool"));
    }

    #[tokio::test]
    #[ignore]
    async fn bench_registry_read_heavy_mixed_write() {
        let registry = Arc::new(ToolRegistry::new());
        for i in 0..40 {
            let tool_name = format!("DeferredTask{i}");
            registry
                .register_deferred_with_category(
                    tool_name.clone(),
                    format!("tool-{i}"),
                    json!({"type":"object","properties":{"v":{"type":"integer"}}}),
                    Box::new(move || Arc::new(StaticTool { name: "DeferredTask0" })),
                    DeferredToolCategory::Task,
                )
                .await;
        }

        let read_start = Instant::now();
        let mut reads = Vec::new();
        for _ in 0..500 {
            let registry = registry.clone();
            reads.push(tokio::spawn(async move {
                let _ = registry.tool_definitions().await;
                let _ = registry.get_turn_view("s1", true, true, true).await;
                let _ = registry.loaded_definitions().await;
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
                let _ = i;
                let _ = registry.tool_definitions().await;
                let _ = registry.get_turn_view("s1", true, true, true).await;
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
        registry.register(Box::new(StaticTool { name: "Bash" })).await;

        let meta = registry.tool_metadata("s1", "Bash").await;
        assert!(meta.is_some());
        let meta = meta.unwrap();
        assert_eq!(meta.name, "Bash");
        assert!(meta.loaded);
        assert!(!meta.deferred);
        assert!(meta.input_schema.is_object());
    }

    #[tokio::test]
    async fn tool_metadata_marks_deferred_tools_correctly() {
        let registry = ToolRegistry::new();
        registry
            .register_deferred_with_category(
                "DeferredTool".to_string(),
                "deferred description".to_string(),
                json!({"type": "object", "properties": {"key": {"type": "string"}}}),
                Box::new(|| Arc::new(StaticTool { name: "DeferredTool" })),
                DeferredToolCategory::Task,
            )
            .await;

        let meta = registry.tool_metadata("s1", "DeferredTool").await;
        assert!(meta.is_some());
        let meta = meta.unwrap();
        assert_eq!(meta.name, "DeferredTool");
        assert!(!meta.loaded);
        assert!(meta.deferred);
        assert_eq!(meta.category, Some(DeferredToolCategory::Task));
        assert!(meta.input_schema.is_object());
    }

    #[tokio::test]
    async fn tool_metadata_for_duplicate_name_prefers_loaded() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(StaticTool { name: "SameName" })).await;
        registry
            .register_deferred(
                "SameName".to_string(),
                "deferred version".to_string(),
                json!({"type": "object"}),
                Box::new(|| Arc::new(StaticTool { name: "SameName" })),
            )
            .await;

        let meta = registry.tool_metadata("s1", "SameName").await;
        assert!(meta.is_some());
        let meta = meta.unwrap();
        assert!(meta.loaded);
        assert_eq!(meta.description, "SameName description");
    }

    #[tokio::test]
    async fn tool_metadata_returns_none_for_unknown() {
        let registry = ToolRegistry::new();
        assert!(registry.tool_metadata("s1", "UnknownTool").await.is_none());
    }

    #[tokio::test]
    async fn all_tool_metadata_includes_deferred_tools() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(StaticTool { name: "Bash" })).await;
        registry.register(Box::new(StaticTool { name: "Read" })).await;
        registry
            .register_deferred_with_category(
                "DeferredTool".to_string(),
                "deferred".to_string(),
                json!({"type": "object"}),
                Box::new(|| Arc::new(StaticTool { name: "DeferredTool" })),
                DeferredToolCategory::Skill,
            )
            .await;

        let metas = registry.all_tool_metadata().await;
        let names: Vec<&str> = metas.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"Bash"));
        assert!(names.contains(&"Read"));
        assert!(names.contains(&"DeferredTool"));
        assert_eq!(metas.len(), 3);
        let deferred_tool = metas.into_iter().find(|meta| meta.name == "DeferredTool").unwrap();
        assert!(!deferred_tool.loaded);
        assert!(deferred_tool.deferred);
        assert_eq!(deferred_tool.category, Some(DeferredToolCategory::Skill));
    }

    #[tokio::test]
    async fn execute_tool_info_uses_schema_validation() {
        let registry = ToolRegistry::new();

        let output = registry.execute("ToolInfo", json!("Bash"), None).await.unwrap();

        assert!(output.is_error);
        assert!(output.content.contains("input must be a JSON object"));
    }

    #[tokio::test]
    async fn loaded_definitions_returns_registered_tools() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(StaticTool { name: "Alpha" })).await;
        registry.register(Box::new(StaticTool { name: "Beta" })).await;

        let defs = registry.loaded_definitions().await;
        let names: HashSet<_> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(defs.len(), 2);
        assert!(names.contains("Alpha"));
        assert!(names.contains("Beta"));
    }

    #[tokio::test]
    async fn loaded_definitions_empty_when_no_tools() {
        let registry = ToolRegistry::new();
        assert!(registry.loaded_definitions().await.is_empty());
    }

    #[tokio::test]
    async fn list_deferred_representations_returns_registered_deferred() {
        let registry = ToolRegistry::new();
        registry
            .register_deferred_with_category(
                "DefA".to_string(),
                "A desc".to_string(),
                json!({"type": "object"}),
                Box::new(|| Arc::new(StaticTool { name: "DefA" })),
                DeferredToolCategory::Skill,
            )
            .await;
        registry
            .register_deferred_with_category(
                "DefB".to_string(),
                "B desc".to_string(),
                json!({"type": "object"}),
                Box::new(|| Arc::new(StaticTool { name: "DefB" })),
                DeferredToolCategory::Task,
            )
            .await;

        let reps = registry.list_deferred_representations().await;
        assert_eq!(reps.len(), 2);
        let by_name: std::collections::HashMap<_, _> = reps.iter().map(|r| (r.name.as_str(), &r.category)).collect();
        assert_eq!(by_name.get("DefA"), Some(&&DeferredToolCategory::Skill));
        assert_eq!(by_name.get("DefB"), Some(&&DeferredToolCategory::Task));
    }

    #[tokio::test]
    async fn list_deferred_representations_unaffected_by_session_activation() {
        let registry = ToolRegistry::new();
        registry
            .register_deferred(
                "DeferredTool".to_string(),
                "desc".to_string(),
                json!({"type": "object"}),
                Box::new(|| Arc::new(StaticTool { name: "DeferredTool" })),
            )
            .await;

        assert_eq!(
            registry.resolve_deferred_with_outcome("s1", "DeferredTool").await,
            DeferredResolveOutcome::Loaded
        );

        let reps = registry.list_deferred_representations().await;
        assert_eq!(reps.len(), 1);
        assert_eq!(reps[0].name, "DeferredTool");
    }
}
