# Plan 1: 完善 nova-agent 模块导出与内部引用修复

## Plan 编号与标题
- Plan 1: 完善 nova-agent 模块导出与内部引用修复

## 前置依赖
- 无

## 本次目标
- 解决 `app/` 和 `conversation/` 目录下 `mod.rs` 与 `lib.rs` 共存的模块冲突。
- 在 `nova-agent/src/lib.rs` 中正确声明 `pub mod app;` 和 `pub mod conversation;`。
- 将所有内部 `use nova_core::*` 替换为 `use crate::*`，`use nova_conversation::*` 替换为 `use crate::conversation::*`。
- 确保 `cargo check -p nova-agent` 单独通过。

## 涉及文件

### 模块入口修复
- **删除**: `crates/nova-agent/src/app/lib.rs`（内容合并到 mod.rs）
- **修改**: `crates/nova-agent/src/app/mod.rs`（合并 lib.rs 的模块声明和 re-export）
- **重命名**: `crates/nova-agent/src/conversation/lib.rs` → `crates/nova-agent/src/conversation/mod.rs`
- **修改**: `crates/nova-agent/src/lib.rs`（添加 `pub mod app;` 和 `pub mod conversation;`）

### 内部引用修复（`nova_core::` → `crate::`）
- `crates/nova-agent/src/app/application.rs`
- `crates/nova-agent/src/app/bootstrap.rs`
- `crates/nova-agent/src/app/conversation_service.rs`
- `crates/nova-agent/src/app/types.rs`
- `crates/nova-agent/src/app/agent_workspace_service.rs`
- `crates/nova-agent/src/app/snapshot_assembler.rs`
- `crates/nova-agent/src/conversation/repository.rs`
- `crates/nova-agent/src/conversation/service.rs`
- `crates/nova-agent/src/conversation/session.rs`

### 内部引用修复（`nova_conversation::` → `crate::conversation::`）
- `crates/nova-agent/src/app/bootstrap.rs`
- `crates/nova-agent/src/app/conversation_service.rs`
- `crates/nova-agent/src/app/agent_workspace_service.rs`
- `crates/nova-agent/src/app/snapshot_assembler.rs`
- `crates/nova-agent/src/app/mod.rs`（合并后的 re-export）

### Cargo.toml 补充
- **修改**: `crates/nova-agent/Cargo.toml`（添加 `sqlx` 依赖，conversation 模块需要）

## 详细设计

### 1. 解决 app/ 模块冲突

当前 `app/mod.rs` 缺少 `agent_workspace_service` 和 `snapshot_assembler` 模块声明，而 `app/lib.rs` 包含完整声明和 re-export。

**操作**: 将 `app/lib.rs` 的完整内容合并到 `app/mod.rs`，删除 `app/lib.rs`。合并后的 `app/mod.rs`:

```rust
pub mod agent_workspace_service;
pub mod application;
pub mod bootstrap;
pub mod conversation_service;
pub mod snapshot_assembler;
pub mod types;

pub use agent_workspace_service::AgentWorkspaceService;
pub use application::{AgentApplication, AgentApplicationImpl};
pub use bootstrap::{build_application, BootstrapOptions};
pub use conversation_service::ConversationService;
pub use types::{AppAgent, AppEvent, AppMessage, AppSession};

// re-export: 保持 app 模块对外接口不变
pub use crate::conversation::SessionService;
pub use crate::event::AgentEvent;
pub use crate::message::ContentBlock;
pub use crate::provider::LlmClient;
```

### 2. 解决 conversation/ 模块入口

当前 `conversation/` 下仅有 `lib.rs`，作为子模块应使用 `mod.rs`。

**操作**: 重命名 `conversation/lib.rs` → `conversation/mod.rs`，修复引用:

```rust
pub mod cache;
pub mod control;
pub mod model;
pub mod repository;
pub mod service;
pub mod session;
pub mod sqlite_manager;

pub use cache::SessionCache;
pub use crate::message::{ContentBlock, Message, Role};  // nova_core → crate
pub use repository::SqliteSessionRepository;
pub use service::SessionService;
pub use session::{Session, SessionSummary};
pub use sqlite_manager::SqliteManager;
```

### 3. 更新 lib.rs 模块声明

在 `nova-agent/src/lib.rs` 添加:

```rust
pub mod app;
pub mod conversation;
```

### 4. 批量替换内部引用

所有 `app/` 和 `conversation/` 下源文件执行替换:

| 旧路径 | 新路径 |
|--------|--------|
| `use nova_core::agent::` | `use crate::agent::` |
| `use nova_core::agent_catalog::` | `use crate::agent_catalog::` |
| `use nova_core::config::` | `use crate::config::` |
| `use nova_core::event::` | `use crate::event::` |
| `use nova_core::message::` | `use crate::message::` |
| `use nova_core::prompt::` | `use crate::prompt::` |
| `use nova_core::provider::` | `use crate::provider::` |
| `use nova_core::skill::` | `use crate::skill::` |
| `use nova_core::tool::` | `use crate::tool::` |
| `use nova_conversation::` | `use crate::conversation::` |
| `nova_core::` (类型限定路径) | `crate::` |

### 5. Cargo.toml 补充

`nova-agent/Cargo.toml` 添加:
```toml
sqlx = { workspace = true }
```
conversation 模块的 `repository.rs` 和 `sqlite_manager.rs` 依赖 sqlx。

## 测试案例

- 正常路径:
  - `cargo check -p nova-agent` 编译通过，无 unresolved import。
  - `cargo test -p nova-agent` 所有既有测试通过。
- 边界条件:
  - `app` 模块的 re-export（`AgentApplication`、`build_application` 等）可被外部 crate 正确引用。
  - `conversation` 模块的 `SessionService` 可通过 `nova_agent::conversation::SessionService` 和 `nova_agent::app::SessionService` 两种路径访问。
- 异常场景:
  - 不存在循环依赖（app → conversation 通过 crate 根中转）。
