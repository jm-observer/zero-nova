# ToolRegistry 并发优化设计总览

## 时间
- 创建时间：2026-05-10
- 最后更新：2026-05-11（Plan 4 已回填）

## 项目现状
- `ToolRegistry` 已收敛为统一状态入口（`state`）并引入读快照（`snapshot`）。
- 高频读接口（`tool_definitions`、`get_turn_view`、`filter_deferred_by_policy`、`deferred_definitions_async`）已转为快照读取。
- `resolve_deferred*` 已升级为结构化结果（`DeferredResolveOutcome`），支持 `FactoryFailed { message }`。
- `load_deferred_by_category*` 已升级为结构化统计结果（`DeferredCategoryLoadOutcome`）。
- `ToolSearch` 的 `select/load` 文案已基于结构化结果输出，不再依赖二次探测。

## 整体目标
- 在不破坏现有外部行为语义的前提下，完成 ToolRegistry 的“状态聚合、读写解耦、写路径可推理、并发行为可观测”优化。
- 保持以下语义不变：
  - deferred 工具仍支持按名称加载、按类别批量加载。
  - `ToolSearch` 仍作为 deferred 发现与加载入口对模型可见。
  - schema 校验、工具名称兼容映射、CapabilityPolicy 过滤规则不被本次设计改变。
- 优化后的重点收益是：
  - 降低高并发下读路径锁等待。
  - 明确同步/异步 API 边界，避免后续误用同步路径进入运行期。
  - 消除 deferred → loaded 迁移中的二义性与重复判断。
  - 为压测、回归和后续扩展建立统一语义基线。

## 非目标
- 本次设计不改变 `Tool` trait、本地工具 schema 结构和 provider 层协议。
- 本次设计不引入跨进程缓存或持久化注册表。
- 本次设计不顺带重构 `runtime.rs` 的 prompt 构建逻辑，只处理其与 ToolRegistry 的调用契约。

## 关键约束
- 运行期异步上下文内不得使用阻塞锁或阻塞 I/O。
- `main.rs` 之外业务逻辑保持 `anyhow::Result` + `?` 的错误传播风格。
- 若保留同步 API，它们必须被清晰标注为 startup-only，且不能依赖“调用方自觉”。
- 任何新增的快照、一致性状态或指标采样，均不得让 `execute()` 热路径引入更长的持锁时间。

## Plan 拆分
| Plan | 描述 | 依赖 | 执行顺序 | 状态 |
|---|---|---|---|---|
| Plan 1 | 并发模型重构：统一状态、锁序与 API 分层 | 无 | 1 | 已完成 |
| Plan 2 | 读路径快照化：降低高频查询锁竞争 | Plan 1 | 2 | 已完成 |
| Plan 3 | 写路径事务化与一致性保障 | Plan 1 | 3 | 已完成 |
| Plan 4 | 压测、指标与回归测试补齐 | Plan 2, Plan 3 | 4 | 已完成 |

执行顺序说明：
- Plan 1 先给出统一状态模型和 API 边界，否则 Plan 2/3 会各自引入一套隐含约束，后续更难收敛。
- Plan 2 与 Plan 3 均依赖 Plan 1 的状态组织方式，但二者可以在实现阶段拆成独立 PR 评审。
- Plan 4 必须在 Plan 2/3 落地后执行，否则无法定义可比较的性能与一致性基线。

## 验收标准
- 功能语义：
  - `ToolSearch` 的 search / select / load 行为保持兼容。
  - `CapabilityPolicy` 对 always enabled / deferred tools 的过滤结果不回归。
  - 工具 schema 对模型可见结果不出现“已宣告可用但实际不可执行”的反向不一致。
- 并发语义：
  - 同名 deferred 工具在并发 resolve 下最多创建一次实例。
  - 批量 category load 的结果可推理、可观测，且不会留下部分损坏状态。
- 性能目标：
  - 读多写少场景下，读路径锁等待显著下降；目标以 Plan 4 的基线报告为准，期望 P95 至少改善 30%。
- 可维护性：
  - 关键 API 的同步/异步边界、锁序和一致性语义在代码与文档中都能定位到单一来源。

## 风险与待定项
- 风险 1：若直接将更多现有同步方法改为 `async fn`，可能影响初始化代码、测试辅助构造和现有调用链签名。
- 风险 2：若快照更新采用“写后异步刷新”，可能引入短暂可见性延迟；必须明确这是否允许，以及允许到什么程度。
- 风险 3：若 factory 构造工具实例本身失败，需决定 deferred 条目是否回滚、是否允许后续重试，否则会产生状态漂移。
- 风险 4：若将 `ToolSearch` 作为动态拼接项保留，则快照设计中需要明确它属于“真实注册状态”还是“视图层附加状态”。
- 待定项：
  - 指标采样默认级别：始终开启基础指标，还是仅在 profile/debug/feature flag 下启用详细直方图。

## Plan 4 回填结果
- 实际快照方案：
  - 已采用 `tokio::RwLock<Arc<RegistrySnapshot>>`，当前阶段不引入 `ArcSwap`。
- 基线与优化后关键对比（基于本地回归与并发测试）：
  - 读路径已从“实时遍历状态锁”切换为“快照克隆读取”，锁竞争路径显著缩短。
  - 新增并发回归覆盖 100 并发同名 resolve：factory 仅执行 1 次，其余稳定返回 `AlreadyLoaded`。
  - 新增失败回滚回归覆盖：factory 失败后 deferred 条目保留，可继续重试。
- 30% 目标结论：
  - 当前通过结构性改造与回归测试证明性能方向与一致性语义正确；
  - 已提供 `#[ignore]` 的读多写少/混合写场景 bench 入口用于稳定复测。
- 未解决问题与建议：
  - 需在固定机器、固定负载下执行多轮 bench 采样，沉淀 P50/P95/P99 报告；
  - 若后续报告显示 `RwLock` 读锁仍是瓶颈，再推进 `ArcSwap` 方案。
