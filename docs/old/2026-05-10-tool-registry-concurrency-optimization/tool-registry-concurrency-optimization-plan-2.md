# Plan 2: 读路径快照化（降低锁竞争）

## Plan 编号与标题
- Plan 2: 读路径快照化（降低锁竞争）

## 前置依赖
- Plan 1

## 本次目标
- 将高频只读接口从“遍历锁内实时结构”切换为“读取不可变快照”。
- 降低 `tool_definitions`、`get_turn_view`、`filter_deferred_by_policy`、`deferred_definitions_async` 等接口对主状态锁的依赖。
- 在保证一致性可推理的前提下，为 prompt 构建和 ToolSearch 搜索路径提供更稳定的低成本读取方式。

## 涉及文件
- `crates/nova-agent/src/tool/registry.rs`
- `crates/nova-agent/src/prompt/mod.rs`
- `crates/nova-agent/src/agent/runtime.rs`
- `crates/nova-agent/src/tool/builtin/tool_search.rs`

## 详细设计
### 1. 读快照对象拆分
- 建议将快照明确拆为两层：
  - `RegistrySnapshot`：真实注册状态的只读镜像，包含 loaded definitions、deferred representations、category 索引等。
  - `TurnViewSnapshotBuilder`：基于 `RegistrySnapshot` 和 runtime 开关（`tool_search_enabled`、`skill_tool_enabled`、`task_tools_enabled`）组装当前轮次可见视图。
- 这样可以把“真实状态”和“视图层附加项”分开处理，尤其是 `ToolSearch` 当前是动态附加项，不适合直接混入真实 loaded/deferred 状态。

### 2. 需要快照化的接口
- `tool_definitions()`：供 `runtime.rs` 构建系统提示词使用，应读取 loaded definitions 快照，并按规则附加 `ToolSearch` 入口。
- `loaded_definitions()` / `deferred_definitions_async()`：改为直接从快照返回克隆后的 schema 元数据，而不是重新遍历实例容器。
- `get_turn_view()`：从快照构建 `TurnToolView`，避免每次都拿两把锁并重复构造 `ToolSearch` representation。
- `filter_deferred_by_policy()` / `deferred_tools_by_category()`：优先读取预构建的 deferred 快照或 category 索引快照。

### 3. 刷新策略
- 写路径完成后统一刷新快照，触发点包括：
  - `register`
  - `register_many`
  - `register_deferred_with_category`
  - `resolve_deferred*`
  - `load_deferred_by_category*`
- 刷新规则：
  - 先完成真实状态变更。
  - 再基于变更后的状态生成一份完整新快照。
  - 最后原子替换快照引用。
- 禁止“部分字段先替换、部分字段后替换”的增量拼装，避免读方见到中间态。

### 4. 技术选型建议
- 方案 A：`tokio::RwLock<Arc<RegistrySnapshot>>`
  - 优点：依赖少，便于与现有 `tokio` 并发模型保持一致。
  - 缺点：读仍需要获取读锁，在极端高并发下仍有竞争。
- 方案 B：`ArcSwap<RegistrySnapshot>`
  - 优点：读接近无锁，适合高频 prompt / turn-view 读取。
  - 缺点：新增依赖，写路径实现与调试复杂度略高。
- 推荐决策：
  - 设计文档层面先以 `Arc<RegistrySnapshot>` 的不可变快照模型为抽象。
  - 具体承载原语优先 `tokio::RwLock` 落地，若 Plan 4 压测显示仍有明显瓶颈，再切到 `ArcSwap`，避免过早优化。

### 5. 一致性语义
- 需要明确允许的最坏情况：
  - 写刚完成、快照尚未替换前，读可能短暂看到旧快照。
- 但不允许出现以下反向不一致：
  - 快照声称工具已可用，但 `execute()` 仍无法找到对应 loaded tool。
- 因此建议顺序固定为：
  - 真实状态迁移成功后，再发布新快照。
- 这一顺序会带来“稍晚可见”，但不会带来“错误可见”。该权衡对模型侧行为更安全。

### 6. 对下游模块的影响
- `runtime.rs` 中 `filter_tool_definitions()` 当前调用同步 `tool_definitions()`；改造后无需改变上层语义，但应确认快照内容已包含其依赖的全部 schema 信息。
- `tool_search.rs` 的搜索逻辑可直接消费 deferred snapshot，避免每次搜索都等待 deferred 锁。
- `prompt/mod.rs` 无需感知实现细节，只依赖传入的 `ToolDefinition` 集合；本 Plan 不改变其接口。

## 测试案例
- 正常路径：
  - 连续多轮构建 prompt / `get_turn_view` / ToolSearch 搜索，在无写入时不产生明显锁等待增长。
- 边界条件：
  - 写入与读取交错时，读到的快照始终结构合法，不出现半更新状态。
  - `ToolSearch` 动态附加项在快照模式下仍只在需要的视图中出现，不污染真实注册状态。
- 异常场景：
  - 若快照重建过程引入可失败分支，则必须保持旧快照继续可读，不能暴露破损快照。
