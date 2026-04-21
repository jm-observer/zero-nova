# Session 持久化存储详细设计

| 章节 | 说明 |
|-----------|------|
| 时间 | 2026-04-20 |
| 项目现状 | 当前 Session 数据（包括消息历史）存储在内存 `HashMap` 中，程序重启后数据全部丢失。 |
| 本次目标 | 实现基于 SQLite 的持久化存储，确保会话、消息历史在应用重启后可恢复。 |

## 1. 技术选型

| 组件 | 选型 | 理由 |
|------------|----------|------|
| **数据库** | SQLite | 轻量级文件数据库，无需独立 Server，适合桌面应用与嵌入式后端。 |
| **驱动层** | `sqlx` | 1. 纯 Rust 实现（配合 `sqlite` feature）；2. 支持异步异步 I/O（`tokio`）；3. 编译期 SQL 校验。 |
| **序列化** | `serde_json` | `Message` 包含复杂的 `ContentBlock` 数组，使用 JSON 存储该字段最为灵活。 |

## 2. 数据库架构 (Schema)

### 2.1 `sessions` 表
存储会话的元数据。

| 字段 | 类型 | 说明 |
|------------|----------|------|
| `id` | TEXT (PK) | 会话唯一标识 (UUID)。 |
| `title` | TEXT | 会话标题。 |
| `agent_id` | TEXT | 关联的 Agent ID。 |
| `created_at` | INTEGER | 创建时间戳 (ms)。 |
| `updated_at` | INTEGER | 最后活跃时间戳 (ms)，用于排序。 |

### 2.2 `messages` 表
存储对话详细内容。

| 字段 | 类型 | 说明 |
|------------|----------|------|
| `id` | INTEGER (PK AI) | 消息 ID。 |
| `session_id` | TEXT (FK) | 关联的会话 ID。 |
| `role` | TEXT | `system` / `user` / `assistant`。 |
| `content` | TEXT (JSON) | 序列化后的 `Vec<ContentBlock>`。 |
| `created_at` | INTEGER | 创建时间戳。 |

## 3. 详细设计

### 3.1 Session 生命周期
1. **初始化**：
   - 启动时校验 `.nova/data` 目录是否存在。
   - 建立 `SqlitePool` 连接池。
   - 自动运行 `CREATE TABLE IF NOT EXISTS`（通过 `sqlx` 迁移）。
2. **加载列表**：
   - `SessionStore::list_sorted()` 改为执行 `SELECT * FROM sessions ORDER BY updated_at DESC`。
3. **加载历史**：
   - `Session::get_messages()` 改为执行 `SELECT * FROM messages WHERE session_id = ? ORDER BY id ASC`。
4. **持久化触发集**：
   - **创建会话**：向 `sessions` 表插入新行。
   - **发送/接收消息**：向 `messages` 表追加记录，并 `UPDATE sessions SET updated_at = ?`。
   - **删除会话**：级联删除 `sessions` 和关联的 `messages`。

### 3.2 性能优化建议
- **L1 缓存**：保留内存中的 `SessionStore` 作为 LRU 缓存，避免频繁的磁盘 IO。
- **批量插入**：Assistant 返回的消息（可能包含多个 Block）在一次事务中写入。
- **WAL 模式**：开启 SQLite 的 Write-Ahead Logging (WAL) 模式，提升并发读写性能。

## 4. 测试案例

- **数据完整性测试**：
  - 创建 Session -> 写入消息 -> 重启应用 -> 验证历史记录是否完全一致。
- **Agent 关联测试**：
  - 验证不同 Agent 的会话是否正确隔离且能按 Agent 过滤加载。
- **异常路径**：
  - 写入时磁盘空间不足的处理。
  - 数据库文件损坏时的自动重联/恢复逻辑（备选方案）。

## 5. 风险与待定项

- **数据库迁移**：随着功能迭代，`Message` 结构可能变化，需考虑 `sqlx-migration` 的管理。
- **文件锁定**：多实例运行（虽然目前不大可能）可能会导致 SQLite 文件锁定冲突。
