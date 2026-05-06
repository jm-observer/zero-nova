# Plan 4: 测试与回归基线

## 前置依赖
- Plan 1: 会话级模型与兼容策略
- Plan 2: 写入链路与幂等合并
- Plan 3: 查询接口与实时事件

## 本次目标
建立覆盖兼容性、幂等性、持久化恢复和事件一致性的测试基线，防止问题回归。

## 涉及文件
- `crates/nova-agent/src/conversation/control.rs`（单元测试）
- `crates/nova-agent/src/conversation/service.rs`（单元/集成测试）
- `crates/nova-agent/src/app/agent_workspace_service.rs`（接口测试）
- 现有测试目录中与 session/runtime 相关测试文件

## 详细设计
1. **兼容性回归**
- 构造旧版 `runtime_control` JSON（无 `skill_bindings`），验证加载成功。

2. **持久化回归**
- 写入技能后重建 `SessionService`（模拟重启），再次读取应保持一致。

3. **并发回归**
- 同一会话并发触发两次技能更新，最终集合不丢失、不重复。

4. **事件回归**
- 技能列表变化时应触发事件；无变化时不重复广播（降低噪音）。

5. **修复流程门禁**
- 每个 Plan 实施后执行：
  1. `cargo clippy --workspace -- -D warnings`
  2. `cargo fmt --check --all`
  3. `cargo test --workspace`

## 测试案例
- **正常路径**：技能首次激活 -> 列表出现。
- **边界路径**：同一技能重复激活 -> 列表不重复。
- **异常路径**：技能数据不完整 -> 忽略非法项并保留已有数据。
- **恢复路径**：应用重启后 -> 列表仍可完整读取。
