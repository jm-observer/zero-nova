# Plan 4: 压测、指标与回归测试补齐

## Plan 编号与标题
- Plan 4: 压测、指标与回归测试补齐

## 前置依赖
- Plan 2
- Plan 3

## 本次目标
- 用可重复的数据验证并发优化收益。
- 将关键一致性场景沉淀为稳定测试，防止后续回归。
- 为是否需要从 `RwLock` 快照升级到 `ArcSwap` 提供证据，而不是凭感觉决策。

## 涉及文件
- `crates/nova-agent/src/tool/registry.rs`
- `crates/nova-agent/src/tool/registry_bench.rs`（如新增）
- `crates/nova-agent/src/tool/builtin/tool_search.rs`
- `docs/2026-05-10-tool-registry-concurrency-optimization/tool-registry-concurrency-optimization.md`

## 详细设计
### 1. 指标定义
- 至少记录以下指标：
  - 状态锁等待时间：P50 / P95 / P99。
  - `get_turn_view`、`tool_definitions`、ToolSearch 搜索路径的耗时分布。
  - 单工具 resolve 和 category load 的耗时分布。
  - 同名工具 factory 实际执行次数。
- 指标目标：
  - 既能观察性能，也能验证一致性假设，例如“同名工具只构建一次”。

### 2. 压测场景
- 场景 A：读多写少
  - 90% `get_turn_view` / `tool_definitions` / ToolSearch search
  - 10% `resolve_deferred_async`
- 场景 B：读写混合
  - 70% 读
  - 30% 写（单工具 resolve + category load 混合）
- 场景 C：突发批量加载
  - 连续触发 category load，并发穿插读请求，验证快照发布与写路径事务的稳定性。
- 场景 D：失败恢复
  - 插入可控 factory 失败工具，验证失败后可重试且不污染状态。

### 3. 回归测试分层
- 单元测试：
  - 验证结果枚举、状态迁移、失败回滚、快照替换语义。
- 并发集成测试：
  - 小规模固定并发数，验证不会重复构造、不会丢失 deferred 条目。
- 基准/压测：
  - 作为手动或 profile 门禁执行，不直接作为普通 CI fail 条件。
- 推荐将随机性较高的超大并发压测放在 bench 或专门命令中，避免 CI flaky。

### 4. 验收口径
- 功能正确性：
  - 所有既有测试通过。
  - ToolSearch 文案与行为与新返回语义一致。
- 一致性：
  - 并发 resolve 同名工具不重复构造。
  - factory 失败可重试，且 loaded/deferred/快照三者不漂移。
- 性能：
  - 读多写少场景下，P95 锁等待较基线下降至少 30%。
  - 若未达到目标，需要在文档中给出瓶颈定位结论，并决定是否推进 `ArcSwap`。

### 5. 结果回填
- Plan 4 完成后，需要把以下结果回填到总览文档：
  - 实际采用的快照方案。
  - 基线与优化后关键指标对比。
  - 是否达成 30% 目标。
  - 未解决问题与后续建议。
- 若最终发现 `RwLock` 快照已足够，则在文档中明确“暂不引入 `ArcSwap`”的依据，避免未来重复讨论。

## 测试案例
- 正常路径：
  - 三类压测场景均可执行并输出稳定指标。
- 边界条件：
  - 小工具集与大工具集都可完成压测，不因 schema 数量变化出现异常结果。
  - 并发 category load 与单工具 resolve 交错执行时，统计结果仍可解释。
- 异常场景：
  - 压测中途取消或超时后，系统状态可恢复，后续读写不受污染。
  - 故意注入 factory 失败时，不会把失败条目永久丢失。
