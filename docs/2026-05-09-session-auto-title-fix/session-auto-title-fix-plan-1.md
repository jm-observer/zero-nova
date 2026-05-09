# Plan 1: 标题状态单一数据源与持久化修复

## 前置依赖
- 无

## 本次目标
- 消除 `Session.title_state` 与 `ControlState.title_state` 的双写分叉，统一为单一数据源。
- 保证标题状态随 `runtime_control` 一并持久化，进程重启后可恢复幂等信息。
- 保持旧 `runtime_control` 数据反序列化兼容（缺失 `title_state` 时自动补默认值）。

## 涉及文件
- `crates/nova-agent/src/conversation/session.rs`
- `crates/nova-agent/src/conversation/control.rs`
- `crates/nova-agent/src/conversation/service.rs`
- `crates/nova-agent/src/conversation/repository.rs`

## 详细设计
- 数据源收敛策略：
  - 以 `ControlState.title_state` 作为唯一状态源（source of truth）。
  - 删除 `Session` 结构体中的 `title_state` 字段，避免同一语义状态跨两处锁对象维护。
- 访问路径调整：
  - 标题调度、成功回写、失败回写统一通过 `session.control.write()` 修改 `control.title_state`。
  - 会话构建（新建/加载/克隆）不再初始化独立 `TitleState` 锁。
  - `service.rs` 中所有 `session.title_state.*` 调用点改为经 `session.control` 访问。
- 持久化保证：
  - `repository::save_session` 与 `repository::update_session_runtime_control` 已写入 `runtime_control` JSON，继续复用该链路。
  - 每次标题状态更新后立即调用 `persist_runtime_control`（或等价持久化入口）确保状态落盘。
  - 修正误导性注释：`control.rs` / `session.rs` 中“title_state 不持久化”相关描述需更新为“通过 runtime_control 持久化”。
- 兼容性：
  - 依赖 `ControlState.title_state` 上的 `#[serde(default)]`，旧数据缺少该字段时自动回退默认值。
  - `repository.rs` 对 `runtime_control` 反序列化失败时继续回退 `ControlState::new(...)`，保证历史脏数据可读。
  - 默认值语义保持：`source=Default, status=Idle, attempt_count=0`。

## 测试案例
- 正常路径：
  - 新建会话后 `control.title_state` 为默认值，且 `Session` 不再暴露独立 `title_state` 字段。
  - 触发自动标题后 `attempt_count` 增加；重载同一会话后值保持一致。
- 边界条件：
  - 读取无 `title_state` 的 legacy `runtime_control` 不报错，并得到默认状态。
  - `runtime_control` JSON 解析失败时回退默认 `ControlState`，服务可继续工作。
- 异常路径：
  - 模拟标题生成失败后，重建 `SessionService` 重新加载会话，状态应为 `Failed` 且包含失败上下文（如错误信息）。
