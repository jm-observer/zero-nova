# Plan 4: 合并 storage 到 repository

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 删除 `conversation/storage/` 子模块，将其唯一的事务函数移入 repository 层。

**Architecture:** `storage/` 目录只有一个 `delete_session_with_messages` 函数。将其移入 `SqliteSessionRepository` 作为方法。

**Tech Stack:** Rust, sqlx

---

## 前置依赖

无（与其他 Plan 独立）

## 涉及文件

- `crates/nova-agent/src/conversation/storage/sqlite_tx.rs` — 删除
- `crates/nova-agent/src/conversation/storage/mod.rs` — 删除
- `crates/nova-agent/src/conversation/mod.rs` — 移除 `pub mod storage`
- `crates/nova-agent/src/conversation/repository/session_repo.rs` — 添加 `delete_session_with_messages` 方法

## 详细设计

### 移动函数

将 `storage/sqlite_tx.rs` 中的 `delete_session_with_messages` 移入 `SqliteSessionRepository`：

```rust
impl SqliteSessionRepository {
    pub(crate) async fn delete_session_with_messages(&self, session_id: &str) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        let mut tx = conn.begin().await?;
        sqlx::query("DELETE FROM messages WHERE session_id = ?")
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
```

### 更新调用方

找到所有调用 `storage::sqlite_tx::delete_session_with_messages(pool, session_id)` 的地方，改为 `self.repository.delete_session_with_messages(session_id)`（或 `repository.delete_session_with_messages(session_id)`）。

## 测试案例

- `cargo test -p nova-agent` 全部通过
- 删除会话功能正常工作
