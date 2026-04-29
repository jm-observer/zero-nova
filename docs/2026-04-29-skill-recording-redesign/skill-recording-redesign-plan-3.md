# Plan 3: 查询接口与实时事件

## 前置依赖
- Plan 2: 写入链路与幂等合并

## 本次目标
让前端 Skill 列表读取会话级真相，并在状态更新后获得实时通知。

## 涉及文件
- `crates/nova-agent/src/app/agent_workspace_service.rs`
- `crates/nova-agent/src/app/snapshot_assembler.rs`
- `crates/nova-agent/src/app/types.rs`
- `crates/nova-agent/src/app/conversation_service.rs`

## 详细设计
1. **读取路径解耦**
- `AgentWorkspaceService::list_session_skill_bindings` 直接读取 `control.skill_bindings`。
- 不再依赖 `runtime.last_turn.skills` 中转，避免语义混淆。

2. **Runtime 快照策略**
- `RuntimeSnapshotAssembler` 维持 `last_turn` 语义不变。
- 如需在 runtime 中展示会话级技能，新增独立字段（需同步协议），不复用 `last_turn.skills`。

3. **事件广播**
- 每次 `skill_bindings` 实际发生变化后，发送 `AppEvent::SessionSkillBindingsUpdated`。
- 事件体包含完整当前列表（而非仅 diff），前端可无状态刷新。
- 事件发送失败仅记录日志，不影响主业务返回。

4. **前端消费约束（接口契约）**
- 首次进入页面先拉取 `list_session_skill_bindings`。
- 之后仅用 `SessionSkillBindingsUpdated` 增量刷新视图；断线重连后再次全量拉取。

## 测试案例
- **读取正确性**：`last_turn` 为空时依然返回会话级 Skill 列表。
- **实时性**：技能状态变更后可收到事件且数据与存储一致。
- **一致性**：连续多次变化下，事件最终态与接口拉取结果一致。
