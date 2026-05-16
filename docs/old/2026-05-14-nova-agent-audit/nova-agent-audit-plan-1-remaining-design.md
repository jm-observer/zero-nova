# Plan 1 剩余任务详细设计：conversation 超规模文件拆分

## 时间

2026-05-14

## 背景

`nova-agent-audit-plan-1.md` 原始目标覆盖 3 个超规模文件：

- `crates/nova-agent/src/prompt/mod.rs`
- `crates/nova-agent/src/conversation/service/mod.rs`
- `crates/nova-agent/src/conversation/repository/mod.rs`

当前 `prompt` 模块拆分已基本落地，目录中已有 `builder.rs`、`context.rs`、`routing.rs`、`side_channel.rs`、`templates.rs`、`trimmer.rs`、`types.rs`、`workflow.rs` 等子模块，Plan 1 的剩余主要工作集中在 `conversation` 下：

1. `conversation/service/mod.rs` 仍包含 `SessionService` 类型定义、辅助函数以及大量测试，文件职责仍不够清晰。
2. `conversation/repository/mod.rs` 仍承载 `SqliteSessionRepository` 的全部 SQL 实现，`session_repo.rs` 与 `message_repo.rs` 仍是占位空壳。

## Plan 编号与标题

Plan 1 Remaining：`conversation/service` 与 `conversation/repository` 超规模文件拆分

## 前置依赖

- `prompt/mod.rs` 拆分已完成或至少不再阻塞本任务。
- 不新增依赖。
- 不改变外部 API 语义，优先做文件级迁移与职责归位。

## 本次目标（可验证）

1. `conversation/service/mod.rs` 变成轻量入口文件，仅保留：
   - 子模块声明；
   - `SessionService` 结构体定义；
   - 构造函数与必要 getter；
   - 少量确需跨子模块共享的类型别名。
2. `conversation/service/mod.rs` 中的业务辅助函数迁入明确子模块。
3. `conversation/service/mod.rs` 中的测试迁入独立测试模块，避免主入口文件继续膨胀。
4. `conversation/repository/mod.rs` 不再承载全部 SQL CRUD，按领域拆分到子模块。
5. `conversation/repository/session_repo.rs` 与 `conversation/repository/message_repo.rs` 不再是占位文件，而是承载真实实现。
6. 所有现有调用方保持可编译，外部仍通过 `conversation::repository::SqliteSessionRepository` 与 `conversation::service::SessionService` 使用能力。
7. 完成后运行：
   - `cargo fmt --all`
   - `cargo test -p nova-agent conversation::service`
   - `cargo test -p nova-agent conversation::repository`
   - 如改动影响面扩大，再运行 `cargo clippy --workspace -- -D warnings` 与 `cargo test --workspace`

## 涉及文件

### 必改文件

- `crates/nova-agent/src/conversation/service/mod.rs`
- `crates/nova-agent/src/conversation/service/queries.rs`
- `crates/nova-agent/src/conversation/service/write.rs`
- `crates/nova-agent/src/conversation/service/title.rs`
- `crates/nova-agent/src/conversation/service/helpers.rs`
- `crates/nova-agent/src/conversation/repository/mod.rs`
- `crates/nova-agent/src/conversation/repository/session_repo.rs`
- `crates/nova-agent/src/conversation/repository/message_repo.rs`

### 建议新增文件

- `crates/nova-agent/src/conversation/service/types.rs`
- `crates/nova-agent/src/conversation/service/session_factory.rs`
- `crates/nova-agent/src/conversation/service/skill_bindings.rs`
- `crates/nova-agent/src/conversation/service/tests.rs`
- `crates/nova-agent/src/conversation/repository/run_repo.rs`
- `crates/nova-agent/src/conversation/repository/artifact_repo.rs`
- `crates/nova-agent/src/conversation/repository/permission_repo.rs`
- `crates/nova-agent/src/conversation/repository/audit_repo.rs`
- `crates/nova-agent/src/conversation/repository/diagnostic_repo.rs`
- `crates/nova-agent/src/conversation/repository/workspace_repo.rs`
- `crates/nova-agent/src/conversation/repository/usage_repo.rs`
- `crates/nova-agent/src/conversation/repository/types.rs`

> 说明：新增文件只用于完成已明确的模块拆分，不引入新功能。

---

## 详细设计

## 1. `conversation/service` 拆分设计

### 1.1 当前问题

`service/mod.rs` 当前仍承担以下职责：

| 职责 | 当前位置 | 问题 |
|------|----------|------|
| 模块入口 | `service/mod.rs` | 合理，但混入实现与测试 |
| `SessionService` 类型定义 | `service/mod.rs` | 合理 |
| 构造函数与 repository getter | `service/mod.rs` | 合理 |
| `session_from_index_row` | `service/mod.rs` | 属于 Session 构造/装配，应独立 |
| `merge_skill_bindings` / `normalize_skill_binding` | `service/mod.rs` | 属于 skill binding 合并策略，应独立 |
| 大量单元测试 | `service/mod.rs` | 导致入口文件膨胀，阅读成本高 |

### 1.2 目标模块结构

```text
conversation/service/
├── mod.rs              # 入口、结构体、构造函数
├── commands.rs         # 已存在：命令 DTO / command 相关逻辑
├── events.rs           # 已存在：事件相关类型
├── helpers.rs          # 通用小工具：路径、标题文本等
├── persist.rs          # 持久化辅助：persist_full_session 等
├── queries.rs          # 查询/加载：get/list/load/ensure_history
├── session_factory.rs  # Session 构造函数
├── skill_bindings.rs   # skill binding 合并与规范化
├── tests.rs            # service 层测试
├── title.rs            # 标题生成调度
├── types.rs            # service 内部类型别名/共享类型
└── write.rs            # 写操作：create/append/delete/copy/update
```

### 1.3 `mod.rs` 保留内容

`service/mod.rs` 最终建议只保留：

```rust
pub mod commands;
pub mod events;

mod helpers;
mod persist;
pub mod queries;
mod session_factory;
mod skill_bindings;
mod title;
mod types;
mod write;

#[cfg(test)]
mod tests;

use ...;

#[derive(Clone)]
pub struct SessionService {
    cache: Arc<SessionCache>,
    repository: SqliteSessionRepository,
    title_generator: Arc<dyn TitleGenerator + Send + Sync>,
    loading: Arc<RwLock<LoadingWaiters>>,
}

impl SessionService {
    pub fn new(...) -> Self { ... }
    pub fn new_with_title_generator(...) -> Self { ... }
    pub fn get_repository(&self) -> SqliteSessionRepository { ... }
}
```

### 1.4 `types.rs`

迁移内容：

```rust
type SessionLoadResult = Option<Arc<Session>>;
type LoadingWaiters = HashMap<String, Vec<oneshot::Sender<SessionLoadResult>>>;
```

可见性建议：

- `pub(super)`：仅 service 子模块共享。
- 避免 `pub` 暴露到 crate 外。

### 1.5 `session_factory.rs`

迁移内容：

- `session_from_index_row(...) -> Session`

建议命名：

- 保留 `session_from_index_row`，减少调用方改动。
- 后续如继续整理，可再改为 `build_indexed_session`。

调用方调整：

- `queries.rs` 中的 `super::session_from_index_row(...)` 改为 `super::session_factory::session_from_index_row(...)`。

### 1.6 `skill_bindings.rs`

迁移内容：

- `merge_skill_bindings(existing, incoming)`
- `normalize_skill_binding(skill)`
- 当前针对 `merge_skill_bindings` 的 3 个单元测试可一起迁入本文件，或统一放入 `tests.rs`。

可见性建议：

```rust
pub(super) fn merge_skill_bindings(...)
fn normalize_skill_binding(...)
```

调用方调整：

- `write.rs` 中如使用 `merge_skill_bindings`，改为 `super::skill_bindings::merge_skill_bindings`。

### 1.7 `tests.rs`

迁移内容：

- 当前 `service/mod.rs` 中 `#[cfg(test)] mod tests` 整体迁入 `service/tests.rs`。

模块声明：

```rust
#[cfg(test)]
mod tests;
```

测试导入调整：

- `use super::SessionService;` 保持可用。
- 原本测试访问 `super::merge_skill_bindings` 改为 `super::skill_bindings::merge_skill_bindings`。
- 原本测试访问 `super::TITLE_GENERATION_TIMEOUT_MS` 如常量仍由 `service/mod.rs` re-export 或直接改为 `super::title::TITLE_GENERATION_TIMEOUT_MS`。

### 1.8 标题生成常量归属

当前标题生成常量位于 `service/mod.rs`：

- `TITLE_MIN_USER_MESSAGES_FIRST_ATTEMPT`
- `TITLE_MIN_USER_MESSAGES_SECOND_ATTEMPT`
- `TITLE_MAX_ATTEMPTS`
- `TITLE_MIN_TOTAL_CHARS`
- `TITLE_GENERATION_TIMEOUT_MS`

建议迁移到 `title.rs`，因为它们只服务标题生成逻辑。

可见性：

- 若测试需要访问，使用 `pub(super)`。
- 若 crate 外无调用，不要 `pub`。

`DEFAULT_SESSION_TITLE` 建议保留在 `mod.rs` 或迁入 `types.rs`：

- 若 `write.rs` 使用频繁，可设为 `pub(super) const DEFAULT_SESSION_TITLE`。
- 不作为公共 API 暴露。

---

## 2. `conversation/repository` 拆分设计

### 2.1 当前问题

`repository/mod.rs` 当前集中实现了以下 SQL 操作：

| 类型 | 方法 |
|------|------|
| Session | `save_session`、`update_session_runtime_control`、`load_session_meta`、`load_session`、`list_sessions`、`find_latest_session_by_agent`、`touch_session`、`delete_session` |
| Message | `save_message` 以及 `load_session` 内部的消息加载逻辑 |
| Run | `create_run`、`update_run_usage`、`update_run_status`、`create_run_step`、`update_run` |
| Artifact | `create_artifact`、`list_artifacts` |
| Permission | `create_permission_request`、`resolve_permission_request`、`list_permission_requests` |
| Audit | `create_audit_log`、`list_audit_logs` |
| Diagnostic | `create_diagnostic_issue`、`clear_diagnostics`、`list_diagnostics` |
| Workspace restore | `save_workspace_restore_state`、`get_workspace_restore_state`、`get_last_workspace_restore_state` |
| Usage | `sum_session_usage`、`count_usage_quality` |

这些方法都挂在 `SqliteSessionRepository` 上是可以接受的，但实现不应全部堆在 `mod.rs`。

### 2.2 目标模块结构

```text
conversation/repository/
├── mod.rs              # 入口、结构体、共享 imports、re-export
├── artifact_repo.rs    # artifact CRUD
├── audit_repo.rs       # audit log CRUD
├── diagnostic_repo.rs  # diagnostic CRUD
├── message_repo.rs     # message CRUD + 消息反序列化辅助
├── permission_repo.rs  # permission request CRUD
├── run_repo.rs         # run / run_step CRUD
├── session_repo.rs     # session CRUD + session 加载
├── types.rs            # SessionRow / 聚合结果等仓储内部类型
├── usage_repo.rs       # usage 聚合查询
└── workspace_repo.rs   # workspace restore state CRUD
```

### 2.3 `mod.rs` 保留内容

```rust
pub mod artifact_repo;
pub mod audit_repo;
pub mod diagnostic_repo;
pub mod message_repo;
pub mod permission_repo;
pub mod run_repo;
pub mod session_repo;
pub mod types;
pub mod usage_repo;
pub mod workspace_repo;

#[derive(Clone)]
pub struct SqliteSessionRepository {
    pub(super) pool: sqlx::SqlitePool,
}

impl SqliteSessionRepository {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }
}

pub use types::{SessionUsageAggregate, UsageQualityCounts};
```

`pool` 字段建议改成 `pub(super)`，使各 repo 子模块能访问，但不暴露给 repository 模块外。

### 2.4 `types.rs`

迁移内容：

```rust
pub(super) type SessionRow = (String, String, String, i64, i64, super::super::control::ControlState);

#[derive(Debug, Clone)]
pub struct SessionUsageAggregate { ... }

#[derive(Debug, Clone)]
pub struct UsageQualityCounts { ... }
```

说明：

- `SessionRow` 仅 repository 与 service 内部使用，可保持 `pub(super)` 或 `pub(crate)`，取决于 `service` 是否直接引用类型别名。
- `SessionUsageAggregate` 与 `UsageQualityCounts` 若已有外部调用，继续通过 `repository::SessionUsageAggregate` re-export。

### 2.5 `session_repo.rs`

迁移方法：

- `save_session`
- `update_session_runtime_control`
- `load_session_meta`
- `load_session`
- `list_sessions`
- `find_latest_session_by_agent`
- `touch_session`
- `delete_session`

`load_session` 当前同时加载 session meta 与 messages。为避免一次性改变行为，第一步仍保留公共方法签名不变，但内部委托 `message_repo` 辅助函数：

```rust
let history = self.load_messages_for_session(id).await?;
```

### 2.6 `message_repo.rs`

迁移方法/辅助函数：

- `save_message`
- `load_messages_for_session`
- 消息 role 字符串与 `Role` 的转换辅助
- content / metadata 的 JSON 反序列化辅助

建议仅将 `save_message` 保持为外部可调用方法，其余辅助为 `pub(super)` 或私有。

### 2.7 其他 repo 子模块

按方法归属迁移：

| 文件 | 方法 |
|------|------|
| `run_repo.rs` | `create_run`、`update_run_usage`、`update_run_status`、`create_run_step`、`update_run`、`list_runs`、`get_run` |
| `artifact_repo.rs` | `create_artifact`、`list_artifacts` |
| `permission_repo.rs` | `create_permission_request`、`resolve_permission_request`、`list_permission_requests` |
| `audit_repo.rs` | `create_audit_log`、`list_audit_logs` |
| `diagnostic_repo.rs` | `create_diagnostic_issue`、`clear_diagnostics`、`list_diagnostics` |
| `workspace_repo.rs` | `save_workspace_restore_state`、`get_workspace_restore_state`、`get_last_workspace_restore_state` |
| `usage_repo.rs` | `sum_session_usage`、`count_usage_quality` |

每个文件都用独立 `impl SqliteSessionRepository` 块承载方法，避免引入 trait 或额外抽象。

---

## 3. 数据流与接口保持策略

### 3.1 Service 层数据流不变

拆分前：

```text
SessionService -> SqliteSessionRepository -> SQLite
```

拆分后：

```text
SessionService -> SqliteSessionRepository impl blocks in submodules -> SQLite
```

外部对象关系不变，`SessionService` 仍持有一个 `SqliteSessionRepository`。

### 3.2 Repository 对外类型不变

保持以下入口稳定：

- `SqliteSessionRepository::new(pool)`
- `SqliteSessionRepository::{save_session, save_message, load_session, ...}`
- `SessionUsageAggregate`
- `UsageQualityCounts`

拆分只改变源码位置，不改变调用方签名。

### 3.3 避免过度抽象

本任务不引入：

- repository trait；
- generic storage abstraction；
- DTO 重命名；
- SQL query builder；
- 新依赖；
- 行为兼容层。

原因：本任务目标是消除超规模文件，不是重构持久化架构。

---

## 4. 执行步骤

### Step 1：拆 `conversation/service/mod.rs` 的非核心内容

1. 新建 `service/types.rs`，迁入 `SessionLoadResult` 与 `LoadingWaiters`。
2. 新建 `service/session_factory.rs`，迁入 `session_from_index_row`。
3. 新建 `service/skill_bindings.rs`，迁入 `merge_skill_bindings` 与 `normalize_skill_binding`。
4. 将标题生成常量迁入 `title.rs`。
5. 将 `#[cfg(test)] mod tests` 迁入 `service/tests.rs`。
6. 调整 imports 与 `super::...` 路径。
7. 运行 `cargo fmt --all` 与 `cargo test -p nova-agent conversation::service`。

### Step 2：拆 `conversation/repository/mod.rs` 的 Session / Message

1. 新建 `repository/types.rs`，迁入聚合类型与 `SessionRow`。
2. 将 `pool` 字段改为 `pub(super)`。
3. 将 session 相关方法迁入 `session_repo.rs`。
4. 将 message 相关方法和消息加载辅助迁入 `message_repo.rs`。
5. 保持 `load_session` 签名不变。
6. 运行 `cargo fmt --all` 与 `cargo test -p nova-agent conversation::repository`。

### Step 3：拆 `conversation/repository/mod.rs` 的剩余领域

1. 迁移 Run 方法到 `run_repo.rs`。
2. 迁移 Artifact 方法到 `artifact_repo.rs`。
3. 迁移 Permission 方法到 `permission_repo.rs`。
4. 迁移 Audit 方法到 `audit_repo.rs`。
5. 迁移 Diagnostic 方法到 `diagnostic_repo.rs`。
6. 迁移 Workspace restore 方法到 `workspace_repo.rs`。
7. 迁移 Usage 聚合方法到 `usage_repo.rs`。
8. 运行 `cargo fmt --all` 与 repository/service 相关测试。

### Step 4：收敛入口文件

1. 检查 `service/mod.rs` 是否只剩入口与类型定义。
2. 检查 `repository/mod.rs` 是否只剩入口与 `SqliteSessionRepository` 定义。
3. 删除占位注释。
4. 检查是否存在重复定义或未使用 re-export。
5. 运行最终验证命令。

---

## 5. 测试案例

### 5.1 正常路径

1. 创建会话：`SessionService::create` 后可从 repository 读回。
2. 追加消息：`append_message` 后 `get_with_history` 能返回完整 history。
3. 会话索引加载：`load_session_index` 只加载 metadata，后续 `ensure_session_history_loaded` 再加载消息。
4. 标题生成：达到触发条件后能更新标题状态并持久化。
5. session copy：复制会话后新会话 history 与控制状态符合原逻辑。
6. run / artifact / permission / audit / diagnostic / workspace 相关 CRUD 仍能读写。
7. usage 聚合查询结果与拆分前一致。

### 5.2 边界路径

1. 空 system prompt 创建会话时不生成 system message。
2. `get` 不存在 session 返回 `Ok(None)`。
3. `ensure_session_history_loaded` 并发冷加载同一个 session 时只执行一次实际加载。
4. `copy_session` 的 `truncate_index` 越界时保留完整 history。
5. skill binding 合并时同一 `skill_id` 去重并覆盖旧字段。
6. permission request 列表分页/状态过滤保持原 SQL 行为。
7. workspace restore state 查询不存在记录时返回 `Ok(None)`。

### 5.3 异常路径

1. 消息 content JSON 反序列化失败时返回 `anyhow::Result` 错误。
2. metadata JSON 反序列化失败时返回错误，不吞掉异常。
3. title generation 超时后状态从 pending 转为 failed，不阻塞 append。
4. repository SQL 执行失败时错误向上传播。
5. 删除不存在 session 不应导致 panic。

---

## 6. 验收标准

1. `crates/nova-agent/src/conversation/service/mod.rs` 不再包含大段测试与业务辅助函数。
2. `crates/nova-agent/src/conversation/repository/session_repo.rs` 与 `message_repo.rs` 承载真实实现，不再是 placeholder。
3. `crates/nova-agent/src/conversation/repository/mod.rs` 不再包含全部 SQL CRUD。
4. 不新增依赖。
5. 不引入 `unwrap/expect` 到非测试代码。
6. 不改变公开调用语义。
7. `cargo fmt --all` 通过。
8. `cargo test -p nova-agent conversation::service` 通过。
9. `cargo test -p nova-agent conversation::repository` 通过。
10. 若全 workspace 检查被要求，则 `cargo clippy --workspace -- -D warnings` 与 `cargo test --workspace` 通过。

---

## 7. 风险与规避

| 风险 | 影响 | 规避 |
|------|------|------|
| 子模块可见性调整导致编译失败 | 中 | 优先使用 `pub(super)`，按编译错误最小化开放范围 |
| 测试迁移后 `super::` 路径失效 | 中 | 迁移测试时同步调整 imports，不改测试断言 |
| `load_session` 拆出 message 加载后行为变化 | 高 | 保持 `load_session` 签名与返回 tuple 完全不变 |
| SQL 迁移时遗漏 `Context` 或 row 转换逻辑 | 中 | 每个方法整体搬迁，不重写 SQL |
| 拆分过度引入 trait/抽象 | 中 | 明确禁止新增 repository trait，仅多文件 `impl` |
| 文件数量增加但职责仍不清晰 | 中 | 按表归属拆分，每个文件只放一个领域的 CRUD |

---

## 8. 非目标

本设计不处理以下内容：

1. 不拆 crate。
2. 不改 SQLite schema。
3. 不重命名 public API。
4. 不将 repository 抽象成 trait。
5. 不优化 SQL 性能。
6. 不调整 title generation 业务规则。
7. 不处理 Plan 2 的同步/异步双写、ToolRegistry 双锁等问题。
8. 不处理 Plan 3 的 crate monolith、ToolDefinition 重复定义等长期问题。
