# Plan 1: Session 父子模型 + SQLite 持久化

## 前置依赖

无。

## 任务目标

在 `conversation::Session` 上加 3 个父子关系字段，在 SQLite `sessions` 表上加 2 个对应列与配套 forward-only migration，扩 `SqliteSessionRepository` 的读写路径携带这两列。

完成后：

- 内存里 `Session` 能表达「我父亲是谁、是从父亲哪条 ToolUse 派生的、我有哪些孩子」
- SQLite `sessions` 表 schema 加 `parent_session_id TEXT` + `parent_tool_use_id TEXT` 两列，老库通过 `ALTER TABLE` 加列、值为 NULL（语义：无父，与现状等价）
- `save_session` / `load_session` / `load_session_meta` / `list_sessions` 全部往返携带这两列
- **本 Plan 不触碰** `AgentTool` 子 Agent 派生路径（Plan 2 做），也不动 `AgentApplicationImpl` 对外 API（Plan 3 做）

## 执行范围

**必须修改**：

| 文件 | 改动 |
|------|------|
| `crates/nova-agent/src/conversation/session.rs` | `Session` struct 增加 `parent_session_id: Option<String>`、`parent_tool_use_id: Option<String>`、`child_session_ids: RwLock<Vec<String>>` 三个字段；增加 `pub async fn push_child(&self, child_id: &str)` 方法 |
| `crates/nova-agent/src/conversation/sqlite_manager.rs` | `run_migrations` 末尾增调 `self.migrate_sessions_parent_child_columns().await?;`；新增该方法照 `migrate_sessions_runtime_control_column` 模板 PRAGMA 检测、缺则 `ALTER TABLE sessions ADD COLUMN parent_session_id TEXT` 与 `parent_tool_use_id TEXT`；同时为 `parent_session_id` 加索引 `CREATE INDEX IF NOT EXISTS idx_sessions_parent ON sessions(parent_session_id)` |
| `crates/nova-agent/src/conversation/repository/mod.rs` 或 `repository/types.rs`（`SessionRow` 定义处） | `SessionRow` 增加 `parent_session_id: Option<String>` 与 `parent_tool_use_id: Option<String>` 字段；`parse_session_row` 函数 SELECT/读取增加两列 |
| `crates/nova-agent/src/conversation/repository/session_repo.rs` | `save_session` 签名增加 `parent_session_id: Option<&str>` 与 `parent_tool_use_id: Option<&str>` 参数；INSERT/UPDATE SQL 增加两列；`load_session_meta` / `load_session` / `list_sessions` / `find_latest_session_by_agent` 的 SELECT 增加两列并填入 `SessionRow`；新增 `pub async fn list_child_session_ids(&self, parent_id: &str) -> Result<Vec<String>>` 方法（执行 `SELECT id FROM sessions WHERE parent_session_id = ? ORDER BY created_at`） |
| Session 在内存与持久化之间的 cache/构造点（`conversation/cache.rs` 与 `service::write` / `service::queries` 中构造 `Session` 的位置） | 构造 `Session` 时把 `parent_session_id` / `parent_tool_use_id` 从 row 透传；初始 `child_session_ids = RwLock::new(Vec::new())`，加载完整 Session 时通过 `repo.list_child_session_ids` 回填 |
| `crates/nova-agent/src/conversation/service/write.rs` | `Session` 构造点（`copy_session` 等）随 struct 字段对齐；不引入新逻辑 |

**允许修改**：

- `Cargo.toml`（根）：不在本 Plan 升 tag，留到 Plan 3
- 测试文件：为新增字段补构造帮助方法

**禁止修改**：

- `AgentTool::run_subagent` 路径（Plan 2）
- `AgentApplicationImpl` 公开签名（Plan 3）
- `Message` / `ContentBlock` / `MessageMetadata` 任何字段（本设计不需要）
- `runtime_control` / `messages` / `runs` 等其它表的 schema
- `delete_session` 行为（本 Plan 暂保留现状：直接 DELETE，子 Session 不会自动 cascade，因为 sessions 表没有自引用 FK；Plan 3 引入 `delete_session_tree` 显式 API 并把 `delete_session` 改为「拒绝有子」）

## Agent 执行步骤

### 步骤 1：扩 `Session` struct（`conversation/session.rs`）

```rust
pub struct Session {
    pub control: RwLock<ControlState>,
    pub id: String,
    pub name: RwLock<String>,
    pub history: RwLock<Vec<Message>>,
    pub created_at: i64,
    pub updated_at: AtomicI64,
    pub chat_lock: Mutex<()>,
    pub cancellation_token: RwLock<Option<CancellationToken>>,
    pub title_state: RwLock<TitleState>,
    // === Plan 1 新增 ===
    pub parent_session_id: Option<String>,
    pub parent_tool_use_id: Option<String>,
    pub child_session_ids: RwLock<Vec<String>>,
}

impl Session {
    pub async fn push_child(&self, child_id: &str) {
        let mut children = self.child_session_ids.write().await;
        if !children.iter().any(|id| id == child_id) {
            children.push(child_id.to_string());
        }
        self.touch_updated_at();
    }

    pub async fn get_child_ids(&self) -> Vec<String> {
        self.child_session_ids.read().await.clone()
    }
}
```

> `parent_session_id` / `parent_tool_use_id` 在 Session 创建后不再变更（无 setter），因此用 `Option<String>` 而非 `RwLock`。

### 步骤 2：SQLite migration（`conversation/sqlite_manager.rs`）

照 `migrate_sessions_runtime_control_column` 模板新增：

```rust
async fn migrate_sessions_parent_child_columns(&self) -> Result<()> {
    let columns = sqlx::query("PRAGMA table_info(sessions)")
        .fetch_all(&self.pool)
        .await
        .context("Failed to inspect sessions table schema")?;

    let mut has_parent_session_id = false;
    let mut has_parent_tool_use_id = false;
    for column in columns {
        let name: String = Row::get(&column, "name");
        if name == "parent_session_id" { has_parent_session_id = true; }
        else if name == "parent_tool_use_id" { has_parent_tool_use_id = true; }
    }

    if !has_parent_session_id {
        sqlx::query("ALTER TABLE sessions ADD COLUMN parent_session_id TEXT")
            .execute(&self.pool).await
            .context("Failed to add parent_session_id column to sessions table")?;
    }
    if !has_parent_tool_use_id {
        sqlx::query("ALTER TABLE sessions ADD COLUMN parent_tool_use_id TEXT")
            .execute(&self.pool).await
            .context("Failed to add parent_tool_use_id column to sessions table")?;
    }

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_sessions_parent ON sessions(parent_session_id)")
        .execute(&self.pool).await
        .context("Failed to create idx_sessions_parent index")?;

    Ok(())
}
```

在 `run_migrations()` 末尾调用：

```rust
self.migrate_sessions_parent_child_columns().await?;
```

> 对全新库不影响 `CREATE TABLE` 语句——保持现状不动；老库由 ALTER 增列。两路终态等价。

### 步骤 3：扩 `SessionRow` 与 `parse_session_row`

```rust
pub struct SessionRow {
    pub id: String,
    pub title: String,
    pub agent_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub runtime_control: ControlState,
    // === Plan 1 新增 ===
    pub parent_session_id: Option<String>,
    pub parent_tool_use_id: Option<String>,
}
```

`parse_session_row` 读取两个 `Option<String>` 列；对老库 PRAGMA 加列后值为 NULL，自然 `None`。

### 步骤 4：改 `SqliteSessionRepository` 读写

**`save_session` 签名扩展**（SQL 完整形式）：

```rust
pub async fn save_session(
    &self,
    id: &str,
    title: &str,
    agent_id: &str,
    created_at: i64,
    updated_at: i64,
    runtime_control: &ControlState,
    parent_session_id: Option<&str>,
    parent_tool_use_id: Option<&str>,
) -> Result<()> {
    // 注意：ON CONFLICT 子句中 `parent_session_id` / `parent_tool_use_id` 故意不出现，
    // 即子 Session 创建后这两列永不被 UPDATE 覆盖（一次写定语义）。
    sqlx::query(
        "INSERT INTO sessions \
         (id, title, agent_id, created_at, updated_at, runtime_control, \
          parent_session_id, parent_tool_use_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET \
            title = excluded.title, \
            agent_id = excluded.agent_id, \
            updated_at = excluded.updated_at, \
            runtime_control = excluded.runtime_control"
    )
    .bind(id).bind(title).bind(agent_id).bind(created_at).bind(updated_at)
    .bind(runtime_control_json)
    .bind(parent_session_id).bind(parent_tool_use_id)
    .execute(&self.pool).await?;
    Ok(())
}
```

> 与初稿差异：①SQL 字符串完整闭合（初稿末尾缺 `)`/`"`，已修正——见总览「已收敛的待澄清点」#7）；②ON CONFLICT 不含 parent_* 列，确保「一次写定」语义。

**所有 SELECT 语句**（`load_session_meta` / `load_session` / `list_sessions` / `find_latest_session_by_agent`）：在列表中追加 `, parent_session_id, parent_tool_use_id`，row.get 读取并填入 `SessionRow`。

**新增 `list_child_session_ids`**：

```rust
pub async fn list_child_session_ids(&self, parent_id: &str) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT id FROM sessions WHERE parent_session_id = ? ORDER BY created_at")
        .bind(parent_id)
        .fetch_all(&self.pool)
        .await?;
    Ok(rows.into_iter().map(|r| r.get::<String, _>("id")).collect())
}
```

### 步骤 5：Session 构造点对齐

凡构造 `Session { ... }` 的地方（`cache.rs`、`service/write.rs::copy_session`、`service` 内首次从 row 实例化、测试 helper 等）——补齐：

- `parent_session_id: row.parent_session_id` / 或 `None`（新建无父 Session）
- `parent_tool_use_id: row.parent_tool_use_id` / 或 `None`
- `child_session_ids: RwLock::new(repo.list_child_session_ids(&id).await.unwrap_or_default())`（仅在 load 路径回填；create 路径初始为空）

> `copy_session`（write.rs:121）拷贝出的新 Session：`parent_session_id = None`、`parent_tool_use_id = None`、`child_session_ids = Vec::new()`——副本是独立根 Session，不继承父子关系。
>
> **前提**：`copy_session` 是"用户克隆为新对话"语义。若未来出现需要保留父子关系的内部复制场景，必须另起 API、不要复用本路径（见总览「已收敛的待澄清点」#1）。

### 步骤 6：调用方修改

所有调 `repo.save_session(...)` 的地方需补两个参数：

- 新建无父 Session（既有路径）：传 `None, None`
- 新建子 Session（Plan 2 才实现）：在 Plan 1 范围内**不出现**

`grep -n "save_session(" crates/nova-agent/src/` 找出所有调用点，逐一补 `None, None`。

## 目标数据结构 / 接口契约

```rust
// session.rs
pub struct Session {
    pub parent_session_id: Option<String>,    // 创建时一次写定；None = 根 Session
    pub parent_tool_use_id: Option<String>,   // 创建时一次写定；定位父 history 哪条 ToolUse 派生本 Session
    pub child_session_ids: RwLock<Vec<String>>, // append-only；load 时从 repo 回填
}
impl Session {
    pub async fn push_child(&self, child_id: &str);
    pub async fn get_child_ids(&self) -> Vec<String>;
}

// SqliteSessionRepository
pub async fn save_session(
    &self, id, title, agent_id, created_at, updated_at, runtime_control,
    parent_session_id: Option<&str>, parent_tool_use_id: Option<&str>,
) -> Result<()>;
pub async fn list_child_session_ids(&self, parent_id: &str) -> Result<Vec<String>>;

// SessionRow（repository 内部 + 跨边界返回）
pub struct SessionRow {
    pub parent_session_id: Option<String>,
    pub parent_tool_use_id: Option<String>,
    // ... 其它字段不变
}
```

## 行为规则

| 输入场景 | 输出 / 状态变化 |
|---------|---------------|
| 新建无父 Session（`save_session(..., None, None)`） | DB 行 parent_* 列为 NULL；内存 Session `parent_session_id=None`、`child_session_ids=[]` |
| 加载老库（PRAGMA 加列前已存在的行） | parent_* 列被 ALTER 加为 NULL；语义等同"无父 Session"，行为与改造前等价 |
| 调 `list_child_session_ids(parent_id)` 而 DB 中无任何子 | 返回 `Vec::new()`（不报错） |
| 同一子 ID 重复 `push_child` | 内存去重（已有则跳过），不影响 DB |
| `save_session` UPDATE 路径（同 id 二次写） | parent_* 列**不被覆写**（CONFLICT DO UPDATE 不包含这两列），保持首写值 |
| `delete_session(parent_id)`（旧 API） | 仅删 parent 行 + 其 messages；**子 Session 行变成孤儿**（parent_session_id 指向已删除 id）—— Plan 3 引入 `delete_session_tree` 与"拒绝删有子" |

## 禁止事项

- 不修改 `AgentTool::run_subagent` 或任何子 Agent 派生现场
- 不暴露 `AgentApplicationImpl::get_session_tree` 等对外 API
- 不引入 `delete_session` cascade 或拒绝逻辑
- 不修改 `Message` / `ContentBlock` / `MessageMetadata` schema
- 不动 git tag（Plan 3 统一升 v0.3.5 + push tag）
- 不引入新依赖

## 测试要求

| 文件 | 测试用例 | 输入 | 断言 |
|------|---------|------|------|
| `conversation/repository/session_repo.rs`（`#[cfg(test)] mod`） | `save_load_root_session` | 调 `save_session(..., None, None)` 后 `load_session_meta` | `parent_session_id == None` 且 `parent_tool_use_id == None` |
| 同上 | `save_load_child_session` | 调 `save_session(..., Some("parent_id"), Some("toolu_xyz"))` 后 `load_session_meta` | 两字段读回与写入一致 |
| 同上 | `list_child_session_ids_returns_children_in_order` | 同 parent_id 写入 3 个 child（不同 created_at），再调 `list_child_session_ids(parent_id)` | 返回 3 个 id，按 created_at 升序 |
| 同上 | `list_child_session_ids_empty_when_no_children` | 写 1 个根 Session 后调 `list_child_session_ids(root_id)` | 返回 `Vec::new()` |
| 同上 | `upsert_does_not_overwrite_parent_columns` | save 一次 `Some("p1"), Some("t1")`，再 save 同 id 但 parent 参数传 `None, None` | 第二次 load 仍读回 `Some("p1"), Some("t1")` |
| `conversation/session.rs`（`#[cfg(test)] mod`） | `push_child_dedups` | 同一 child_id 重复 push_child 3 次 | `get_child_ids().len() == 1` |
| `conversation/sqlite_manager.rs`（`#[cfg(test)] mod`） | `migration_idempotent_on_existing_db` | 模拟"老库无 parent_* 列"，调 `run_migrations` 两次 | 不报错；PRAGMA 显示有 parent_session_id / parent_tool_use_id 两列 |

**验证命令**（CLAUDE.md 修复流程）：

```bash
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test --workspace -p nova-agent conversation::
```

## 完成条件

- [ ] `Session` 加 3 字段、`push_child` / `get_child_ids` 方法实现
- [ ] SQLite migration 增 2 列 + 索引，老库 ALTER 路径与全新库 CREATE 路径终态等价
- [ ] `SessionRow` 与 `parse_session_row` 携带新两列
- [ ] `SqliteSessionRepository::save_session` 签名扩展、四个 SELECT 路径全部携带新两列；`list_child_session_ids` 实现
- [ ] 全 workspace `cargo clippy / fmt / test` 全绿
- [ ] 所有调 `save_session(...)` 的旧路径补 `None, None`、零回归
- [ ] 本 Plan 范围内**没有**触碰 `AgentTool` / `AgentApplicationImpl` 公开签名
- [ ] 本 Plan 的总览状态在 `session-parent-child-tree.md` 改为「已完成」并 commit
