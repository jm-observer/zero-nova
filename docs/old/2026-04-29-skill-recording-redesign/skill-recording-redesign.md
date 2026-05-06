# 详细设计：Agent 工作台 Skill 记录重构

- **时间**：2026-04-29（创建）/ 2026-04-29（最后更新）
- **状态**：待评审
- **主题**：修复 Agent 工作台 Skill 列表丢失，并明确会话级 Skill 状态模型

## 项目现状
当前 Skill 展示数据主要来自 `last_turn_snapshot.skills`。该字段语义是“最近一轮快照”，不适合表达“会话生命周期内 Skill 绑定状态”。当最近一轮无激活 Skill 时，前端列表会出现空或不完整。

## 整体目标
建立“会话级 Skill 绑定状态”的后端持久化模型，并通过稳定接口与事件推送同步给前端，确保以下目标：
- 刷新、重启、跨轮次后 Skill 列表不丢失
- `last_turn` 与“会话级 Skill 列表”语义解耦
- 读写路径幂等且可回归测试

## Plan 拆分

| Plan | 描述 | 依赖 | 执行顺序 | 状态 |
|------|------|------|------|------|
| [Plan 1: 会话级模型与兼容策略](./skill-recording-redesign-plan-1.md) | 在 `ControlState` 增加 `skill_bindings` 并定义兼容反序列化与字段约束 | 无 | 1 | 待开始 |
| [Plan 2: 写入链路与幂等合并](./skill-recording-redesign-plan-2.md) | 在回合执行中采集 Skill 并合并到 `skill_bindings`，覆盖两条执行路径 | Plan 1 | 2 | 待开始 |
| [Plan 3: 查询接口与实时事件](./skill-recording-redesign-plan-3.md) | `list_session_skill_bindings` 直接返回会话级 Skill，补齐事件广播与前端消费约束 | Plan 2 | 3 | 待开始 |
| [Plan 4: 测试与回归基线](./skill-recording-redesign-plan-4.md) | 增加兼容、幂等、重启恢复、事件时序测试，形成回归基线 | Plan 1-3 | 4 | 待开始 |

## 风险与待定项
- **协议兼容风险**：是否在 `SessionRuntimeSnapshot` 新增会话级字段，需与前端接口契约同步确认。
- **并发更新风险**：同会话多并发回合时，需确保 Skill 合并不丢写（单次写入前读后写原子化）。
- **数据膨胀风险**：长期会话可能累积较多 Skill 记录，需限制字段规模并保留最小必要信息。
- **事件一致性风险**：事件推送失败不应影响主流程，前端需支持主动拉取兜底。

## 非目标
- 本次不引入新数据库表，不做事件溯源分表。
- 本次不改动 Skill 路由策略与 Prompt 编排策略。
