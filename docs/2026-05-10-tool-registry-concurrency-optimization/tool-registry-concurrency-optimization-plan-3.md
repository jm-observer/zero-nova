# Plan 3: 写路径事务化与一致性保障

## Plan 编号与标题
- Plan 3: 写路径事务化与一致性保障

## 前置依赖
- Plan 1

## 本次目标
- 将 deferred → loaded 的迁移定义为显式事务语义，而不是多个离散步骤的组合效果。
- 用结构化返回结果替代布尔值，消除调用方二次探测状态的需求。
- 明确 factory 失败、重复加载、批量加载的行为与日志边界。

## 涉及文件
- `crates/nova-agent/src/tool/registry.rs`
- `crates/nova-agent/src/tool/builtin/tool_search.rs`
- `crates/nova-agent/src/agent/runtime.rs`

## 详细设计
### 1. 单工具迁移语义
- 将 `resolve_deferred(name)` / `resolve_deferred_async(name)` 的语义升级为返回显式枚举，例如：
  - `Loaded`
  - `AlreadyLoaded`
  - `NotFound`
  - `FactoryFailed { message }`
- 语义定义：
  - `Loaded`：命中 deferred，成功构建实例并完成迁移。
  - `AlreadyLoaded`：目标已在 loaded 中存在，本次未重复创建。
  - `NotFound`：既不在 deferred，也不在 loaded。
  - `FactoryFailed`：factory 执行失败或校验失败，状态保持为“仍可重试”的一致状态。

### 2. 事务步骤
- 推荐单次迁移采用以下固定步骤：
  - 在状态锁内检查 loaded/deferred 中的当前归属。
  - 若命中 deferred，先将其标记为“正在迁移”或临时取出到局部变量。
  - 在锁外执行 factory，避免实例构造长时间占用状态锁。
  - factory 成功后重新入锁提交到 loaded，并刷新快照。
  - factory 失败则回滚 deferred 条目，保证后续请求可重试。
- 这里的关键不是“绝对单锁包住全部步骤”，而是“步骤之间的状态转移可证明且可回滚”。

### 3. 并发竞争处理
- 并发场景下只允许一个请求真正执行同名工具的 factory。
- 可选实现方式：
  - 方案 A：在 deferred 条目中增加 `Loading` 中间状态。
  - 方案 B：引入按工具名的细粒度 in-flight map，记录当前加载任务。
- 推荐优先方案 A，原因：
  - 与当前 registry 内部状态最贴近。
  - 不需要额外任务管理抽象。
  - 更容易把结果并入后续快照刷新与测试建模。
- 无论采用哪种方式，其他并发请求都不应重复创建实例；其返回结果可以是：
  - 等待首个加载完成后返回 `AlreadyLoaded` 或 `FactoryFailed`。
  - 或立即返回 `AlreadyLoading`（若新增该状态）。
- 推荐不要新增 `AlreadyLoading` 暴露给上层，而是内部等待并折叠为最终结果，减少调用方复杂度。

### 4. 批量加载语义
- `load_deferred_by_category*` 不应再只返回 `()`；建议返回结构化统计结果，例如：
  - `requested`
  - `loaded`
  - `already_loaded`
  - `not_found`
  - `failed`
- 批量流程建议分两阶段：
  - 先基于 category 索引或快照计算候选名称集合。
  - 再逐个调用统一的单工具迁移事务。
- 批量接口的价值是“按类别编排”，不应另起一套与单工具迁移不同的一致性语义。

### 5. ToolSearch 联动改造
- `tool_search.rs` 当前逻辑是：
  - `resolve_deferred_async(name)` 返回 true -> Loaded
  - 否则再查 `has_loaded_tool_async(name)` -> AlreadyLoaded / NotFound
- 改造后应直接基于结构化结果生成用户文案，避免双查询。
- `load:category:*` 路径也应输出结构化摘要，而不是仅返回“Loaded all tools for category”。这样更利于调试和评审。

### 6. 日志与错误边界
- Registry 层仅在真正不可恢复或需要诊断并发异常时记录错误日志。
- 对于预期分支（AlreadyLoaded、NotFound、可重试的 FactoryFailed），优先返回结构化结果，由调用层决定是否展示给模型或记录 debug 级信息。
- 目标是避免同一失败在 registry、ToolSearch、runtime 三层重复打印。

## 测试案例
- 正常路径：
  - 单工具 resolve 返回 `Loaded`，随后在 loaded 集合与快照中可见。
  - category 批量加载返回的统计结果与实际迁移数量一致。
- 边界条件：
  - 并发 100 次 resolve 同一工具，最终 factory 仅执行一次。
  - 工具已 loaded 时再次 resolve，不重复创建实例，且结果稳定为 `AlreadyLoaded`。
- 异常场景：
  - factory 失败后 deferred 条目保持一致，可再次重试。
  - 批量加载中某个工具失败，不影响其他工具结果统计，也不破坏整体快照结构。
