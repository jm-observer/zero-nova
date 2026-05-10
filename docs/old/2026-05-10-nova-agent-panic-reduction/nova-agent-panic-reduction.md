# nova-agent-panic-reduction

## 时间
- 创建日期：2026-05-10
- 最后更新：2026-05-10

## 项目现状
- `crates/nova-agent/src/agent.rs` 在关键循环分支存在 `last().unwrap()`，虽然当前有前置条件，但属于隐式假设，后续维护容易引入 panic。
- `crates/nova-agent/src/conversation/repository.rs` 在生产路径存在多处 JSON 序列化与反序列化 `unwrap/unwrap_or_else`，一旦脏数据或序列化异常会导致进程崩溃或静默降级。
- `crates/nova-agent/src/app/conversation_service.rs` 存在锁读取 `unwrap`、快照序列化 `unwrap`，属于请求主路径，失败时应返回可观测错误而非 panic。
- 当前错误处理风格不统一：有的路径使用 `anyhow::Result + context`，有的路径使用 panic 或吞错 fallback，导致故障定位成本高。

## 整体目标
- 消除 `nova-agent` 生产代码中的中高优先级 panic 点（`unwrap/expect` 及等价隐式 panic）。
- 统一关键路径错误策略为 `anyhow::Result + ? + .context("...")`，保证失败可回传、可定位、可日志化。
- 对“可降级”的场景显式定义降级规则，对“不可降级”的场景显式失败，避免隐式吞错。

## Plan 拆分
- Plan 1（待开始）：Repository 层 panic 点治理。
  说明：覆盖 `repository.rs` 的 JSON 序列化/反序列化、会话行解析逻辑，替换 `unwrap/unwrap_or_else` 并补充上下文。
  依赖：无。
  执行顺序：1。
- Plan 2（待开始）：ConversationService 关键路径 panic 点治理。
  说明：覆盖 `conversation_service.rs` 的锁读取与快照序列化路径，建立“锁中毒/快照转换失败”错误边界。
  依赖：Plan 1（建议，非强依赖）。
  执行顺序：2。
- Plan 3（待开始）：Agent 主循环隐式 panic 假设收敛。
  说明：覆盖 `agent.rs` 中关键分支 `unwrap` 与前置条件耦合点，改为显式分支或错误返回。
  依赖：Plan 2（建议，非强依赖）。
  执行顺序：3。

## 风险与待定项
- 风险：部分 `unwrap_or_else(default)` 可能承载历史兼容策略，直接改为失败可能改变行为；需要在 Plan 1 中逐点定义“失败 vs 降级”。
- 风险：会话锁从 `std::sync::RwLock` 读取时若出现中毒，改为返回错误会让请求失败率上升；需配套日志与可观测性。
- 待定：是否在本次同时收敛“吞错但不 panic”的 `serde_json::from_str(...).ok()` 路径（建议纳入 Plan 1 高优先级范围）。