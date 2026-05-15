# Plan 1: 并发模型重构（统一状态与 API 分层）

## Plan 编号与标题
- Plan 1: 并发模型重构（统一状态与 API 分层）

## 前置依赖
- 无

## 本次目标
- 将 ToolRegistry 的内部组织从“两把并列容器锁”收敛为“单一状态入口 + 明确读写责任边界”的模型。
- 为同步 API、异步 API、视图 API 建立清晰分层，避免运行期误用 startup-only 同步路径。
- 为后续读快照化与写事务化奠定稳定的数据模型和命名语义。

## 涉及文件
- `crates/nova-agent/src/tool/registry.rs`
- `crates/nova-agent/src/tool/builtin/tool_search.rs`
- `crates/nova-agent/src/tool/builtin/mod.rs`
- `crates/nova-agent/src/agent/runtime.rs`

## 详细设计
### 1. 状态聚合
- 引入内部状态对象，例如：
  - `RegistryState { loaded, deferred, indexes }`
  - `loaded` 保存已加载工具实例。
  - `deferred` 保存 deferred 条目与其元数据。
  - `indexes` 预留给按名称、按类别的快速索引，避免后续 Plan 再次拆结构。
- `ToolRegistry` 外层不再直接暴露两组独立容器字段，而是通过单一状态入口统一访问，减少“调用方先锁 A、再锁 B”的隐式事务拼装。

### 2. API 分层
- 保留同步 API，但仅限启动阶段和测试辅助：
  - `register`
  - `register_many`
  - `register_deferred_with_category`
- 运行期会参与锁竞争或状态迁移的接口统一提供异步版本，并作为正式入口：
  - `resolve_deferred_async`
  - `load_deferred_by_category_async`
  - 未来若 `get_turn_view` 继续承担高频视图职责，应改为读取快照，不再混用同步锁路径。
- 对仍保留的同步接口增加显式语义标识，建议方式二选一：
  - 方案 A：命名层区分，如 `register_startup_only`。
  - 方案 B：保留名称，但将私有锁函数与文档都明确标记为 startup-only，并在 debug 断言中检测运行态误用。
- 推荐优先方案 B，改动面更小，更适合在现有调用链上渐进演进。

### 3. 锁序与实现约束
- 若 Plan 1 仍暂时保留多把锁，则需要在文档和实现里统一锁顺序，建议固定为：
  - `deferred` → `loaded`
- 但从演进角度，推荐尽早收敛为“单状态锁 + 锁内原子状态迁移”，原因：
  - 当前 `resolve_deferred*` 先删 deferred，再加 loaded，本质是跨容器事务。
  - 后续若引入快照刷新和指标记录，跨锁步骤会进一步增加不一致窗口。
- 因此 Plan 1 的落地目标不是立即做性能优化，而是先把“状态如何被一致地修改”固定下来。

### 4. 命名与返回语义预埋
- 当前 `resolve_deferred*` 返回 `bool`，无法区分：
  - 成功从 deferred 迁移到 loaded。
  - 工具本来就已 loaded。
  - 工具根本不存在。
- Plan 1 应先定义后续统一返回枚举的命名，例如：
  - `DeferredResolveOutcome::Loaded`
  - `DeferredResolveOutcome::AlreadyLoaded`
  - `DeferredResolveOutcome::NotFound`
  - `DeferredResolveOutcome::FactoryFailed`
- 即使枚举具体实现放到 Plan 3，Plan 1 也应先把调用契约定下来，避免 `ToolSearch` 和 runtime 层继续围绕布尔值追加分支。

### 5. 向后兼容要求
- 保持以下行为不变：
  - deferred 工具仍可按名称按需加载。
  - `ToolSearch` 仍是模型发现 deferred 工具的入口。
  - `execute()` 中对工具别名的 canonical mapping 不变。
  - schema 校验与路径预处理行为不变。

### 6. 对调用方的影响
- `runtime.rs` 当前主要在启动时调用 `register`，在提示词构建前读取 `tool_definitions()`；这部分应继续可用，但需要明确其读取的是“运行期可见工具视图”还是“当前已加载工具 + ToolSearch 入口”。
- `tool_search.rs` 当前依赖 `resolve_deferred_async + has_loaded_tool_async` 的双阶段判断；Plan 1 应将其列为必须改造的下游调用点。

## 测试案例
- 正常路径：
  - 启动阶段注册 loaded/deferred 工具后，运行期 `execute()` 与 `ToolSearch(select:...)` 均可正常工作。
- 边界条件：
  - 同一 deferred 工具被并发 resolve 时，不因锁序错误产生死锁或 panic。
  - startup-only 接口在无竞争的测试/初始化阶段保持兼容。
- 异常场景：
  - 若运行期误用同步路径，应能在 debug 或测试中快速暴露，而不是静默依赖 try_lock 成功。
