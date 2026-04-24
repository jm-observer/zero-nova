# Zero-Nova Tool System Enhancement Design Document

## 1. Overview

This document describes the detailed design for enhancing the zero-nova tool system, bridging the gap between the current implementation and the ideal tool definitions defined in `docs/tool_definitions/`.

### 1.1 Current State

The current implementation provides 5 built-in tools under `crates/nova-core/src/tool/`:

| Current Tool         | File                        | Description                          |
| -------------------- | --------------------------- | ------------------------------------ |
| `bash`               | `builtin/bash.rs`           | Shell execution (sh/pwsh/cmd)        |
| `read_file`          | `builtin/file_ops.rs`       | Read file contents with paging       |
| `write_file`         | `builtin/file_ops.rs`       | Write file contents                  |
| `spawn_subagent`     | `builtin/subagent.rs`       | Spawn isolated subagent              |
| `web_fetch`          | `builtin/web_fetch.rs`      | Fetch URL and extract text           |
| `web_search`         | `builtin/web_search/mod.rs` | Web search (Google/Tavily/DuckDuckGo)|

### 1.2 Target State

The ideal tool set (from `docs/tool_definitions/`) defines 10 distinct tools:

| Target Tool     | Status         | Maps To Current  |
| --------------- | -------------- | ---------------- |
| `Agent`         | **Needs Major Upgrade** | `spawn_subagent` |
| `Bash`          | **Needs Enhancement**   | `bash`           |
| `Edit`          | **New**                 | _(none)_         |
| `Read`          | **Needs Enhancement**   | `read_file`      |
| `Write`         | **Needs Minor Update**  | `write_file`     |
| `Skill`         | **New**                 | _(none)_         |
| `TaskCreate`    | **New**                 | _(none)_         |
| `TaskList`      | **New**                 | _(none)_         |
| `TaskUpdate`    | **New**                 | _(none)_         |
| `ToolSearch`    | **New**                 | _(none)_         |

> Note: `Variant.json` is a duplicate of `Write.json`, ignored in this design.

---

## 2. Gap Analysis

### 2.1 New Tools to Implement (4 tools)

#### 2.1.1 `Edit` - File Editing Tool
- **Purpose**: Perform precise string replacements in files, sending only the diff rather than rewriting the entire file.
- **Gap**: Currently there is no edit tool. Users must use `write_file` to overwrite the entire file, which is error-prone for large files and generates unnecessary data transfer.
- **Parameters**:
  - `file_path: String` (required) - Absolute path to file
  - `old_string: String` (required) - Text to find
  - `new_string: String` (required) - Replacement text
  - `replace_all: bool` (default: false) - Replace all occurrences

#### 2.1.2 `Skill` - Skill Execution Tool
- **Purpose**: Execute named "skills" (predefined specialized workflows) within the agent conversation.
- **Gap**: The current system has no skill concept. Skills are a plugin mechanism that allows extending agent behavior without modifying core code.
- **Parameters**:
  - `skill: String` (required) - Skill name from available-skills registry
  - `args: String` (optional) - Arguments for the skill

#### 2.1.3 Task Management Tools (`TaskCreate`, `TaskList`, `TaskUpdate`)
- **Purpose**: Structured task tracking during agent sessions, enabling progress visibility, dependency management, and multi-step workflow coordination.
- **Gap**: The current system has no task/todo tracking. The agent cannot track multi-step work or show progress to the user.
- **TaskCreate Parameters**:
  - `subject: String` (required) - Brief task title
  - `description: String` (required) - What needs to be done
  - `activeForm: String` (optional) - Present continuous form for spinner display
  - `metadata: Object` (optional) - Arbitrary metadata
- **TaskList Parameters**: None (lists all tasks)
- **TaskUpdate Parameters**:
  - `id: String` (required) - Task ID
  - `status: String` (optional) - `pending` | `in_progress` | `completed` | `deleted`
  - `subject: String` (optional) - Update title
  - `description: String` (optional) - Update description
  - `activeForm: String` (optional) - Update spinner text
  - `owner: String` (optional) - Assign to agent
  - `metadata: Object` (optional) - Merge metadata
  - `addBlocks: [String]` (optional) - Tasks blocked by this one
  - `addBlockedBy: [String]` (optional) - Tasks this one depends on

#### 2.1.4 `ToolSearch` - Deferred Tool Loading
- **Purpose**: Support lazy-loading of tool schemas. Tools can be registered with only a name (deferred), and their full schemas are fetched on demand.
- **Gap**: The current `ToolRegistry` loads all tool definitions eagerly. For large tool sets or plugin systems, this is wasteful.
- **Parameters**:
  - `query: String` (required) - Query to find deferred tools (`select:Name` or keywords)
  - `max_results: number` (default: 5) - Max results to return

### 2.2 Existing Tools to Enhance

#### 2.2.1 `Bash` → Enhanced Bash
Current gaps:
- **Missing `description` parameter**: The ideal Bash tool accepts a human-readable description of the command for auditability and UX.
- **Missing `run_in_background` parameter**: No built-in background execution support.
- **Missing `dangerouslyDisableSandbox` parameter**: No sandbox escape mechanism.
- **Tool name casing**: Current is `bash`, ideal is `Bash` (capitalized).

#### 2.2.2 `read_file` → `Read` (Enhanced)
Current gaps:
- **Tool name**: `read_file` → `Read`
- **Parameter naming**: `path` → `file_path` (must be absolute path)
- **Missing `pages` parameter**: No PDF reading support.
- **Missing image support**: No multimodal file reading (PNG, JPG).
- **Missing Jupyter notebook support**: No `.ipynb` parsing.
- **Line number format**: Current uses `{:>5} | {}`, ideal uses `cat -n` style (tab-separated).

#### 2.2.3 `write_file` → `Write` (Enhanced)
Current gaps:
- **Tool name**: `write_file` → `Write`
- **Parameter naming**: `path` → `file_path` (must be absolute path)
- **Missing pre-read enforcement**: Ideal spec requires the file to have been read before writing (to prevent blind overwrites).

#### 2.2.4 `spawn_subagent` → `Agent` (Major Upgrade)
Current gaps:
- **Tool name**: `spawn_subagent` → `Agent`
- **Missing `subagent_type` parameter**: Current has no typed agent concept. Ideal supports multiple agent types (Explore, Plan, general-purpose, etc.).
- **Missing `isolation` parameter**: No git worktree isolation support.
- **Missing `run_in_background` parameter**: Current always runs synchronously.
- **Missing `description` parameter**: No short description for logging/UX.
- **Missing `model` parameter**: Model override exists but is undocumented in schema.
- **Missing `SendMessage` integration**: Cannot resume/continue a previously spawned agent.
- **Parameter naming**: `task` → `prompt`

---

## 3. Detailed Design

### 3.1 Architecture Overview

```
crates/nova-core/src/
  tool.rs                          # Core traits (enhanced)
  tool/
    builtin/
      mod.rs                       # Registration (updated)
      bash.rs                      # Enhanced Bash tool
      edit.rs                      # NEW: Edit tool
      file_ops.rs                  # Refactored into read.rs + write.rs
      read.rs                      # NEW: Enhanced Read tool (replaces ReadFileTool)
      write.rs                     # NEW: Enhanced Write tool (replaces WriteFileTool)
      agent.rs                     # NEW: Replaces subagent.rs
      skill.rs                     # NEW: Skill tool
      task.rs                      # NEW: TaskCreate/TaskList/TaskUpdate
      tool_search.rs               # NEW: ToolSearch tool
      web_fetch.rs                 # Existing (minor updates)
      web_search/                  # Existing (no changes)
    registry.rs                    # NEW: Enhanced ToolRegistry with deferred loading
```

### 3.2 Core Framework Changes (`tool.rs`)

#### 3.2.1 Enhanced ToolDefinition

```rust
#[derive(Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    /// If true, the tool schema is deferred and must be fetched via ToolSearch.
    pub defer_loading: bool,
}
```

#### 3.2.2 Enhanced ToolRegistry

```rust
pub struct ToolRegistry {
    /// Fully loaded tools.
    pub tools: Vec<Box<dyn Tool>>,
    /// Deferred tools: only name + description stored, schema loaded on demand.
    pub deferred: Vec<DeferredToolEntry>,
}

pub struct DeferredToolEntry {
    pub name: String,
    pub description: String,
    /// Factory to produce the full tool on demand.
    pub factory: Box<dyn Fn() -> Box<dyn Tool> + Send + Sync>,
}

impl ToolRegistry {
    /// Returns definitions for all loaded tools + stub definitions for deferred tools.
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> { ... }

    /// Resolves a deferred tool by name, loading its full schema.
    pub fn resolve_deferred(&mut self, name: &str) -> Option<ToolDefinition> { ... }

    /// Search deferred tools by keyword query.
    pub fn search_deferred(&self, query: &str, max_results: usize) -> Vec<ToolDefinition> { ... }
}
```

#### 3.2.3 ToolContext Enhancement

```rust
pub struct ToolContext {
    pub event_tx: mpsc::Sender<AgentEvent>,
    pub tool_use_id: String,
    /// Reference to the task store for TaskCreate/TaskList/TaskUpdate.
    pub task_store: Option<Arc<Mutex<TaskStore>>>,
    /// Reference to the skill registry.
    pub skill_registry: Option<Arc<SkillRegistry>>,
    /// Session-level state: files that have been read (for Write pre-read enforcement).
    pub read_files: Arc<Mutex<HashSet<String>>>,
}
```

### 3.3 New Tool: `Edit` (`builtin/edit.rs`)

#### 3.3.1 Implementation Strategy

```rust
pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Edit".to_string(),
            description: "Performs exact string replacements in files...".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "The absolute path to the file to modify" },
                    "old_string": { "type": "string", "description": "The text to replace" },
                    "new_string": { "type": "string", "description": "The text to replace it with" },
                    "replace_all": { "type": "boolean", "default": false, "description": "Replace all occurrences" }
                },
                "required": ["file_path", "old_string", "new_string"]
            }),
            defer_loading: false,
        }
    }

    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        // 1. Validate file_path is absolute
        // 2. Check read_files set in context (file must have been read first)
        // 3. Read file content
        // 4. Check old_string uniqueness:
        //    - If replace_all=false, old_string must appear exactly once
        //    - If replace_all=true, replace all occurrences
        // 5. Perform replacement
        // 6. Write modified content back
        // 7. Return diff summary (lines changed count)
    }
}
```

#### 3.3.2 Key Behaviors
- **Uniqueness enforcement**: When `replace_all=false`, the edit fails if `old_string` appears 0 or 2+ times.
- **Pre-read enforcement**: The file must have been previously read via the Read tool in the same session.
- **Whitespace preservation**: Exact match including indentation.
- **Error reporting**: Clear messages about why an edit failed (not found, ambiguous match, etc.).

### 3.4 New Tool: `Skill` (`builtin/skill.rs`)

#### 3.4.1 Skill Registry

```rust
/// Represents a loadable skill.
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    /// The system prompt patch or instructions to inject.
    pub instructions: String,
    /// Optional namespace/plugin prefix.
    pub plugin: Option<String>,
}

/// Registry for available skills.
pub struct SkillRegistry {
    skills: HashMap<String, SkillDefinition>,
}

impl SkillRegistry {
    pub fn new() -> Self { ... }

    /// Load skills from a directory (e.g. `.nova/skills/`).
    pub fn load_from_dir(path: &Path) -> Result<Self> { ... }

    /// Find a skill by name (supports `plugin:skill` format).
    pub fn find(&self, name: &str) -> Option<&SkillDefinition> { ... }

    /// List all available skill names.
    pub fn list(&self) -> Vec<String> { ... }
}
```

#### 3.4.2 SkillTool Implementation

```rust
pub struct SkillTool {
    registry: Arc<SkillRegistry>,
}

#[async_trait]
impl Tool for SkillTool {
    fn definition(&self) -> ToolDefinition { ... }

    async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        let skill_name = input["skill"].as_str()?;
        let args = input["args"].as_str().unwrap_or("");

        // 1. Look up skill in registry
        // 2. If found, return the skill's instructions as the tool output
        //    (the LLM will then follow those instructions)
        // 3. If not found, return error listing available skills
    }
}
```

#### 3.4.3 Skill Loading
Skills are loaded from the project's `.nova/skills/` directory. Each skill is a directory containing:
```
.nova/skills/
  my-skill/
    skill.toml          # metadata: name, description, aliases
    instructions.md     # system prompt patch injected when skill is invoked
    scripts/            # optional helper scripts
```

### 3.5 New Tools: Task Management (`builtin/task.rs`)

#### 3.5.1 TaskStore

```rust
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub subject: String,
    pub description: String,
    pub active_form: Option<String>,
    pub status: TaskStatus,
    pub owner: Option<String>,
    pub metadata: HashMap<String, Value>,
    pub blocks: Vec<String>,       // task IDs this task blocks
    pub blocked_by: Vec<String>,   // task IDs blocking this task
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

/// In-memory store for tasks within a session.
pub struct TaskStore {
    tasks: HashMap<String, Task>,
    next_id: AtomicU64,
}

impl TaskStore {
    pub fn new() -> Self { ... }
    pub fn create(&mut self, subject: String, description: String, active_form: Option<String>, metadata: Option<HashMap<String, Value>>) -> Task { ... }
    pub fn list(&self) -> Vec<&Task> { ... }
    pub fn get(&self, id: &str) -> Option<&Task> { ... }
    pub fn update(&mut self, id: &str, update: TaskUpdateRequest) -> Result<&Task> { ... }
}
```

#### 3.5.2 Three Separate Tool Structs

```rust
pub struct TaskCreateTool {
    store: Arc<Mutex<TaskStore>>,
}

pub struct TaskListTool {
    store: Arc<Mutex<TaskStore>>,
}

pub struct TaskUpdateTool {
    store: Arc<Mutex<TaskStore>>,
}
```

Each implements the `Tool` trait independently. All three share the same `Arc<Mutex<TaskStore>>` instance.

#### 3.5.3 Dependency Tracking
- `addBlocks`: When task A blocks task B, B cannot start until A completes.
- `addBlockedBy`: When task A is blocked by task B, A cannot start until B completes.
- When a task is marked `completed`, automatically remove it from the `blocked_by` lists of dependent tasks.

#### 3.5.4 UI Events
Task tools emit `AgentEvent` variants for the frontend to display progress:

```rust
// New AgentEvent variants
pub enum AgentEvent {
    // ... existing variants ...

    /// A task was created.
    TaskCreated { id: String, subject: String },
    /// A task status changed.
    TaskStatusChanged { id: String, subject: String, status: String, active_form: Option<String> },
}
```

### 3.6 New Tool: `ToolSearch` (`builtin/tool_search.rs`)

#### 3.6.1 Implementation

```rust
pub struct ToolSearchTool {
    registry: Arc<Mutex<ToolRegistry>>,  // shared reference to the registry
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn definition(&self) -> ToolDefinition { ... }

    async fn execute(&self, input: Value, _context: Option<ToolContext>) -> Result<ToolOutput> {
        let query = input["query"].as_str()?;
        let max_results = input["max_results"].as_u64().unwrap_or(5) as usize;

        let registry = self.registry.lock().await;

        if query.starts_with("select:") {
            // Direct selection: "select:Read,Edit,Grep"
            let names: Vec<&str> = query[7..].split(',').collect();
            let results = registry.resolve_deferred_batch(&names);
            format_as_functions_block(results)
        } else {
            // Keyword search
            let results = registry.search_deferred(query, max_results);
            format_as_functions_block(results)
        }
    }
}
```

#### 3.6.2 Query Semantics
- `select:Read,Edit` - Exact name match, load schemas for these tools
- `notebook jupyter` - Keyword search across deferred tool names and descriptions
- `+slack send` - Require "slack" in name, rank by relevance of remaining keywords

### 3.7 Enhanced `Agent` Tool (replaces `spawn_subagent`)

#### 3.7.1 Key Changes from Current `spawn_subagent`

| Aspect              | Current (`spawn_subagent`)     | Target (`Agent`)                    |
| ------------------- | ------------------------------ | ----------------------------------- |
| Name                | `spawn_subagent`               | `Agent`                             |
| Param: task/prompt  | `task`                         | `prompt`                            |
| Param: description  | _(none)_                       | Required, 3-5 word summary          |
| Agent types         | _(none)_                       | `subagent_type` with typed agents   |
| Background run      | _(none)_                       | `run_in_background: bool`           |
| Isolation           | Workspace directory only       | `isolation: "worktree"` (git worktree) |
| Resume              | _(none)_                       | `SendMessage` to resume by agent ID |
| Model override      | Hidden in input                | Explicit `model` enum parameter     |
| Tool whitelist      | All tools given to subagent    | Per-agent-type tool whitelist        |

#### 3.7.2 Agent Type System

Leverage the existing `AgentSpec` in `config.rs` (`GatewayConfig.agents`) to define agent types:

```rust
pub struct AgentTool {
    config: AppConfig,
    /// Map of agent type name → AgentSpec.
    agent_types: HashMap<String, AgentSpec>,
    /// Track running agents for SendMessage/resume.
    running_agents: Arc<Mutex<HashMap<String, AgentHandle>>>,
}

struct AgentHandle {
    id: String,
    agent_type: String,
    event_tx: mpsc::Sender<String>,  // for SendMessage
    // ... other state
}
```

#### 3.7.3 Background Execution

```rust
if run_in_background {
    let handle = tokio::spawn(async move {
        // Run agent in background
        // When complete, send notification via parent event_tx
    });
    // Return immediately with agent ID
    return Ok(ToolOutput {
        content: json!({ "agent_id": agent_id, "status": "running" }).to_string(),
        is_error: false,
    });
}
```

#### 3.7.4 Git Worktree Isolation

When `isolation: "worktree"` is specified:

```rust
async fn create_worktree(workspace: &Path) -> Result<(PathBuf, String)> {
    // 1. Generate unique branch name: agent/<agent_id>
    // 2. git worktree add <temp_path> -b <branch_name>
    // 3. Return (worktree_path, branch_name)
}

async fn cleanup_worktree(worktree_path: &Path, branch_name: &str, has_changes: bool) {
    if !has_changes {
        // git worktree remove <path>
        // git branch -d <branch_name>
    }
    // If has changes, return path+branch in result for user to review
}
```

### 3.8 Enhanced `Bash` Tool

#### 3.8.1 Schema Changes

Add new parameters to the input schema:

```rust
input_schema: json!({
    "type": "object",
    "properties": {
        "command": { "type": "string", "description": "The command to execute" },
        "timeout": { "type": "integer", "description": "Optional timeout in milliseconds (max 600000)" },
        "description": { "type": "string", "description": "Clear, concise description of what this command does" },
        "run_in_background": { "type": "boolean", "description": "Run in background, return immediately" },
        "dangerouslyDisableSandbox": { "type": "boolean", "description": "Override sandbox mode" }
    },
    "required": ["command"]
})
```

#### 3.8.2 Background Execution

```rust
if run_in_background {
    let (bg_tx, bg_rx) = mpsc::channel(1);
    tokio::spawn(async move {
        let result = execute_command(...).await;
        let _ = bg_tx.send(result).await;
        // Notify parent via event_tx
        let _ = event_tx.send(AgentEvent::BackgroundTaskComplete {
            id: tool_use_id,
            name: "Bash".to_string(),
        }).await;
    });
    return Ok(ToolOutput {
        content: "Command started in background. You will be notified when it completes.".to_string(),
        is_error: false,
    });
}
```

#### 3.8.3 Tool Name

Rename from `bash` to `Bash` (capitalized) to match the ideal definition.

### 3.9 Enhanced `Read` Tool

#### 3.9.1 Schema Changes

```rust
input_schema: json!({
    "type": "object",
    "properties": {
        "file_path": { "type": "string", "description": "The absolute path to the file to read" },
        "offset": { "type": "integer", "description": "Line number to start reading from" },
        "limit": { "type": "integer", "description": "Number of lines to read (max 2000)" },
        "pages": { "type": "string", "description": "Page range for PDF files (e.g., '1-5')" }
    },
    "required": ["file_path"]
})
```

#### 3.9.2 New Capabilities

1. **PDF Support**: Use a PDF parsing crate (e.g. `pdf-extract` or `lopdf`) to extract text from PDF files by page range.
2. **Image Support**: For image files (PNG/JPG/etc.), return the file as base64-encoded data for multimodal LLM consumption.
3. **Jupyter Notebook Support**: Parse `.ipynb` JSON format, returning all cells with their outputs.
4. **Read-tracking**: Record the file path in `ToolContext.read_files` for Edit/Write pre-read enforcement.

```rust
async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
    let file_path = input["file_path"].as_str()?;
    let ext = Path::new(file_path).extension().and_then(|e| e.to_str());

    // Track that this file has been read
    if let Some(ctx) = &context {
        if let Some(read_files) = &ctx.read_files {
            read_files.lock().await.insert(file_path.to_string());
        }
    }

    match ext {
        Some("pdf") => self.read_pdf(file_path, input["pages"].as_str()).await,
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("webp") =>
            self.read_image(file_path).await,
        Some("ipynb") => self.read_notebook(file_path).await,
        _ => self.read_text(file_path, offset, limit).await,
    }
}
```

#### 3.9.3 Tool Name

Rename from `read_file` to `Read`.

### 3.10 Enhanced `Write` Tool

#### 3.10.1 Pre-read Enforcement

```rust
async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
    let file_path = input["file_path"].as_str()?;

    // Check if file exists and was previously read
    if Path::new(file_path).exists() {
        if let Some(ctx) = &context {
            if let Some(read_files) = &ctx.read_files {
                let read_set = read_files.lock().await;
                if !read_set.contains(file_path) {
                    return Ok(ToolOutput {
                        content: "Error: You must read the file before writing to it. Use the Read tool first.".to_string(),
                        is_error: true,
                    });
                }
            }
        }
    }

    // ... proceed with write
}
```

#### 3.10.2 Parameter and Name Changes

- Rename tool from `write_file` to `Write`
- Rename parameter `path` to `file_path`
- Enforce absolute path validation

---

## 4. Configuration Changes

### 4.1 `config.rs` Updates

```rust
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ToolConfig {
    #[serde(default)]
    pub bash: BashConfig,
    #[serde(default)]
    pub skills_dir: Option<String>,    // NEW: path to skills directory
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BashConfig {
    pub shell: Option<String>,
    pub sandbox: Option<bool>,         // NEW: enable/disable sandboxing
}
```

### 4.2 `config.toml` Example

```toml
[tool]
skills_dir = ".nova/skills"

[tool.bash]
shell = "pwsh"
sandbox = true
```

---

## 5. Event System Changes

### 5.1 New AgentEvent Variants

```rust
pub enum AgentEvent {
    // ... existing variants ...

    /// Task management events
    TaskCreated {
        id: String,
        subject: String,
    },
    TaskStatusChanged {
        id: String,
        subject: String,
        status: String,
        active_form: Option<String>,
    },

    /// Background task completion notification
    BackgroundTaskComplete {
        id: String,
        name: String,
    },

    /// Skill loaded event
    SkillLoaded {
        skill_name: String,
    },
}
```

---

## 6. Registration Changes (`builtin/mod.rs`)

```rust
pub fn register_builtin_tools(
    registry: &mut ToolRegistry,
    config: &AppConfig,
    task_store: Arc<Mutex<TaskStore>>,
    skill_registry: Arc<SkillRegistry>,
) {
    // Core tools
    registry.register(Box::new(bash::BashTool::new(&config.tool.bash)));
    registry.register(Box::new(read::ReadTool::new(None)));
    registry.register(Box::new(write::WriteTool::new(None)));
    registry.register(Box::new(edit::EditTool::new()));

    // Agent tool
    registry.register(Box::new(agent::AgentTool::new(config.clone())));

    // Web tools
    registry.register(Box::new(web_search::WebSearchTool::new(&config.search)));
    registry.register(Box::new(web_fetch::WebFetchTool::new()));

    // Skill tool
    registry.register(Box::new(skill::SkillTool::new(skill_registry)));

    // Task tools
    registry.register(Box::new(task::TaskCreateTool::new(task_store.clone())));
    registry.register(Box::new(task::TaskListTool::new(task_store.clone())));
    registry.register(Box::new(task::TaskUpdateTool::new(task_store.clone())));

    // ToolSearch (deferred loading support)
    // Note: ToolSearch needs a reference to the registry itself,
    // so it's registered separately after registry construction.
}
```

---

## 7. Implementation Plan

### Phase 1: Core Infrastructure (Foundation)
1. Enhance `ToolDefinition` with `defer_loading` field
2. Enhance `ToolContext` with `read_files`, `task_store`, `skill_registry`
3. Add new `AgentEvent` variants
4. Update `ToolConfig` and `BashConfig` in config

### Phase 2: File Operation Tools
1. Implement `Edit` tool (`builtin/edit.rs`)
2. Enhance `Read` tool (rename, add PDF/image/notebook support, read-tracking)
3. Enhance `Write` tool (rename, add pre-read enforcement)
4. Deprecate `file_ops.rs`, split into `read.rs` and `write.rs`

### Phase 3: Task Management
1. Implement `TaskStore` data structure
2. Implement `TaskCreate` tool
3. Implement `TaskList` tool
4. Implement `TaskUpdate` tool (with dependency tracking)

### Phase 4: Agent System Upgrade
1. Implement typed agent system (`subagent_type`)
2. Add background execution support
3. Add git worktree isolation
4. Add SendMessage/resume capability
5. Rename `spawn_subagent` → `Agent`

### Phase 5: Skill & ToolSearch
1. Implement `SkillRegistry` and skill loading from `.nova/skills/`
2. Implement `Skill` tool
3. Implement deferred tool loading in `ToolRegistry`
4. Implement `ToolSearch` tool

### Phase 6: Bash Enhancement
1. Add `description` parameter
2. Add `run_in_background` support
3. Add `dangerouslyDisableSandbox` parameter
4. Rename `bash` → `Bash`

### Phase 7: Integration & Testing
1. Update `register_builtin_tools` with new signature
2. Update all callers of `register_builtin_tools`
3. Write unit tests for each new tool
4. Write integration tests for tool interactions (e.g., Read → Edit flow)
5. Update the gateway/server layer to pass new context fields

---

## 8. Migration & Backward Compatibility

### 8.1 Tool Name Migration

Since tool names change (`bash` → `Bash`, `read_file` → `Read`, etc.), the system should support a transition period:

```rust
impl ToolRegistry {
    pub async fn execute(&self, name: &str, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
        // Try exact match first
        // Then try legacy name mapping
        let canonical = match name {
            "bash" => "Bash",
            "read_file" => "Read",
            "write_file" => "Write",
            "spawn_subagent" => "Agent",
            other => other,
        };
        // ... find and execute
    }
}
```

### 8.2 Parameter Compatibility

For `Read` and `Write`, support both old (`path`) and new (`file_path`) parameter names during transition:

```rust
let file_path = input["file_path"]
    .as_str()
    .or_else(|| input["path"].as_str())  // legacy fallback
    .ok_or_else(|| anyhow!("Missing 'file_path' field"))?;
```

---

## 9. Dependencies

### 9.1 New Crate Dependencies

| Crate            | Purpose                              | Used By      |
| ---------------- | ------------------------------------ | ------------ |
| `lopdf` or `pdf-extract` | PDF text extraction          | Read tool    |
| `base64`         | Image encoding for multimodal        | Read tool    |
| `chrono`         | Task timestamps                      | TaskStore    |
| `fuzzy-matcher`  | Keyword matching for ToolSearch      | ToolSearch   |

### 9.2 Existing Dependencies (already in use)

- `serde_json` - JSON handling
- `tokio` - Async runtime
- `async-trait` - Async trait support
- `anyhow` - Error handling
- `reqwest` - HTTP client
- `scraper` - HTML parsing

---

## 10. Summary

This design brings the zero-nova tool system from 5 basic tools to a full 10-tool ecosystem matching the ideal specifications. The implementation is divided into 7 phases prioritizing foundational changes first, then building new capabilities on top. Key additions include:

- **Edit tool** for precise file modifications
- **Task management** for structured workflow tracking
- **Skill system** for extensible agent behaviors
- **Enhanced Agent** with typed agents, background execution, and git isolation
- **Deferred tool loading** for scalability
- **PDF/image/notebook support** in file reading
