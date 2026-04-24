use crate::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

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

    pub fn create(
        &mut self,
        subject: String,
        description: String,
        active_form: Option<String>,
        metadata: Option<HashMap<String, Value>>,
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

pub struct TaskCreateTool {
    pub store: Arc<Mutex<TaskStore>>,
}

impl TaskCreateTool {
    pub fn new(store: Arc<Mutex<TaskStore>>) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl Tool for TaskCreateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "TaskCreate".to_string(),
            description: "Creates a new task in the session's task store.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "subject": { "type": "string", "description": "Brief task title" },
                    "description": { "type": "string", "description": "What needs to be done" },
                    "activeForm": { "type": "string", "description": "Present continuous form for spinner display (e.g., 'Compiling code')" },
                    "metadata": { "type": "object", "description": "Arbitrary metadata" }
                },
                "required": ["subject", "description"]
            }),
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
        let active_form = input["activeForm"].as_str().map(|s| s.to_string());
        let metadata = input["metadata"].as_object().cloned().map(|m| m.into_iter().collect());

        let mut store = self.store.lock().await;
        let task = store.create(subject.clone(), description, active_form, metadata);

        if let Some(ctx) = context {
            let _ = ctx
                .event_tx
                .send(crate::event::AgentEvent::TaskCreated {
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
    pub store: Arc<Mutex<TaskStore>>,
}

impl TaskListTool {
    pub fn new(store: Arc<Mutex<TaskStore>>) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl Tool for TaskListTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "TaskList".to_string(),
            description: "Lists all tasks in the session's task store.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            defer_loading: false,
        }
    }

    async fn execute(&self, _input: Value, _context: Option<ToolContext>) -> Result<ToolOutput> {
        let store = self.store.lock().await;
        let tasks = store.list();
        Ok(ToolOutput {
            content: serde_json::to_string(&tasks)?,
            is_error: false,
        })
    }
}

pub struct TaskUpdateTool {
    pub store: Arc<Mutex<TaskStore>>,
}

impl TaskUpdateTool {
    pub fn new(store: Arc<Mutex<TaskStore>>) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl Tool for TaskUpdateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "TaskUpdate".to_string(),
            description: "Updates an existing task.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Task ID" },
                    "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "deleted"] },
                    "subject": { "type": "string" },
                    "description": { "type": "string" },
                    "activeForm": { "type": "string" },
                    "owner": { "type": "string" },
                    "metadata": { "type": "object" },
                    "addBlocks": { "type": "array", "items": { "type": "string" } },
                    "addBlockedBy": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["id"]
            }),
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
            active_form: input["activeForm"].as_str().map(|s| s.to_string()),
            owner: input["owner"].as_str().map(|s| s.to_string()),
            metadata: input["metadata"].as_object().cloned().map(|m| m.into_iter().collect()),
            add_blocks: input["addBlocks"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()),
            add_blocked_by: input["addBlockedBy"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()),
        };

        let mut store = self.store.lock().await;
        let task = store.update(id, update)?;

        if let Some(ctx) = context {
            let _ = ctx
                .event_tx
                .send(crate::event::AgentEvent::TaskStatusChanged {
                    id: task.id.clone(),
                    subject: task.subject.clone(),
                    status: serde_json::to_string(&task.status)?.trim_matches('"').to_string(),
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
    use super::{TaskStatus, TaskStore, TaskUpdateRequest};

    #[test]
    fn completing_task_unblocks_dependents() {
        let mut store = TaskStore::new();
        let blocker = store.create("blocker".to_string(), "blocker".to_string(), None, None);
        let blocked = store.create("blocked".to_string(), "blocked".to_string(), None, None);

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

    #[test]
    fn blocked_task_cannot_start() {
        let mut store = TaskStore::new();
        let blocker = store.create("blocker".to_string(), "blocker".to_string(), None, None);
        let blocked = store.create("blocked".to_string(), "blocked".to_string(), None, None);

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
}
