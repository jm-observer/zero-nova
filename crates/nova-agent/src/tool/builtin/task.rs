use crate::event::AgentEvent;
use crate::tool::{RegisteredToolDefinition, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub subject: String,
    pub description: String,
    pub active_form: Option<String>,
    pub status: TaskStatus,
    pub owner: Option<String>,
    pub metadata: HashMap<String, Value>,
    pub blocks: Vec<String>,     // task IDs this task blocks
    pub blocked_by: Vec<String>, // task IDs blocking this task
    /// 是否是主任务（用户直接创建的任务 vs 自动生成的子任务）
    pub is_main_task: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

impl TaskStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Deleted => "deleted",
        }
    }
}

#[derive(Clone)]
pub struct TaskUpdateRequest {
    pub status: Option<TaskStatus>,
    pub subject: Option<String>,
    pub description: Option<String>,
    pub active_form: Option<String>,
    pub owner: Option<String>,
    pub metadata: Option<HashMap<String, Value>>,
    pub add_blocks: Option<Vec<String>>,
    pub add_blocked_by: Option<Vec<String>>,
}

/// In-memory store for tasks within a session.
///
/// # 编排模式 metadata 约定
///
/// 在多 Agent 编排场景中，`Task.metadata` 使用以下保留 key：
///
/// | Key | 类型 | 说明 |
/// |-----|------|------|
/// | `"orchestration_plan_id"` | `String` | 所属编排 Plan 的 ID |
/// | `"orchestration_stage_id"` | `String` | 所属 Stage 的 ID |
/// | `"orchestration_agent_id"` | `String` | 子 Agent 的标识符 |
/// | `"orchestration_role"` | `String` | `"orchestrator"` \| `"sub_agent"` \| `"reviewer"` |
pub struct TaskStore {
    tasks: HashMap<String, Task>,
    next_id: AtomicU64,
}

impl TaskStore {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn list_owned(&self) -> Vec<Task> {
        self.tasks.values().cloned().collect()
    }

    pub fn create(
        &mut self,
        subject: String,
        description: String,
        active_form: Option<String>,
        metadata: Option<HashMap<String, Value>>,
        is_main_task: bool,
    ) -> Task {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst).to_string();
        let now = chrono::Utc::now();
        let task = Task {
            id: id.clone(),
            subject,
            description,
            active_form,
            status: TaskStatus::Pending,
            owner: None,
            metadata: metadata.unwrap_or_default(),
            blocks: Vec::new(),
            blocked_by: Vec::new(),
            is_main_task,
            created_at: now,
            updated_at: now,
        };
        self.tasks.insert(id, task.clone());
        task
    }

    pub fn list(&self) -> Vec<&Task> {
        self.tasks.values().collect()
    }

    pub fn get(&self, id: &str) -> Option<&Task> {
        self.tasks.get(id)
    }

    pub fn update(&mut self, id: &str, update: TaskUpdateRequest) -> Result<Task> {
        self.ensure_task_exists(id)?;
        self.validate_dependencies(id, update.add_blocks.as_deref(), update.add_blocked_by.as_deref())?;

        let now = chrono::Utc::now();
        let next_status;
        {
            let task = self
                .tasks
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("Task {} not found", id))?;

            if let Some(subject) = update.subject.clone() {
                task.subject = subject;
            }
            if let Some(description) = update.description.clone() {
                task.description = description;
            }
            if let Some(active_form) = update.active_form.clone() {
                task.active_form = Some(active_form);
            }
            if let Some(owner) = update.owner.clone() {
                task.owner = Some(owner);
            }
            if let Some(metadata) = update.metadata.clone() {
                for (k, v) in metadata {
                    task.metadata.insert(k, v);
                }
            }
            if let Some(blocks) = update.add_blocks.as_ref() {
                for block_id in blocks {
                    push_unique(&mut task.blocks, block_id.clone());
                }
            }
            if let Some(blocked_by) = update.add_blocked_by.as_ref() {
                for blocked_by_id in blocked_by {
                    push_unique(&mut task.blocked_by, blocked_by_id.clone());
                }
            }

            next_status = update.status.clone().unwrap_or_else(|| task.status.clone());
            if next_status == TaskStatus::InProgress && !task.blocked_by.is_empty() {
                return Err(anyhow::anyhow!(
                    "Task {} is blocked by: {}",
                    id,
                    task.blocked_by.join(", ")
                ));
            }
            task.status = next_status.clone();
            task.updated_at = now;
        }

        if let Some(blocks) = update.add_blocks {
            for blocked_task_id in blocks {
                if let Some(blocked_task) = self.tasks.get_mut(&blocked_task_id) {
                    push_unique(&mut blocked_task.blocked_by, id.to_string());
                    blocked_task.updated_at = now;
                }
            }
        }

        if let Some(blocked_by) = update.add_blocked_by {
            for blocking_task_id in blocked_by {
                if let Some(blocking_task) = self.tasks.get_mut(&blocking_task_id) {
                    push_unique(&mut blocking_task.blocks, id.to_string());
                    blocking_task.updated_at = now;
                }
            }
        }

        if next_status == TaskStatus::Completed {
            let blocked_tasks = self
                .tasks
                .get(id)
                .map(|current| current.blocks.clone())
                .unwrap_or_default();
            for blocked_task_id in blocked_tasks {
                if let Some(blocked_task) = self.tasks.get_mut(&blocked_task_id) {
                    blocked_task.blocked_by.retain(|blocked_by_id| blocked_by_id != id);
                    blocked_task.updated_at = now;
                }
            }
        }

        self.tasks
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Task {} not found", id))
    }

    fn ensure_task_exists(&self, id: &str) -> Result<()> {
        if self.tasks.contains_key(id) {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Task {} not found", id))
        }
    }

    fn validate_dependencies(
        &self,
        id: &str,
        add_blocks: Option<&[String]>,
        add_blocked_by: Option<&[String]>,
    ) -> Result<()> {
        for dependency_id in add_blocks
            .into_iter()
            .flatten()
            .chain(add_blocked_by.into_iter().flatten())
        {
            if dependency_id == id {
                return Err(anyhow::anyhow!("Task {} cannot depend on itself", id));
            }
            if !self.tasks.contains_key(dependency_id) {
                return Err(anyhow::anyhow!("Task {} not found", dependency_id));
            }
        }
        Ok(())
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

#[derive(Clone)]
pub struct TaskStoreHandle {
    inner: Arc<RwLock<TaskStore>>,
}

impl TaskStoreHandle {
    pub fn new(store: TaskStore) -> Self {
        Self {
            inner: Arc::new(RwLock::new(store)),
        }
    }

    pub async fn create_task(
        &self,
        subject: String,
        description: String,
        active_form: Option<String>,
        metadata: Option<HashMap<String, Value>>,
        is_main_task: bool,
    ) -> Task {
        let mut store = self.inner.write().await;
        store.create(subject, description, active_form, metadata, is_main_task)
    }

    pub async fn list_tasks(&self) -> Vec<Task> {
        let store = self.inner.read().await;
        store.list_owned()
    }

    pub async fn update_task(&self, id: &str, update: TaskUpdateRequest) -> Result<Task> {
        let mut store = self.inner.write().await;
        store.update(id, update)
    }
}

pub struct TaskCreateTool {
    store: TaskStoreHandle,
}

impl TaskCreateTool {
    pub fn new(store: TaskStoreHandle) -> Self {
        Self { store }
    }

    pub fn input_schema() -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "subject": { "type": "string", "description": "Brief task title" },
                "description": { "type": "string", "description": "What needs to be done" },
                "active_form": { "type": "string", "description": "Present continuous form for spinner display (e.g., 'Compiling code')" },
                "metadata": { "type": "object", "description": "Arbitrary metadata" }
            },
            "required": ["subject", "description"]
        })
    }
}

/// 关键词检测器，用于检测用户输入中是否需要创建主任务。
pub struct TaskKeywordDetector {
    /// 触发主任务创建的关键词列表
    keywords: Vec<&'static str>,
}

impl TaskKeywordDetector {
    /// 创建默认的关键词检测器。
    pub fn new() -> Self {
        Self {
            keywords: vec![
                "创建任务",
                "新建任务",
                "add task",
                "new task",
                "create task",
                "开始",
                "开始做",
                "启动",
                "track",
                "记录",
                "跟踪",
            ],
        }
    }

    /// 检查输入是否包含任务关键词。
    pub fn is_task_keyword(&self, input: &str) -> bool {
        let input_lower = input.to_lowercase();
        self.keywords.iter().any(|keyword| input_lower.contains(keyword))
    }

    /// 获取 détected 的关键词列表。
    pub fn get_keywords(&self) -> &[&'static str] {
        &self.keywords
    }
}

impl Default for TaskKeywordDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for TaskCreateTool {
    fn definition(&self) -> RegisteredToolDefinition {
        RegisteredToolDefinition {
            name: "TaskCreate".to_string(),
            description: "Creates a new task in the session's task store.".to_string(),
            input_schema: Self::input_schema(),
            defer_loading: false,
        }
    }

    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        let subject = input["subject"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'subject'"))?
            .to_string();
        let description = input["description"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'description'"))?
            .to_string();
        let active_form = input["active_form"].as_str().map(|s| s.to_string());
        let metadata = input["metadata"].as_object().cloned().map(|m| m.into_iter().collect());

        let task = self
            .store
            .create_task(subject.clone(), description, active_form, metadata, true)
            .await;

        if let Some(ctx) = context {
            let _ = ctx
                .event_tx
                .send(AgentEvent::TaskCreated {
                    id: task.id.clone(),
                    subject,
                })
                .await;
        }

        Ok(ToolOutput {
            content: serde_json::to_string(&task)?,
            is_error: false,
        })
    }
}

pub struct TaskListTool {
    store: TaskStoreHandle,
}

impl TaskListTool {
    pub fn new(store: TaskStoreHandle) -> Self {
        Self { store }
    }

    pub fn input_schema() -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }
}

#[async_trait::async_trait]
impl Tool for TaskListTool {
    fn definition(&self) -> RegisteredToolDefinition {
        RegisteredToolDefinition {
            name: "TaskList".to_string(),
            description: "Lists all tasks in the session's task store.".to_string(),
            input_schema: Self::input_schema(),
            defer_loading: false,
        }
    }

    async fn execute(&self, _input: Value, _context: Option<ToolContext>) -> Result<ToolOutput> {
        let tasks = self.store.list_tasks().await;
        Ok(ToolOutput {
            content: serde_json::to_string(&tasks)?,
            is_error: false,
        })
    }
}

pub struct TaskUpdateTool {
    store: TaskStoreHandle,
}

impl TaskUpdateTool {
    pub fn new(store: TaskStoreHandle) -> Self {
        Self { store }
    }

    pub fn input_schema() -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Task ID" },
                "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "deleted"] },
                "subject": { "type": "string" },
                "description": { "type": "string" },
                "active_form": { "type": "string" },
                "owner": { "type": "string" },
                "metadata": { "type": "object" },
                "addBlocks": { "type": "array", "items": { "type": "string" } },
                "addBlockedBy": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["id"]
        })
    }
}

#[async_trait::async_trait]
impl Tool for TaskUpdateTool {
    fn definition(&self) -> RegisteredToolDefinition {
        RegisteredToolDefinition {
            name: "TaskUpdate".to_string(),
            description: "Updates an existing task.".to_string(),
            input_schema: Self::input_schema(),
            defer_loading: false,
        }
    }

    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        let id = input["id"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'id'"))?;

        let update = TaskUpdateRequest {
            status: input["status"].as_str().and_then(|s| match s {
                "pending" => Some(TaskStatus::Pending),
                "in_progress" => Some(TaskStatus::InProgress),
                "completed" => Some(TaskStatus::Completed),
                "deleted" => Some(TaskStatus::Deleted),
                _ => None,
            }),
            subject: input["subject"].as_str().map(|s| s.to_string()),
            description: input["description"].as_str().map(|s| s.to_string()),
            active_form: input["active_form"].as_str().map(|s| s.to_string()),
            owner: input["owner"].as_str().map(|s| s.to_string()),
            metadata: input["metadata"].as_object().cloned().map(|m| m.into_iter().collect()),
            add_blocks: input["addBlocks"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()),
            add_blocked_by: input["addBlockedBy"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()),
        };

        let task = self.store.update_task(id, update).await?;

        if let Some(ctx) = context {
            let _ = ctx
                .event_tx
                .send(AgentEvent::TaskStatusChanged {
                    id: task.id.clone(),
                    subject: task.subject.clone(),
                    status: task.status.as_str().to_string(),
                    active_form: task.active_form.clone(),
                })
                .await;
        }

        Ok(ToolOutput {
            content: serde_json::to_string(&task)?,
            is_error: false,
        })
    }
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TaskCreateTool, TaskListTool, TaskStatus, TaskStore, TaskStoreHandle, TaskUpdateRequest, TaskUpdateTool,
    };
    use crate::event::AgentEvent;
    use crate::tool::{Tool, ToolContext};
    use serde_json::json;
    use std::collections::HashSet;
    use std::sync::Arc;
    use tokio::sync::{mpsc, Mutex};

    #[test]
    fn completing_task_unblocks_dependents() {
        let mut store = TaskStore::new();
        let blocker = store.create("blocker".to_string(), "blocker".to_string(), None, None, true);
        let blocked = store.create("blocked".to_string(), "blocked".to_string(), None, None, true);

        store
            .update(
                &blocker.id,
                TaskUpdateRequest {
                    status: None,
                    subject: None,
                    description: None,
                    active_form: None,
                    owner: None,
                    metadata: None,
                    add_blocks: Some(vec![blocked.id.clone()]),
                    add_blocked_by: None,
                },
            )
            .unwrap();

        assert_eq!(store.get(&blocked.id).unwrap().blocked_by, vec![blocker.id.clone()]);

        store
            .update(
                &blocker.id,
                TaskUpdateRequest {
                    status: Some(TaskStatus::Completed),
                    subject: None,
                    description: None,
                    active_form: None,
                    owner: None,
                    metadata: None,
                    add_blocks: None,
                    add_blocked_by: None,
                },
            )
            .unwrap();

        assert!(store.get(&blocked.id).unwrap().blocked_by.is_empty());
    }

    #[tokio::test]
    async fn task_store_handle_create_list_update_roundtrip() {
        let store = TaskStoreHandle::new(TaskStore::new());
        let created = store
            .create_task(
                "subject-1".to_string(),
                "desc-1".to_string(),
                Some("running-1".to_string()),
                None,
                true,
            )
            .await;

        let listed = store.list_tasks().await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, created.id);
        assert_eq!(listed[0].subject, "subject-1");

        let updated = store
            .update_task(
                &created.id,
                TaskUpdateRequest {
                    status: Some(TaskStatus::InProgress),
                    subject: None,
                    description: None,
                    active_form: None,
                    owner: None,
                    metadata: None,
                    add_blocks: None,
                    add_blocked_by: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.status, TaskStatus::InProgress);
        let listed_after_update = store.list_tasks().await;
        assert_eq!(listed_after_update.len(), 1);
        assert_eq!(listed_after_update[0].status, TaskStatus::InProgress);
    }

    #[tokio::test]
    async fn task_store_handle_list_returns_owned_snapshot() {
        let store = TaskStoreHandle::new(TaskStore::new());
        let created = store
            .create_task("snapshot".to_string(), "snapshot".to_string(), None, None, true)
            .await;

        let snapshot = store.list_tasks().await;
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].status, TaskStatus::Pending);

        let _ = store
            .update_task(
                &created.id,
                TaskUpdateRequest {
                    status: Some(TaskStatus::Completed),
                    subject: None,
                    description: None,
                    active_form: None,
                    owner: None,
                    metadata: None,
                    add_blocks: None,
                    add_blocked_by: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(snapshot[0].status, TaskStatus::Pending);
        let latest = store.list_tasks().await;
        assert_eq!(latest[0].status, TaskStatus::Completed);
    }

    #[test]
    fn blocked_task_cannot_start() {
        let mut store = TaskStore::new();
        let blocker = store.create("blocker".to_string(), "blocker".to_string(), None, None, true);
        let blocked = store.create("blocked".to_string(), "blocked".to_string(), None, None, true);

        store
            .update(
                &blocked.id,
                TaskUpdateRequest {
                    status: None,
                    subject: None,
                    description: None,
                    active_form: None,
                    owner: None,
                    metadata: None,
                    add_blocks: None,
                    add_blocked_by: Some(vec![blocker.id.clone()]),
                },
            )
            .unwrap();

        let err = store
            .update(
                &blocked.id,
                TaskUpdateRequest {
                    status: Some(TaskStatus::InProgress),
                    subject: None,
                    description: None,
                    active_form: None,
                    owner: None,
                    metadata: None,
                    add_blocks: None,
                    add_blocked_by: None,
                },
            )
            .unwrap_err();

        assert!(err.to_string().contains("blocked by"));
    }

    #[tokio::test]
    async fn task_store_handle_concurrent_list_succeeds() {
        let store = TaskStoreHandle::new(TaskStore::new());
        for i in 0..10 {
            let _ = store
                .create_task(format!("subject-{i}"), format!("desc-{i}"), None, None, true)
                .await;
        }

        let mut joins = Vec::new();
        for _ in 0..16 {
            let store = store.clone();
            joins.push(tokio::spawn(async move { store.list_tasks().await.len() }));
        }

        for join in joins {
            let len = join.await.unwrap();
            assert_eq!(len, 10);
        }
    }

    #[tokio::test]
    async fn task_create_tool_emits_task_created_event() {
        let store = TaskStoreHandle::new(TaskStore::new());
        let tool = TaskCreateTool::new(store.clone());
        let (event_tx, mut event_rx) = mpsc::channel(4);

        let output = tool
            .execute(
                json!({
                    "subject": "create-subject",
                    "description": "create-desc",
                    "active_form": "Creating task"
                }),
                Some(ToolContext {
                    event_tx,
                    tool_use_id: "tool-1".to_string(),
                    session_id: "session-1".to_string(),
                    task_store: Some(store.clone()),
                    skill_registry: None,
                    read_files: Arc::new(Mutex::new(HashSet::new())),
                    turn_read_state: None,
                    environment: None,
                    shared_environment: None,
                    cancellation_token: None,
                    visible_tool_names: Arc::new(std::collections::HashSet::new()),
                }),
            )
            .await
            .unwrap();

        let created: super::Task = serde_json::from_str(&output.content).unwrap();
        let event = event_rx.recv().await.unwrap();
        match event {
            AgentEvent::TaskCreated { id, subject } => {
                assert_eq!(id, created.id);
                assert_eq!(subject, "create-subject");
            }
            _ => panic!("expected TaskCreated event"),
        }

        let listed = store.list_tasks().await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, created.id);
    }

    #[tokio::test]
    async fn task_update_tool_emits_status_changed_event() {
        let store = TaskStoreHandle::new(TaskStore::new());
        let created = store
            .create_task(
                "to-update".to_string(),
                "desc".to_string(),
                Some("Running".to_string()),
                None,
                true,
            )
            .await;
        let tool = TaskUpdateTool::new(store.clone());
        let (event_tx, mut event_rx) = mpsc::channel(4);

        let output = tool
            .execute(
                json!({
                    "id": created.id,
                    "status": "in_progress"
                }),
                Some(ToolContext {
                    event_tx,
                    tool_use_id: "tool-2".to_string(),
                    session_id: "session-1".to_string(),
                    task_store: Some(store.clone()),
                    skill_registry: None,
                    read_files: Arc::new(Mutex::new(HashSet::new())),
                    turn_read_state: None,
                    environment: None,
                    shared_environment: None,
                    cancellation_token: None,
                    visible_tool_names: Arc::new(std::collections::HashSet::new()),
                }),
            )
            .await
            .unwrap();

        let updated: super::Task = serde_json::from_str(&output.content).unwrap();
        let event = event_rx.recv().await.unwrap();
        match event {
            AgentEvent::TaskStatusChanged {
                id,
                subject,
                status,
                active_form,
            } => {
                assert_eq!(id, updated.id);
                assert_eq!(subject, updated.subject);
                assert_eq!(status, "in_progress");
                assert_eq!(active_form, updated.active_form);
            }
            _ => panic!("expected TaskStatusChanged event"),
        }
    }

    #[tokio::test]
    async fn task_list_tool_returns_owned_tasks_json() {
        let store = TaskStoreHandle::new(TaskStore::new());
        let _ = store
            .create_task("list-1".to_string(), "desc-1".to_string(), None, None, true)
            .await;
        let _ = store
            .create_task("list-2".to_string(), "desc-2".to_string(), None, None, true)
            .await;

        let tool = TaskListTool::new(store);
        let output = tool.execute(json!({}), None).await.unwrap();
        let tasks: Vec<super::Task> = serde_json::from_str(&output.content).unwrap();

        assert_eq!(tasks.len(), 2);
    }
}
