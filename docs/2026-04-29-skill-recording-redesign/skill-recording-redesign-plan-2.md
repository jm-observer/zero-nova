# Plan 2: 写入链路与幂等合并

## 前置依赖
- Plan 1: 会话级模型与兼容策略

## 本次目标
在会话运行时稳定采集 Skill，并幂等合并到 `ControlState.skill_bindings`，覆盖 turn-context 新路径和旧路径。

## 涉及文件
- `crates/nova-agent/src/conversation/service.rs`
- `crates/nova-agent/src/app/conversation_service.rs`

## 详细设计
1. **更新接口扩展**
- 扩展 `SessionService::update_runtime_state`：新增参数 `new_skills: Option<Vec<serde_json::Value>>`。
- 单次调用内执行：读取当前 `skill_bindings` -> 合并 -> 去重 -> 写回 -> 持久化。

2. **去重与覆盖规则**
- 去重主键：`skill_id`。
- 若 `skill_id` 已存在，按“新值覆盖旧值”更新 `name/status/description`。
- 无 `skill_id` 的非法项直接跳过，并记录 `warn` 日志（不阻断主流程）。

3. **双路径采集**
- `use_turn_context = true`：从 `turn_context_to_snapshot(...).skills` 采集。
- `use_turn_context = false`：从运行事件（如 `SkillActivated/SkillSwitched/SkillExited`）聚合本轮技能变化，回合结束时统一提交。

4. **写入时机**
- 回合开始后、模型调用前可写入一版初始快照（若可得）。
- 回合结束后再写入最终快照，保证状态收敛。

## 测试案例
- **幂等测试**：同一 `skill_id` 多次写入最终仅保留一条。
- **覆盖测试**：同一 `skill_id` 状态变化后字段按最新值更新。
- **双路径测试**：新旧执行路径均能产生持久化技能记录。
- **容错测试**：非法 `skill` 数据不会中断回合，且不会污染存储。
