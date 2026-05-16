# 2026-05-16 Agent Skill Tool 能力统一

## 背景

当前 `zero-nova` 中 agent、skill、tool 通过多层配置和 turn 级裁剪共同决定最终能力集合，导致：

- 不同 agent 的工具集不稳定
- 同一 skill 在不同 turn 中可见性不一致
- 主 agent 与子 agent 行为分歧过大
- `/orchestrator` 等显式 skill 请求缺乏稳定预期

## 决策

采用“能力统一，职责区分”模型：

- 所有 agent 共享同一套 skill registry
- 所有 agent 共享同一套已注册 tool 集
- 不再按 agent 或 turn 做工具白名单 / skill-based tool filtering
- 不再保留 deferred tool / ToolSearch 双态设计，统一使用 loaded tool 模型
- 保留 agent prompt 差异，仅用于职责偏好表达
- 仅保留最小运行时安全约束

进一步约束：

- skill prompt 只表达工作流建议和使用规则，不再暗示工具白名单语义
- agent prompt 只表达角色偏好，不再暗示能力缺失或专属能力

## 影响范围

- `gateway.agents` 配置模型
- 根 runtime 与子 agent runtime 的工具注册流程
- `prepare_turn()` 与 active skill 处理逻辑
- prompt 构建与 skill 注入的职责边界
- `ToolPolicy` / `CapabilityPolicy` 的语义定义
- `.nova/prompts/` 与 `.nova/skills/` 中的能力说明文案
- `ToolRegistry` 与 CLI 中对 loaded/deferred 双态的观测逻辑

## 取舍

放弃方案：

- 保留现有多层工具裁剪，只修补 `AllowListWithDeferred`
  - 原因：只能修表面问题，不能降低整体复杂度
- 按 agent 保留不同工具集
  - 原因：继续增加调试成本，无法保证显式 skill 请求稳定

保留方案：

- agent prompt 角色差异
  - 原因：能力统一后仍需要行为偏好区分

## 文档同步

需要更新：

- `docs/design/system-overview.md`
- `docs/design/nova-agent-engine-boundaries.md`

已同步：

- `docs/design/system-overview.md`
- `docs/design/nova-agent-engine-boundaries.md`

## 关联项

- `docs/2026-05-16-unified-agent-skill-tool-capability/unified-agent-skill-tool-capability.md`
- `docs/2026-05-16-unified-agent-skill-tool-capability/unified-agent-skill-tool-capability-plan-1.md`
- `docs/2026-05-16-unified-agent-skill-tool-capability/unified-agent-skill-tool-capability-plan-2.md`
- `docs/2026-05-16-unified-agent-skill-tool-capability/unified-agent-skill-tool-capability-plan-3.md`
