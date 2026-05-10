# Plan 1: Repository 层 panic 点治理

## Plan 编号与标题
- Plan 1: Repository 层 panic 点治理

## 前置依赖
- 无

## 本次目标
- 去除 `crates/nova-agent/src/conversation/repository.rs` 生产代码中 `unwrap/expect`。
- 将 JSON 编解码失败纳入 `anyhow::Result` 返回链，并附带最小充分上下文（表名、字段名、记录标识）。
- 对历史脏数据读取场景建立一致策略：可降级则带告警降级，不可降级则失败返回。

## 涉及文件
- `crates/nova-agent/src/conversation/repository.rs`
- `crates/nova-agent/src/conversation/repository.rs`（同文件内单元测试按需补充）

## 详细设计
1. 目标点清单（生产代码）
- `list_sessions()`：`serde_json::from_str(&json).unwrap_or_else(...)`。
- `create_audit_log()`：`serde_json::to_string(&log.details).unwrap()`。
- `create_diagnostic_issue()`：`serde_json::to_string(v).unwrap()`。
- `save_workspace_restore_state()`：`serde_json::to_string(&state.snapshot).unwrap()`。
- `list_audit_logs()`：`serde_json::from_str(&details_json).unwrap_or(...)`（当前非 panic，但会吞错）。
- `parse_session_row()`：`serde_json::from_str(&json).unwrap_or_else(...)`。

2. 处理策略
- 序列化（to_string）失败：直接 `context("...")?` 返回错误。
- 反序列化（from_str）失败：按场景分级。
  - A. 关键控制状态（`runtime_control`）建议“可降级+告警”：返回默认 `ControlState::new(agent_id)`，同时 `log::warn!` 记录 session_id/agent_id/错误摘要。
  - B. 审计详情（`audit_logs.details`）建议“保留兼容降级”：降为 `Value::Null`，但增加 `log::warn!`，避免静默吞错。
  - C. 若调用方依赖强一致（新增接口），则改为失败返回，不做降级。

3. API 与签名影响
- 尽量保持现有函数签名不变（仍为 `Result<...>`），仅调整内部实现。
- `parse_session_row` 继续返回 `Result<SessionRow>`，但移除 panic 分支。

4. 可观测性
- 增加结构化告警关键字段：`session_id`、`agent_id`、`column`、`error`。
- 避免重复日志：Repository 只在“降级发生”时打印一次 warn；失败返回由上层决定是否记录 error。

## 测试案例
- 正常路径：合法 `runtime_control/details/snapshot` JSON 时，读写行为不变。
- 边界路径：`runtime_control` 为非法 JSON，`list_sessions` 与 `parse_session_row` 返回默认控制状态并记录 warn。
- 异常路径：构造 `serde_json::to_string` 失败场景（可用含非有限浮点等输入）时，相关接口返回 `Err`。
- 回归路径：现有 repository 单测全量通过，特别是 run/audit/diagnostic 相关用例。