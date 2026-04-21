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
| **驱动层** | `sqlx` | 1. 纯 Rust 实现（配合 `sqlite` feature）；2. 支持异步 I/O（`tokio`）；3. 编译期 SQL 校验。 |
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

### 3.1 存储策略：Write-Through Cache
为了兼顾性能和可靠性，采用 **Write-Through** 模式：
- **读取 (Read)**：首先从内存 `SessionStore` 的 `HashMap` 中查找。若不存在（如应用启动后），从 SQLite 加载数据并填充回内存。
- **写入 (Write)**：任何对 Session 的修改（如 `append_user_message`, `append_assistant_messages`）必须同步触发数据库操作：更新内存数据 $\rightarrow$ 异步/同步写入 SQLite $\rightarrow$ 更新 `sessions` 表的 `updated_at`。

### 3.2 组件划分
1. **`SqliteManager`**：负责连接池管理、`.nova/data` 目录创建及 `sqlx` 迁移逻辑。
2. **`SqliteSessionRepository`**：实现对数据库的增删改查，将业务模型与数据库行进行映射。
3. **`SessionStore` (Refactored)**：整合 `SqliteSessionRepository`，作为统一的 Session 管理入口，维护内存缓存。

### 3.3 关键技术实现
- **JSON 序列化**：利用 `serde_json` 将 `Vec<ContentBlock>` 转换为 `TEXT` 存储。
- **并发处理**：利用 `sqlx` 的异步特性配合 `tokio`，确保数据库 I/O 不会阻塞异步运行时。

## 4. 测试案例

- **数据完整性测试**：创建 Session $\rightarrow$ 写入消息 $\rightarrow$ 重启应用 $\rightarrow$ 验证历史记录是否完全一致。
- **Agent 关联测试**：验证不同 Agent 的会话是否正确隔离。
- **排序测试**：验证 `updated_at` 是否能正确驱动会话列表的降序排列。

## 5. 风险与待定项

- **数据库迁移**：随着 `Message` 结构变化，需通过 `sqlx-migration` 管理版本。
- **文件锁定**：考虑多实例运行时 SQLite 的文件锁定问题。
