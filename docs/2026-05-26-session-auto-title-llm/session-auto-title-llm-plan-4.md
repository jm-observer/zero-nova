# Plan 4: 测试 + 修复循环

## 前置依赖

Plan 1 / 2 / 3 全部完成。

## 任务目标

- 把 Plan 1/2/3 各自的新单测/集成测试合并跑通。
- 补一条端到端 smoke：从 `AgentApplicationImpl::start_turn` 入口出发，触发两条用户消息后断言 `SessionSummaryUpdated` 事件被发出，且 title 等于 mock LLM 返回值。
- 全量 `cargo clippy + fmt + test` 三件套全绿。

## 执行范围

**必须修改**：

- 沿用 Plan 1/2/3 的测试文件。
- 新增 `crates/nova-agent/tests/integration/session_auto_title_e2e.rs`（如已存在同名集成测试组织习惯，按既有目录结构落地）。

**禁止修改**：

- 任何 Plan 1/2/3 不在改动列表内的非测试代码。

## Agent 执行步骤

1. 在 `crates/nova-agent/tests/integration/` 下新增 `session_auto_title_e2e.rs`：
   - 启动 `wiremock` mock provider，返回 SSE 单 chunk `路由设计`。
   - 构造最小 AppConfig 指向 mock。
   - 构造 ConversationService → AgentApplicationImpl。
   - `create_session(None, "default")` → 触发两次 `start_turn`（mock turn 让 agent 跳过实际 turn 执行）。
   - 监听 sender，断言收到 `SessionSummaryUpdated { title: Some("路由设计"), ... }`。
2. 跑修复循环：
   ```bash
   cargo clippy --workspace -- -D warnings
   cargo fmt --check --all
   cargo test --workspace
   ```
3. 若任一步失败，定位修复后**重新跑完整循环**。

## 行为规则

| 场景 | 期望输出 |
|------|----------|
| 两条用户消息（合计 ≥ 24 char）触发 + mock 返回 `路由设计` | session.name == `路由设计`，发出 `SessionSummaryUpdated` |
| 一条短消息 | 不触发调度，无 `SessionSummaryUpdated` |
| mock 返回 5xx | title 状态 Failed(retryable)，下一条消息再次触发 |
| mock 在两次尝试都返回 5xx | `attempt_count == 2`，第三次用户消息不再触发 |

## 禁止事项

- 禁止把端到端测试改成直接调 `run_title_generation` 内部 API（要从公开入口出发）。
- 禁止跳过 fmt/clippy 仅跑 test。

## 测试要求

只有所有以下命令在最后一次运行全绿才视为完成：

```bash
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test --workspace
```

## 完成条件

- [ ] e2e smoke 通过
- [ ] 三件套全绿
- [ ] `docs/design/` 下相关基线文档（若存在 `nova-agent-engine-boundaries.md` 或 conversation 相关文档）已同步更新
- [ ] `docs/adr/2026-05-26-title-generator-injection.md` 已新增
- [ ] 总览文档 `Plan 拆分` 状态全部标记为「已完成」
