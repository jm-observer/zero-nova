# Plan 2: 原子快照缓存（确定实施）

## 前置依赖
- Plan 1

## 本次目标
- 将 `config_snapshot` 读取路径固定为原子快照缓存。
- 消除高频读取场景下的 `AppConfig` 读锁竞争与 `serde_json::to_value(...)` 重复序列化开销。
- 保证配置磁盘态、内存态、快照态在更新成功后保持一致。

## 涉及文件
- `crates/nova-agent/src/app/application.rs`
- 视实现复杂度决定是否拆分 `crates/nova-agent/src/app/config_snapshot_cache.rs`
- `Cargo.toml`

## 依赖与选型
- 固定选型：`arc-swap`
- 引入方式：在根 `Cargo.toml` 的 `[workspace.dependencies]` 增加 `arc-swap`
- 选择理由：
  - 读路径为无锁原子加载，契合高频快照读取场景。
  - API 简单，适合维护 `Arc<Value>` 这种只读快照。
  - 相比自行组合原子指针或额外锁，复杂度更低，语义更明确。

## 详细设计
### 1. 状态结构
- 在 `AgentApplicationImpl` 中新增原子快照字段，持有 `Arc<Value>`。
- `self.config` 继续作为真实配置源，用于更新、共享和其他依赖 `AppConfig` 结构化访问的路径。
- 快照缓存仅承担“对外读取 JSON 配置快照”的职责，避免职责混杂。

### 2. 初始化流程
- 在 `AgentApplicationImpl::new` 中，对初始 `AppConfig` 先执行一次 `serde_json::to_value(...)`。
- 将结果包装为 `Arc<Value>` 后写入原子缓存。
- 若初始化序列化失败，构造函数直接返回错误，不允许实例带着空缓存启动。

### 3. 读取路径
- `config_snapshot().await` 不再读取 `self.config`，而是直接从原子缓存 load 当前 `Arc<Value>`。
- 对外仍返回 `Result<Value>`，因此读取后执行一次 `(*snapshot).clone()`。
- 这里保留 `Value` 克隆是合理成本：
  - 接口返回所有权，无法只借用缓存内容。
  - 已经移除了更重的锁和序列化开销，剩余成本可接受。

### 4. 更新路径
- `update_config` 顺序固定为：
  1. 解析 `payload` 得到 `new_config`
  2. 预先生成 `config_str`
  3. 预先生成 `snapshot_value`
  4. 写磁盘
  5. 更新 `self.config.write().await`
  6. 用 `Arc<Value>` 原子替换快照缓存
- 该顺序的目的：
  - 序列化失败发生在写入前，避免部分更新。
  - 磁盘写失败时，内存态和快照态都保持旧值。
  - 一旦内存态更新成功，快照态立即原子切换到新值。

### 5. 一致性边界
- 允许存在极短瞬间：某个并发读请求在原子替换前读到旧快照；这是并发读写下可接受的一致性边界。
- 不允许出现长时间旧值暴露，也不允许更新成功后快照长期落后于 `self.config`。
- 由于 `snapshot_value` 在写磁盘前已构建完成，原子替换阶段不应再有可失败步骤。

### 6. 错误处理
- `serde_json::from_value::<AppConfig>(payload)` 失败时，返回带 context 的解析错误。
- `toml::to_string(&new_config)` 或 `serde_json::to_value(&new_config)` 失败时，返回序列化错误，且不写磁盘。
- `tokio::fs::write(...)` 失败时，返回带路径上下文的错误，且不更新内存配置与快照缓存。

## 测试案例
- 正常路径：初始化后快照可读，内容与 `AppConfig` 一致。
- 更新路径：`update_config` 成功返回后立即读取快照，字段为新值。
- 并发路径：高并发读取 + 低频更新，不出现死锁、panic、长时间旧值暴露。
- 失败路径：磁盘写失败时，`self.config` 与快照缓存都保持旧值。
- 回归路径：读取快照实现不再包含 `self.config.read().await` 和 `serde_json::to_value(...)`。
