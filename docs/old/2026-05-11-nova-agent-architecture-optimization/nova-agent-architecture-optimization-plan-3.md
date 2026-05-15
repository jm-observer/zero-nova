# Plan 3：热点状态与工具执行并发优化

## 前置依赖
- Plan 1
- Plan 2

## 本次目标
- 在已明确启动/运行时边界的基础上，收敛 `TaskStore`、工具执行上下文、读缓存等热点状态的锁粒度与并发语义。
- 保持工具行为和外部接口不变，但减少无谓串行化与后续扩展障碍。
- 明确 turn-scoped、session-scoped、runtime-scoped 状态边界，避免 `AgentRuntime` 继续承载所有工具状态细节。

## 涉及文件
- `crates/nova-agent/src/tool/builtin/task.rs`
- `crates/nova-agent/src/tool/builtin/mod.rs`
- `crates/nova-agent/src/agent/runtime.rs`
- `crates/nova-agent/src/tool/read_cache.rs`
- `crates/nova-agent/src/tool/registry.rs`
- `crates/nova-agent/src/app/application.rs`

## 现状依据
- `crates/nova-agent/src/agent/runtime.rs:42` 定义 `AgentRuntime`，同时持有 provider client、tool registry、config、task store、skill registry、read files、side channel 等状态。
- `crates/nova-agent/src/agent/runtime.rs:46`、`crates/nova-agent/src/tool/registry.rs:28` 和 `crates/nova-agent/src/tool/builtin/task.rs:248` 均使用 `Arc<Mutex<TaskStore>>`。
- `crates/nova-agent/src/tool/builtin/task.rs:72` 的 `TaskStore` 内部维护 `HashMap<String, Task>` 与 `AtomicU64`。
- `crates/nova-agent/src/agent/runtime.rs:508` 每轮创建 `Arc<RwLock<TurnReadState>>`，`crates/nova-agent/src/tool/read_cache.rs:17` 的 `TurnReadState` 保存单轮读取状态。
- `crates/nova-agent/src/tool/registry.rs:34` 将 `turn_read_state` 作为 `ToolContext` 的可选 turn-level 状态传入工具。

## 详细设计
### 1. `TaskStore` 并发模型梳理
- 当前 `TaskCreate`、`TaskList`、`TaskUpdate` 共享同一个 `Arc<Mutex<TaskStore>>`。
- 该设计正确但保守，意味着只要有一个任务相关工具正在持锁，其余读写都需等待。
- 优化方向应分两步：
  - 第一步：缩短持锁区。解析输入、格式化输出、构造 markdown/JSON 等操作不应在锁内完成。
  - 第二步：根据访问模式决定是否采用 `RwLock` 或 service 化封装。
- 初步推荐：优先引入 `TaskStoreHandle` 或 `TaskService`，隐藏锁类型，避免 `Arc<Mutex<TaskStore>>` 泄漏到 `AgentRuntime`、`ToolContext` 和各工具实现中：

```rust
#[derive(Clone)]
pub(crate) struct TaskStoreHandle {
    inner: Arc<RwLock<TaskStore>>,
}
```

- 若实际写入频率较高、读写比例不明显，则可继续使用 `Mutex`，但仍应通过 handle 隔离锁实现，方便后续调整。
- 不建议直接上复杂分片或 actor 化，除非现有使用模式已经证明 `RwLock` 不足。

### 2. `TaskStore` API 边界
- 将 store 操作收敛为语义方法，而不是在工具中直接持锁访问内部结构：
  - `create_task(input) -> Result<Task>`
  - `list_tasks(filter) -> Vec<TaskSummary>`
  - `update_task(id, patch) -> Result<Task>`
  - `get_task(id) -> Option<Task>`
- 工具层只负责参数解析、权限/上下文校验、输出格式化。
- store/service 层负责 id 分配、状态转换、metadata 合并和一致性检查。
- 这样可以把并发控制集中在一处，也便于测试状态转换。

### 3. 工具执行上下文的状态边界
- `AgentRuntime` 中除了 tool registry，还维护 `task_store`、`read_files`、skill registry、side channel 等上下文状态。
- 随着工具数量增加，runtime 很容易变成“所有工具状态的大容器”。
- 建议整理出按职责分组的上下文：
  - turn-scoped state：如 `TurnReadState`、本轮 tool result accumulator、loop guard。
  - session-scoped state：如任务存储、项目目录、技能绑定、会话级权限。
  - runtime-scoped state：如工具注册表、全局配置、共享 HTTP client、provider client。
- 本 Plan 不强制一次性新增大量类型，但至少应避免在 `ToolContext` 中继续暴露原始锁实现。

### 4. 读缓存与重复读取判定
- 当前 `ReadTool` 已包含 canonical path + range 检测逻辑，这是合理的 turn 级优化。
- 需要进一步确认并固化以下语义：
  - `TurnReadState` 只在单轮 `run_turn` 内创建和使用。
  - 不同 session、不同 turn 不共享 read cache。
  - 工具并发执行时，读缓存只保护“重复读取判定与状态写入”，不包裹实际文件读取。
- 若当前实现已满足这些语义，主要补充测试和文档；不要把 turn cache 升级为全局缓存。

### 5. `read_files` 与 `TurnReadState` 的关系
- `AgentRuntime` 目前同时持有 `read_files: Arc<Mutex<HashSet<String>>>` 和每轮 `TurnReadState`。
- 需要明确二者职责：
  - `read_files` 若用于 session/global 级“曾经读取过的文件”记录，应命名和文档体现其生命周期。
  - `TurnReadState` 仅用于本轮重复读取收敛。
- 若二者语义重复，应合并或删除其中之一；若语义不同，应避免工具调用方混用。

### 6. 工具注册与执行热路径协同
- `tool/registry.rs` 已承担工具定义注册与执行入口，runtime 侧如何消费注册表快照、如何构造本轮可见工具定义，也需要和状态设计保持一致。
- 特别是任务工具、技能工具、project manager 等依赖 session 状态的工具，需明确它们读取的是“瞬时快照”还是“执行时实时状态”。
- 推荐原则：
  - 工具定义展示优先使用稳定快照。
  - 工具执行时读取必要的最新 session state。
  - 不跨多个锁长时间持有；如必须读取多个状态，先复制最小必要数据再释放锁。

### 7. 锁顺序与失败恢复
- 为避免引入死锁，需要定义固定锁顺序。例如：
  1. turn-scoped state
  2. session-scoped state
  3. runtime/global state
- 实际实现中应尽量避免同时持有多个锁；当无法避免时，在代码注释中说明顺序原因。
- 工具执行失败后，不应遗留部分更新的共享状态。任务更新这类操作应先校验输入，再进入短事务式更新。

### 8. 迁移步骤
1. 统计任务工具当前锁内逻辑，先把输出格式化等无关逻辑移出锁区。
2. 引入 `TaskStoreHandle` / `TaskService`，将锁类型从工具实现和 `ToolContext` 中隐藏。
3. 根据读写路径选择内部使用 `Mutex` 或 `RwLock`，优先保持行为稳定。
4. 明确 `read_files` 与 `TurnReadState` 的生命周期，删除重复或补齐注释与测试。
5. 调整 `AgentRuntime` 构造 `ToolContext` 的流程，减少 runtime 对具体工具状态结构的了解。
6. 补充并发测试和失败恢复测试。

## 测试案例
### 正常路径
- 多个任务工具在典型交替读写场景下保持语义正确。
- `TaskCreate` 创建的任务可被 `TaskList` 和 `TaskUpdate` 正确观察。
- 单轮内重复读取相同文件/范围仍能命中 read cache 规则。
- 不同 turn 中读取同一文件不会错误命中上一轮 `TurnReadState`。

### 边界条件
- 并发执行多个只读任务列表查询时，不因全局串行锁导致不必要等待。
- 会话级状态与轮次级状态不会互相污染。
- 空任务列表、未知 task id、重复 update、metadata 缺失等场景结果稳定。
- 多工具并发读取不同文件时，文件 I/O 不被 read cache 写锁长时间包裹。

### 异常场景
- 任一工具执行失败时，不会遗留损坏的共享状态。
- 锁顺序调整后，不引入死锁、饥饿或隐藏 panic。
- 任务更新参数非法时，不应修改原任务。
- read cache 状态更新失败或路径 canonicalize 失败时，错误上下文应可定位文件路径。

## 验收标准
- `ToolContext` 和工具实现不再依赖裸露的 `Arc<Mutex<TaskStore>>` 作为长期接口。
- `TaskStore` 的读写操作通过语义方法完成，锁内只保留最小状态访问。
- `TurnReadState` 生命周期明确为单轮，不与 session/global 状态混淆。
- `AgentRuntime` 中工具相关状态按生命周期边界更清晰，新增工具不需要理解所有既有状态细节。
- 通过 `cargo clippy --workspace -- -D warnings`、`cargo fmt --all`、`cargo test --workspace`。
