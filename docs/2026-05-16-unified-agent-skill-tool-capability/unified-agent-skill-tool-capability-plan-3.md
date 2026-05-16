# Plan 3: 收敛 Prompt / Skill 行为语义

## 前置依赖

- Plan 1: 统一 Tool 注册模型
- Plan 2: 删除 Turn 级 Tool 裁剪

## 任务目标

在统一能力模型下，收敛 agent prompt、skill prompt 注入与重构后长期设计资产，使系统满足“能力统一，职责区分”的目标。

完成后应满足：

- agent prompt 只表达职责偏好，不再暗示能力隔离
- skill prompt 只表达工作流与行为建议，不再依赖 tool 白名单语义
- 长期设计资产与设计影响记录完成同步

## 执行范围

- 必须修改：
  - `.nova/prompts/agent-nova.md`
  - `.nova/prompts/agent-developer.md`
  - `docs/design/system-overview.md`
  - `docs/design/nova-agent-engine-boundaries.md`
  - `docs/adr/2026-05-16-unified-agent-skill-tool-capability.md`
- 允许修改：
  - `.nova/skills/` 中直接引用旧白名单语义的说明文案
- 禁止修改：
  - 不要重新引入任何工具可见性裁剪
  - 不要把 prompt 文案变成新的隐式工具白名单

## Agent 执行步骤

1. 修改 `agent-nova.md` 与 `agent-developer.md`，明确所有 agent 共享统一能力集合，只保留职责建议
2. 检查 `.nova/skills/orchestrator/SKILL.md` 等 skill 文案，删除对“工具可见性受 skill 限制”的假设
3. 更新 `docs/design/system-overview.md`，补充统一能力模型说明与索引
4. 更新 `docs/design/nova-agent-engine-boundaries.md`，说明 agent / skill / tool 的新边界
5. 新增 `docs/adr/2026-05-16-unified-agent-skill-tool-capability.md`，记录本次设计决策、影响范围与取舍
6. 补齐回归测试，验证统一模型下 `/orchestrator` 等显式 skill 场景行为稳定

## 目标数据结构 / 接口契约

Prompt 语义契约：

- `agent-nova`：默认通用 agent，拥有统一能力集合
- `agent-developer`：开发任务偏好 agent，拥有统一能力集合
- skills：统一注册、统一可见，按输入触发或按模型决策使用

设计文档契约：

- `system-overview.md` 必须索引新的能力统一设计
- `nova-agent-engine-boundaries.md` 必须说明“注册统一、turn 不裁剪”的稳定边界

## 行为规则

| 输入 / 场景 | 期望结果 |
|------|----------|
| `nova` 处理 `/orchestrator ...` | 可稳定看到统一 skill / tool 集 |
| `developer` 处理 `/orchestrator ...` | 能力上与 `nova` 一致，只由 prompt 决定是否倾向直接执行或编排 |
| 普通编码任务 | 两类 agent 都共享完整能力集，但保持各自职责偏好 |
| `ToolInfo` / `Skill` 查询 | 在不同 agent 间结果一致 |

## 禁止事项

- 不要在 prompt 中重新加入“某 agent 禁止某工具”的文案
- 不要跳过长期设计资产更新
- 不要省略 ADR

## 测试要求

- 补充或修改测试，覆盖：
  - 主 agent / 子 agent 统一能力可见性
  - 显式 skill 触发场景的稳定性
  - `ToolInfo` / `Skill` 在不同 agent 下结果一致
- 必须执行：
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --check --all`
  - `cargo test --workspace`

## 完成条件

- [x] agent prompt 已收敛为职责偏好语义
- [x] skill 文案不再依赖工具白名单假设
- [x] 长期设计资产已更新
- [x] ADR 已新增
- [x] 统一能力模型回归测试通过
- [x] `cargo clippy --workspace -- -D warnings` 通过
- [x] `cargo fmt --check --all` 通过
- [x] `cargo test --workspace` 通过
