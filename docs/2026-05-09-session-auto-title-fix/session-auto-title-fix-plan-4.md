# Plan 4: 回归测试与稳定性验证

## 前置依赖
- Plan 1
- Plan 2
- Plan 3

## 本次目标
- 补齐覆盖“重启幂等 + 推送事件 + 失败恢复”的自动化测试。
- 确保修复流程（clippy/fmt/test）在 workspace 维度全部通过。

## 涉及文件
- `crates/nova-agent/src/conversation/service.rs`（测试模块）
- `crates/nova-agent/src/conversation/control.rs`（测试模块）
- `crates/nova-agent/src/app/application.rs`（必要时补事件相关测试）
- `deskapp/src/core/state.ts` 或前端测试文件（必要时）

## 详细设计
- 后端测试新增：
  - `title_state_persists_after_service_rebuild`
  - `title_is_not_regenerated_after_success_and_reload`
  - `title_generation_failure_does_not_leave_pending_state`
  - `session_summary_updated_event_emitted_once_on_title_change`
- 前端联动测试（可选但推荐）：
  - 模拟收到 `session.summary.updated` 后，`state.sessions` 和聊天头部标题同步刷新。
- 非功能验证：
  - 并发压测场景下同一 session 不重复起多个标题任务。

## 测试案例
- 正常路径：
  - 二条有效用户消息触发生成成功，收到单次 summary 更新事件。
- 边界条件：
  - 重启后再次发消息，不会因为状态丢失重复改标题。
  - 第三条消息触发第二次尝试后成功。
- 异常路径：
  - 生成器返回错误时，聊天消息仍正常落库，接口返回成功。
  - 事件通道关闭时，不影响标题落库。

## 修复流程清单
- `cargo clippy --workspace -- -D warnings`
- `cargo fmt --all --check`
- `cargo test --workspace`

上述三步必须在 Plan 4 实施完成后连续通过，任一步失败需修复后重跑完整循环。
