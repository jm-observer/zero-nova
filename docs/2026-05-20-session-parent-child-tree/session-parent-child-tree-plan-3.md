# Plan 3: get_session_tree API + delete_session_tree + v0.3.5 tag

## 前置依赖

Plan 1（Session 父子模型 + 持久化）、Plan 2（AgentTool 子 Session 路径改造）。

## 任务目标

在 `AgentApplicationImpl` 暴露三件事：

1. `list_child_sessions(parent_id) -> Vec<SessionSummary>`——轻量列子 Session 元数据，不拉 history
2. `get_session_tree(root_id, max_depth) -> SessionTree`——深度优先返回完整父子树（每节点含 history）
3. `delete_session_tree(root_id)`——级联删根及所有后代；同时把既有 `delete_session(id)` 行为改为「有子时拒绝删除并返回错误」，避免意外造成孤儿子 Session

完成后：

- 下游 zero 仓 `SessionFlagTool` 调 `get_session_tree(parent_id, 8)` 一次拉到错误会话整棵树（含每节点 LLM raw req/resp），即可整树 snapshot 进 run-state 离线复盘
- 控制台可调 `list_child_sessions` 做"展开/折叠"按需加载（暂未启用，预留接口）
- `delete_session` 的现有调用者拿到「拒绝」错误时可显式选择 `delete_session_tree` 走级联

最后 commit 并升 git tag `v0.3.5`、push origin。

## 执行范围

**必须修改**：

| 文件 | 改动 |
|------|------|
| `crates/nova-agent/src/app/application.rs` | `impl AgentApplicationImpl` 末尾新增 `list_child_sessions` / `get_session_tree` / `delete_session_tree` 三个 pub 方法；`delete_session` 现状改为「先 `list_child_session_ids` 检查；非空则返回 `Err(anyhow!("session {id} has {n} child sessions; use delete_session_tree"))`」 |
| `crates/nova-agent/src/app/mod.rs` 或新建 `app/session_tree.rs` | 新增 `SessionTree` 类型定义；`mod.rs` 中 `pub use` 导出 |
| `crates/nova-agent/src/lib.rs` | 顶层 `pub use` 增加 `SessionTree`、`SessionSummary`（若未导出） |
| `crates/nova-agent/src/conversation/service/queries.rs`（或 `service` 内合适位置） | 新增 `pub async fn list_child_session_summaries(parent_id: &str) -> Result<Vec<SessionSummary>>`，内部调 `repo.list_child_session_ids` 后逐个 `load_session_meta` 转 `SessionSummary` |
| `crates/nova-agent/src/conversation/service/write.rs` | `pub async fn delete_session_tree(root_id: &str) -> Result<usize>`：DFS 收集所有后代 id 后单 transaction 删；返回删除数 |
| **根** `Cargo.toml` / 各 crate `Cargo.toml` | 版本号准备升 v0.3.5（按项目现有版本管理方式；当前 workspace.package.version 为 0.1.0 而 git tag 是 v0.3.x，以 git tag 为准） |

**允许修改**：

- `docs/design/system-overview.md`：「Session 模型」段补"父子树"小节
- `docs/design/nova-agent-engine-boundaries.md`：「对外 API」段补 `get_session_tree` / `list_child_sessions` / `delete_session_tree` 三条
- `docs/adr/2026-05-20-session-parent-child-tree.md`：本 Plan 完成后定稿、补充实施后回顾段
- 新增 `tests/integration/session_tree.rs`（或就近的集成测试文件）：端到端跑「父 + 1 子 + 1 孙」拓扑 + 树查询 + 级联删

**禁止修改**：

- `Session` struct（Plan 1 已定）
- `AgentTool::run_subagent` 路径（Plan 2 已定）
- 既有 `delete_session` 的调用者代码（让其拿到错误后自行处理；强制级联是反语义）
- `OrchestratorEngine` / `SubAgentExecutor` trait
- `ConversationWriteHandle`（Plan 2 已定，本 Plan 仅消费）

## Agent 执行步骤

### 步骤 1：定义 `SessionTree`

新建 `crates/nova-agent/src/app/session_tree.rs`：

```rust
use crate::message::Message;
use crate::conversation::session::SessionSummary;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTree {
    pub summary: SessionSummary,
    /// 本子树相对父 Session 的派生锚点：父 history 里那条 ToolUse 的 id。
    /// 根 Session（顶层）此字段为 None。
    pub parent_tool_use_id: Option<String>,
    /// 本 Session 的完整 history（含 ProviderHttpTrace metadata）。
    pub history: Vec<Message>,
    /// 直接子 Session 子树（DFS，max_depth 控制递归深度）。
    pub children: Vec<SessionTree>,
    /// 当 max_depth 触底导致本节点子树未完全展开时为 true；展开完整时为 false。
    pub truncated: bool,
}
```

`app/mod.rs` 增 `pub mod session_tree; pub use session_tree::SessionTree;`。

### 步骤 2：`list_child_session_summaries`（`conversation/service/queries.rs`）

```rust
pub async fn list_child_session_summaries(&self, parent_id: &str) -> Result<Vec<SessionSummary>> {
    let child_ids = self.repository.list_child_session_ids(parent_id).await?;
    let mut out = Vec::with_capacity(child_ids.len());
    for id in child_ids {
        // 复用现有的 SessionSummary 构造路径：
        // 1) load_session_meta 拿 row（含 message_count 由单独 count 查询补，避免 load full history）
        // 2) 构造 SessionSummary
        if let Some(row) = self.repository.load_session_meta(&id).await? {
            let msg_count = self.repository.count_messages(&id).await.unwrap_or(0);
            out.push(SessionSummary {
                id: row.id,
                name: row.title,
                agent_id: row.agent_id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                message_count: msg_count,
            });
        }
    }
    Ok(out)
}
```

> 若 `repository::message_repo` 暂无 `count_messages`，本步骤含义为：新增 `pub async fn count_messages(&self, session_id: &str) -> Result<usize>`（SQL `SELECT COUNT(*) FROM messages WHERE session_id = ?`）。

### 步骤 3：`AgentApplicationImpl::list_child_sessions`

```rust
pub async fn list_child_sessions(&self, parent_id: &str) -> Result<Vec<SessionSummary>> {
    self.conversation_service.list_child_session_summaries(parent_id).await
}
```

### 步骤 4：`AgentApplicationImpl::get_session_tree`

```rust
pub async fn get_session_tree(&self, root_id: &str, max_depth: usize) -> Result<SessionTree> {
    self.build_tree_recursive(root_id, max_depth).await
}

async fn build_tree_recursive(&self, id: &str, remaining_depth: usize) -> Result<SessionTree> {
    // 1) load 自身（含 history）
    let session = self.conversation_service.load_session_full(id).await?
        .ok_or_else(|| anyhow!("session {id} not found"))?;
    let history = session.get_history().await;
    let summary = SessionSummary {
        id: session.id.clone(),
        name: session.get_name().await,
        agent_id: session.get_active_agent().await,
        created_at: session.created_at,
        updated_at: session.updated_at.load(Ordering::SeqCst),
        message_count: history.len(),
    };
    let parent_tool_use_id = session.parent_tool_use_id.clone();

    // 2) 递归子节点（深度上限）
    // 注意：已知串行 await 子树（实现简单、正确性优先）。
    // 性能限制：8 层 × 4 子 ≈ 65k Session 的极端场景慢。
    // zero 侧调用是错误标记触发、非热路径，可接受。
    // 如未来下游高频调用则改 `futures::try_join_all`。
    // （见总览「已收敛的待澄清点」#5）
    let (children, truncated) = if remaining_depth == 0 {
        (Vec::new(), !session.get_child_ids().await.is_empty())
    } else {
        let child_ids = session.get_child_ids().await;
        let mut children = Vec::with_capacity(child_ids.len());
        for child_id in child_ids {
            // Box::pin recursion in async fn
            let subtree = Box::pin(self.build_tree_recursive(&child_id, remaining_depth - 1)).await?;
            children.push(subtree);
        }
        (children, false)
    };

    Ok(SessionTree { summary, parent_tool_use_id, history, children, truncated })
}
```

> 若 `ConversationService` 暂无 `load_session_full(id) -> Option<Arc<Session>>`，本步骤含义为：暴露一个薄方法路由到既有 `cache.get_or_load` 路径。

### 步骤 5：`AgentApplicationImpl::delete_session_tree` + 改 `delete_session`

```rust
pub async fn delete_session_tree(&self, root_id: &str) -> Result<usize> {
    self.conversation_service.delete_session_tree(root_id).await
}

pub async fn delete_session(&self, id: &str) -> Result<()> {
    let children = self.conversation_service.list_child_session_summaries(id).await?;
    if !children.is_empty() {
        anyhow::bail!(
            "session {} has {} child sessions; use delete_session_tree to delete cascade",
            id, children.len()
        );
    }
    self.conversation_service.delete_session(id).await
}
```

`SessionWriteService::delete_session_tree`：

```rust
pub async fn delete_session_tree(&self, root_id: &str) -> Result<usize> {
    // 见总览「已收敛的待澄清点」#4：root 不存在时返回 Ok(0)，不进入后续路径。
    if self.repository.load_session_meta(root_id).await?.is_none() {
        return Ok(0);
    }

    // DFS 收集所有后代 id
    let mut to_delete = vec![root_id.to_string()];
    let mut stack = vec![root_id.to_string()];
    while let Some(cur) = stack.pop() {
        let kids = self.repository.list_child_session_ids(&cur).await?;
        stack.extend(kids.iter().cloned());
        to_delete.extend(kids);
    }
    // 单事务删（messages 已经 ON DELETE CASCADE，sessions 行手动删）
    let count = to_delete.len();
    self.repository.delete_sessions_bulk(&to_delete).await?;
    for id in &to_delete {
        self.cache.remove(id).await;
    }
    Ok(count)
}
```

> `delete_sessions_bulk` 在 `session_repo.rs` 新增：单 transaction 内 `DELETE FROM messages WHERE session_id IN (...)` + `DELETE FROM sessions WHERE id IN (...)`。

### 步骤 6：lib.rs export

```rust
// crates/nova-agent/src/lib.rs
pub use app::SessionTree;  // 经 app/mod.rs 二次 re-export，避免直接路径 app::session_tree::SessionTree
pub use conversation::session::SessionSummary;
```

> `SessionTree` 已在步骤 1 由 `app/mod.rs` 的 `pub use session_tree::SessionTree;` 透出，lib.rs 直接 `pub use app::SessionTree`。与既有 `ToolInventoryView`（listing-api 设计已采用的同款模式）保持一致。

### 步骤 7：docs/design 与 ADR 更新

- `docs/design/system-overview.md` 增「Session 父子树」段：链接到本设计 + 标注 v0.3.5 引入
- `docs/design/nova-agent-engine-boundaries.md`「对外 API」表新增 3 行
- `docs/adr/2026-05-20-session-parent-child-tree.md` 定稿，**必须**包含以下记录条款：
  - 「核心取舍」表四项决策的"上下文 + 后果"展开
  - 总览「已收敛的待澄清点」#6：**single-turn 子 Agent 是设计意图，不是临时约束**——future multi-turn 子 Agent 应在同一子 Session 内累积 history，而非每轮重新派生
  - 总览「项目现状」段的 trace 粒度限制（turn 末次 LLM 调用的单值快照），未来升级路径（`Vec<TraceEntry>`）

### 步骤 8：升 tag

修复流程通过后：

```bash
git add -A
git commit -m "feat: 子 Session 独立化 + 父子树查询 API（v0.3.5）"
git tag v0.3.5
git push origin main --tags
```

zero 仓后续在自身 Plan 1 中 bump `nova-agent` / `nova-agent-loader` tag 为 `v0.3.5`。

## 目标数据结构 / 接口契约

```rust
// lib.rs（顶层 pub use）
pub use SessionTree;
pub use SessionSummary;

// AgentApplicationImpl
impl AgentApplicationImpl {
    pub async fn list_child_sessions(&self, parent_id: &str) -> Result<Vec<SessionSummary>>;
    pub async fn get_session_tree(&self, root_id: &str, max_depth: usize) -> Result<SessionTree>;
    pub async fn delete_session_tree(&self, root_id: &str) -> Result<usize>;
    pub async fn delete_session(&self, id: &str) -> Result<()>;  // 改：有子时返回 Err
}

pub struct SessionTree {
    pub summary: SessionSummary,
    pub parent_tool_use_id: Option<String>,
    pub history: Vec<Message>,
    pub children: Vec<SessionTree>,
    pub truncated: bool,
}
```

## 行为规则

| 输入场景 | 输出 |
|---------|------|
| `list_child_sessions(p_id)`，p_id 有 3 个子 | 按 created_at 升序返回 3 个 `SessionSummary`（轻量，无 history） |
| `list_child_sessions(p_id)`，p_id 无子 | 返回 `Vec::new()` |
| `list_child_sessions(p_id)`，p_id 不存在 | 返回 `Vec::new()`（不报错——repo 查为空） |
| `get_session_tree(root_id, 0)` | 仅返回根节点（history 完整）；若根有子 → `truncated: true`，`children: []` |
| `get_session_tree(root_id, 8)` | 深度优先展开到 8 层；超过 8 层的子树 `truncated: true` 截断 |
| `get_session_tree(non_existent_id, _)` | `Err(anyhow!("session {id} not found"))` |
| `delete_session(p_id)`，p_id 有子 | `Err`，DB 不变 |
| `delete_session(p_id)`，p_id 无子 | 删 1 行 + 其 messages |
| `delete_session_tree(root_id)`，含 1 父 + 2 子 + 3 孙 | 单事务删 6 个 Session + 全部 messages；返回 `Ok(6)` |
| `delete_session_tree(root_id)`，root 不存在 | 返回 `Ok(0)`——见总览「已收敛的待澄清点」#4。实现上先 `load_session_meta(root_id)` 检查；不存在直接返回，不进入 DFS / cache.remove 路径 |

## 禁止事项

- 不让 `delete_session` 隐式级联（保留显式 `delete_session_tree`）
- 不在 `get_session_tree` 内部加任何 history 截断/缩略（让外部消费方按需处理）
- 不引入流式/分页（v0.3.5 首版接受一次性返回；如未来体积成问题再迭代）
- 不并发展开子树（保持 `for child_id in child_ids` 串行 await——见总览「已收敛的待澄清点」#5，已知性能限制；未来需要再改 `try_join_all`）
- 不暴露 `&Session` 引用，全部克隆为独立 `SessionTree`（生命周期 + 重构自由度）
- 不动 `ContentBlock` / `Message` schema

## 测试要求

| 文件 | 测试用例 | 输入 | 断言 |
|------|---------|------|------|
| `app/application.rs`（`#[cfg(test)] mod` 或就近集成） | `list_child_sessions_returns_summaries_in_created_order` | DB 写父 + 3 子（不同 created_at） | 返回 3 项，created_at 升序 |
| 同上 | `list_child_sessions_empty_for_leaf` | DB 写孤立根 | 返回 `Vec::new()` |
| 同上 | `get_session_tree_depth_0_returns_root_only_with_truncated` | DB 写父 + 1 子 | tree.history 完整、tree.children 空、tree.truncated == true |
| 同上 | `get_session_tree_recurses_to_full_depth` | DB 写父 + 1 子 + 1 孙 | tree.children.len()==1、tree.children[0].children.len()==1、所有 truncated==false |
| 同上 | `get_session_tree_truncates_at_depth_limit` | DB 写 10 层链；调 `get_session_tree(root, 3)` | 第 3 层节点 truncated==true、children==[] |
| 同上 | `get_session_tree_attaches_parent_tool_use_id_on_children` | 子 Session parent_tool_use_id="toolu_abc" | tree.children[0].parent_tool_use_id == Some("toolu_abc") |
| 同上 | `get_session_tree_not_found_returns_err` | 调不存在 id | Err |
| 同上 | `delete_session_rejects_when_has_children` | DB 父 + 1 子；调 `delete_session(p)` | Err 含 "child sessions"；DB 行数不变 |
| 同上 | `delete_session_allows_when_no_children` | DB 写孤立根；调 `delete_session(root)` | Ok；DB 行数 -1 |
| 同上 | `delete_session_tree_cascades` | DB 父 + 2 子 + 3 孙；调 `delete_session_tree(p)` | Ok(6)；DB 中 6 个 Session 行 + 全部 messages 删除 |
| 同上 | `delete_session_tree_returns_zero_for_nonexistent_root` | 空 DB 调 `delete_session_tree("ghost")` | `Ok(0)`，不报错；cache 无副作用 |
| `tests/integration/session_tree.rs`（新增） | `end_to_end_subagent_tree_query` | 跑一次带子 Agent 的 turn（mock provider 触发子 Agent + 孙 Agent），然后调 `get_session_tree(parent_id, 8)` | tree.children[0].history 含子 turn 的 Assistant Message（带 ProviderHttpTrace metadata）；tree.children[0].children[0].history 含孙 turn 消息 |

**验证命令**：

```bash
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test --workspace
```

## 完成条件

- [ ] `SessionTree` 类型定义并 `pub use` 自 `nova_agent` crate 顶层
- [ ] `list_child_sessions` / `get_session_tree` / `delete_session_tree` 三个 API 实现
- [ ] `delete_session` 改为「有子时拒绝」
- [ ] 全 workspace `cargo clippy / fmt / test` 全绿
- [ ] `docs/design/system-overview.md`、`docs/design/nova-agent-engine-boundaries.md` 更新
- [ ] `docs/adr/2026-05-20-session-parent-child-tree.md` 定稿
- [ ] git commit + `git tag v0.3.5` + `git push origin main --tags`
- [ ] 本 Plan 的总览状态在 `session-parent-child-tree.md` 改为「已完成」
- [ ] 通知 zero 仓侧可以 bump `nova-agent` / `nova-agent-loader` 到 v0.3.5 并启动其 Plan 1
