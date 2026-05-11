# Plan 1：会话加载与持久化路径优化

## 前置依赖
- 无

## 本次目标
- 将会话服务从“启动全量预加载 + 整体持久化”逐步调整为“启动仅加载必要索引 + 按需冷加载消息 + 增量持久化变更”。
- 在保持现有会话语义不变的前提下，降低启动时间、内存占用和长会话场景下的写放大。
- 明确缓存中“会话元数据已知”和“消息历史已加载”的状态边界，避免用“是否在 cache 中”隐式表达完整加载状态。

## 涉及文件
- `crates/nova-agent/src/app/bootstrap.rs`
- `crates/nova-agent/src/app/application.rs`
- `crates/nova-agent/src/conversation/service/mod.rs`
- `crates/nova-agent/src/conversation/cache.rs`
- `crates/nova-agent/src/conversation/repository/mod.rs`
- `crates/nova-agent/src/conversation/repository/session_repo.rs`
- `crates/nova-agent/src/conversation/repository/message_repo.rs`
- `crates/nova-agent/tests/integration/session_*.rs`

## 现状依据
- `crates/nova-agent/src/app/bootstrap.rs:52` 当前启动时调用 `session_service.load_all().await?`。
- `crates/nova-agent/src/conversation/service/mod.rs:76` 的 `load_all()` 会先 `repository.list_sessions().await?`，再逐个完整加载会话。
- `crates/nova-agent/src/conversation/service/mod.rs:1582` 的测试也模拟启动阶段执行 `load_all()`，后续改造需要同步更新测试语义。

## 详细设计
### 1. 启动加载目标调整
- 当前 `bootstrap` 在应用启动时调用 `session_service.load_all()`，其内部会完整加载所有 session 与消息。
- 调整后，启动阶段只需要准备以下最小运行集：
  - session 基础元数据索引：`id`、`title`、`agent_id`、`created_at`、`updated_at`、`project_dir`、必要控制状态。
  - 对“最近会话”“按 agent 找最新会话”这类查询必要的排序信息。
  - 不加载 message body、tool result、大型附件上下文等运行时才需要的数据。
- 具体实现上，将现有 `load_all()` 拆成两个明确职责的入口：
  - `load_session_index()`：启动期加载轻量索引，不加载完整 history。
  - `ensure_session_history_loaded(session_id)`：在首次访问某个 session history 时再加载消息体。
- `load_all()` 不建议继续作为生产启动入口。若短期需要保留，应标记为测试/迁移辅助，并避免被 `bootstrap` 调用。

### 2. 缓存模型改造
- 当前缓存以完整 `Session` 为中心，默认认为 history 已可用。
- 改造后需显式区分三类状态：
  - `Indexed`：只有元数据和排序/查找字段可用。
  - `LoadingHistory`：某个 task 正在加载消息历史，用于并发去重。
  - `HistoryLoaded`：元数据与消息历史均可用。
- 推荐缓存内部保存一个轻量 wrapper，而不是让调用方猜测 `Session.messages` 是否完整：

```rust
struct CachedSessionEntry {
    session: Arc<Session>,
    history_state: HistoryState,
}

enum HistoryState {
    Indexed,
    Loading,
    Loaded,
}
```

- 具体类型名可按现有代码调整；关键要求是状态语义显式化。
- 为避免并发重复冷加载，延续现有 loading 去重思路，但去重对象从“整个 session 不存在”收敛为“history 尚未加载”。
- 冷加载完成后应以一次原子缓存更新替换旧 entry，避免其它调用方观察到“部分消息已写入但状态仍未 loaded”的中间态。

### 3. 仓储层职责下沉
- repository 层需要把“查索引”和“查完整历史”作为两个明确语义分开暴露：
  - `list_session_summaries()`：返回轻量 session 行，不 join / 不反序列化消息。
  - `load_session_metadata(session_id)`：加载单个会话元数据。
  - `load_session_messages(session_id)`：只加载消息列表。
  - `load_session_with_messages(session_id)`：保留作为组合接口，供测试或特殊迁移场景使用。
- 这样可以避免 service 层在每次轻量查询时被迫承担完整消息反序列化成本。
- 若当前 repository 已有相近接口，应优先改名或补语义注释，不额外制造重复 API。

### 4. 读取路径的懒加载边界
- 以下调用不应触发消息冷加载：
  - `list_sessions()`
  - `find_latest_session_by_agent()`
  - `session_exists()`
  - 仅展示标题、创建时间、更新时间的 UI/API 查询
- 以下调用必须在读取前确保 history 已就绪：
  - `session_messages()`
  - `start_turn()` / 构建 prompt 的路径
  - copy/fork 需要复制完整历史的路径
  - 任何需要追加消息并依赖当前最后一条消息语义的路径
- `ensure_session_history_loaded()` 应成为这些路径的共同入口，避免各调用点自行判断加载状态。

### 5. 持久化路径增量化
- 当前 `persist_full_session()` 以整会话视角落库，适合作为初始化、迁移或修复逻辑，但不应成为常规热路径。
- 优化方向：
  - 创建 session 时：写 session 基础记录 + 初始 system message（若存在）。
  - 追加消息时：仅 append 新消息，并更新 session 的 `updated_at` / title / control 等必要字段。
  - 修改 title / runtime control / project_dir 时：走局部 update，而非重写整会话。
  - 删除或归档会话时：按现有事务边界处理 session 与 messages 的一致性。
- `persist_full_session()` 可保留为 `persist_full_session_for_rebuild()` 或类似命名，避免误用为默认写路径。

### 6. 一致性与错误语义
- 增量持久化失败时不得静默吞错；调用方必须拿到 `anyhow::Result` 错误上下文。
- 写入顺序建议遵循“数据库成功后再更新内存可见态”或“内存更新失败可回滚/重建”的单一策略，避免内存和数据库出现不可解释的分歧。
- 对创建/追加消息这类需要多表写入的路径，仓储层应使用事务保证一致性。
- 单个 session 的消息反序列化失败不应影响索引列表加载，但首次访问该 session history 时必须返回带上下文的错误。

### 7. 迁移步骤
1. 为 repository 增加或明确轻量索引查询与消息查询接口。
2. 为 cache 增加显式 history 状态，并保持现有外部查询语义。
3. 将 `SessionService::load_all()` 拆为 `load_session_index()` 与 history 冷加载入口。
4. 修改 `bootstrap` 使用 `load_session_index()`。
5. 将 `session_messages()`、`start_turn()` 等路径接入 `ensure_session_history_loaded()`。
6. 将追加消息、更新标题/控制状态等热写路径调整为增量持久化。
7. 更新测试，保留必要的全量加载测试作为迁移/兼容用例。

## 测试案例
### 正常路径
- 启动后仅加载会话索引，首次读取某个 session 消息时再触发 history 冷加载。
- 新建会话、追加消息、切换 agent 后，轻量列表与完整消息读取结果保持一致。
- `start_turn()` 在 history 未加载时能自动冷加载，并使用完整历史构建 prompt。
- 更新 title / runtime control / project_dir 后，列表查询与完整会话查询都能看到一致结果。

### 边界条件
- 大量历史会话存在时，启动不需要完整加载所有消息即可完成初始化。
- 同一 session 被并发首次访问时，只触发一次 history 冷加载。
- 空会话、只有 system message 的会话、超长历史会话都能被正确索引与冷加载。
- 列表查询不因某个 session 的消息损坏而失败。

### 异常场景
- 单个 session 的 message 反序列化失败时，应在访问该 session history 时返回带 session id 的错误上下文。
- 增量持久化某一步失败时，不得静默吞错；需保证内存态与持久化失败可被调用方感知。
- 冷加载过程中 repository 返回错误时，缓存状态应回到可重试状态，而不是永久停留在 `Loading`。
- 并发冷加载中等待方取消时，不应影响发起加载方完成缓存更新。

## 验收标准
- `bootstrap` 不再调用生产语义上的全量 `load_all()`。
- `list_sessions()`、`find_latest_session_by_agent()` 等轻量查询不触发消息历史加载。
- 首次访问消息历史时触发懒加载，并有并发去重。
- 常规追加消息路径不再重写整会话。
- 通过 `cargo clippy --workspace -- -D warnings`、`cargo fmt --all`、`cargo test --workspace`。
