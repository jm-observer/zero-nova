# Agent Skill Tool 能力统一设计

## 时间

- 创建时间：2026-05-16
- 最后更新：2026-05-16

## 项目现状

当前 Agent、Skill、Tool 机制存在以下结构性问题：

1. **能力来源分散**
   - agent 的 `prompt_file`、`tool_whitelist`、`enable_project_developer_prompt` 在 `gateway.agents` 中配置
   - skill 的 `tool_policy` 在 `.nova/skills/*/SKILL.md` 中配置
   - turn 级工具可见性又会在 `prepare_turn()` 中基于 active skill 再做一次裁剪

2. **同名能力在不同入口下不一致**
   - 根 agent runtime 与子 agent runtime 的工具注册路径不同
   - `tool_whitelist` 只对某些 agent 生效，导致 agent 间工具集不稳定
   - `ToolPolicy::AllowListWithDeferred` 的声明语义与当前 turn 实际可见工具不完全一致

3. **职责边界不清**
   - prompt 差异、skill 注入、tool 可见性、tool 注册四件事交叉耦合
   - 某些行为差异来自 prompt，某些来自 registry，某些来自 turn 裁剪，调试成本高

4. **显式 skill 请求不稳定**
   - `/orchestrator` 一类请求是否真正可执行，取决于当前 agent prompt、skill 路由命中、tool 是否已注册且本轮可见
   - 用户难以建立稳定预期

## 整体目标

本次重构的目标是将 Agent / Skill / Tool 机制收敛为以下模型：

1. **所有 agent 共享同一套 skill registry**
2. **所有 agent 共享同一套已注册 tool 集**
3. **不再按 agent 或 turn 做 tool whitelist / skill-based tool filtering**
4. **保留 agent prompt 差异，但 prompt 差异只负责职责偏好，不再负责能力隔离**
5. **仅保留最小运行时安全约束，例如写文件前置读取、危险操作确认、递归编排保护**

重构后的核心原则：

- **能力统一，职责区分**
- **注册即共享，可见即稳定**
- **skill 影响行为提示，不影响工具可见性**
- **agent prompt 影响策略偏好，不影响基础能力**

## Plan 拆分

| Plan | 描述 | 依赖 | 顺序 | 状态 |
|------|------|------|------|------|
| Plan 1 | 移除 agent 级和子 agent 级工具白名单路径，统一根 runtime 与子 runtime 的工具注册模型 | 无 | 1 | 已完成 |
| Plan 2 | 删除 turn 级 skill tool 过滤逻辑，将 active skill 收敛为 prompt / metadata 概念，不再裁剪工具集 | Plan 1 | 2 | 已完成 |
| Plan 3 | 收敛 prompt 与 skill 行为规则，保留角色差异但统一能力语义，并补齐测试与长期设计资产 | Plan 1, Plan 2 | 3 | 已完成 |

## 风险与待定项

- 去掉工具白名单后，子 agent 可能滥用 `Agent` 或编排能力，需要保留最小递归保护策略
- 去掉 skill-based tool filtering 后，模型可见工具更多，可能增加误用概率，需要通过 prompt 软约束和少量意图级规则收敛
- `ToolPolicy` 是否完全删除，还是先保留结构但降级为仅影响 prompt 展示，需要在实施时定稿
- `enable_project_developer_prompt` 是否继续保留为 agent 级差异项，目前建议保留
- 长期设计资产预计至少需要更新：
  - `docs/design/system-overview.md`
  - `docs/design/nova-agent-engine-boundaries.md`
