# 多 Agent 编排修复方案

- **时间**：2026-05-06
- **状态**：待评审

---

## 项目现状

当前仓库已经落入了一部分多 Agent 编排实现，但设计文档、后端事件链路、前端消费逻辑和 Skill 契约之间存在明显偏差，导致功能处于“代码存在、能力不可用”的状态。

### 已确认的问题

| 问题 | 位置 | 现象 |
|---|---|---|
| 编排事件未走正式协议链路 | `crates/nova-agent/src/orchestrator/mod.rs`、`crates/nova-agent/src/tool/builtin/agent.rs`、`crates/nova-gateway-core/src/bridge.rs` | 后端将编排事件伪装成 `system_log` / `tool_log`，前端无法按 `orchestration_*` 事件消费 |
| 前后端字段命名不一致 | `crates/nova-agent/src/orchestrator/mod.rs`、`deskapp/src/ui/orchestration-view.ts` | 后端发送 camelCase，前端读取 snake_case，导致 Plan/Agent 状态不渲染 |
| Skill 契约与解析器不一致 | `.nova/skills/orchestrator/SKILL.md`、`crates/nova-agent/src/orchestrator/planner.rs` | Skill 示例输出 snake_case，解析器只接受 camelCase |
| `OrchestrateTask` 工具缺失 | `.nova/skills/orchestrator/SKILL.md`、`crates/nova-agent/src/tool/builtin/mod.rs` | Skill 引用了不存在的工具，显式触发路径不可执行 |
| 调度失败语义不完整 | `crates/nova-agent/src/orchestrator/scheduler.rs`、`crates/nova-agent/src/orchestrator/mod.rs` | 子 Agent 失败后直接短路返回，无法形成完整阶段结果，也无法支撑 review/retry |
| 编排测试缺口明显 | `crates/nova-agent/tests`、`deskapp/src/__tests__` | 没有覆盖协议事件、Skill 入口、前端事件消费和失败路径 |

### 修复原则

1. **先修契约，再修实现**：先统一事件名、字段名、JSON Schema 和工具入口，再修执行逻辑，避免边改边漂移。
2. **单次修复保持聚焦**：每个 Plan 聚焦一层职责，不把协议、调度、前端和 Skill 混成一个大改动。
3. **以正式消息通路为准**：编排事件必须进入 `ProgressEvent` 标准链路，禁止继续依赖日志字符串承载结构化事件。
4. **以 Rust 类型为 source of truth**：前后端共享结构必须从 Rust 协议类型导出，Skill 文档示例也必须与协议保持一致。

---

## 整体目标

本次修复不是继续扩功能，而是把现有多 Agent 编排能力修到“可触发、可执行、可展示、可验证”的可用状态：

1. `orchestration_*` 事件通过正式 `ProgressEvent` 链路送达前端
2. Skill、工具、解析器和协议字段命名完全一致
3. `OrchestrateTask` 作为真实工具存在并可被 Skill 调用
4. 调度器在成功、失败、取消三类路径下都能产出可消费的完整结果
5. 前端 Agent 树与进度 UI 能正确渲染
6. 后端与前端补齐回归测试，修复流程可稳定通过

---

## Plan 拆分

| Plan | 标题 | 职责 | 依赖 | 状态 |
|---|---|---|---|---|
| **Plan 1** | 协议契约收敛 | 统一 Rust 协议类型、事件字段、前端消费字段和 schema 导出 | 无 | 待开始 |
| **Plan 2** | 工具入口与 Skill 对齐 | 实现 `OrchestrateTask`，修复 Skill 文档与工具注册 | Plan 1 | 待开始 |
| **Plan 3** | 调度执行语义修复 | 修正事件发射、失败聚合、取消传播和 review 输入 | Plan 1, Plan 2 | 待开始 |
| **Plan 4** | 前端展示与回归验证 | 修正事件路由/UI 渲染并补齐前后端测试 | Plan 1, Plan 3 | 待开始 |

执行顺序：Plan 1 → Plan 2 → Plan 3 → Plan 4

---

## 修复范围

### 后端

- `crates/nova-protocol`
- `crates/nova-agent`
- `crates/nova-gateway-core`

### 前端

- `deskapp/src/core`
- `deskapp/src/services`
- `deskapp/src/ui`
- `deskapp/src/__tests__`

### 文档与 Skill

- `.nova/prompts/agent-nova.md`
- `.nova/skills/orchestrator/SKILL.md`
- 新增本目录下修复文档

---

## 关键决策

### 1. 统一使用 camelCase

现有 Rust 协议结构、前端 generated schema 和 gateway 消息整体都偏向 camelCase。为减少额外兼容层，本次修复统一采用 camelCase：

- `planId`
- `stageId`
- `agentId`
- `subagentType`
- `outputSummary`
- `dependsOn`
- `contextFiles`

Skill 文档与前端消费逻辑全部向该约定收敛。

### 2. 编排事件必须是 `ProgressEvent`

编排事件不再通过 `SystemLog` / `LogDelta` 中转 JSON 字符串，而应直接构造：

```rust
ProgressEvent {
    kind: "orchestration_plan".to_string(),
    args: Some(...),
    ..
}
```

若现有 `AgentEvent` 无法表达该能力，应新增明确的结构化事件分支，再由 gateway 映射到 `ChatProgress`。

### 3. 保留阶段失败结果，而非立即丢失上下文

调度器遇到单个子 Agent 失败时，不应直接丢弃已完成结果并用 `?` 短路；需要形成带状态的 `SubAgentResult` 集合，并让上层决定：

- 是否发出 `stage_complete(allSuccess=false)`
- 是否继续 review
- 是否执行 retry 策略
- 是否提前终止后续 stage

当前版本先不实现自动 retry，但必须保留可扩展的失败结果模型。

---

## 风险与待定项

| 类型 | 描述 | 缓解措施 |
|---|---|---|
| 协议破坏性 | 事件字段从 snake_case 切到 camelCase，可能影响未搜索到的前端路径 | 先全局搜索所有 `agent_id` / `stage_id` / `plan_id` 消费点，再补测试 |
| 工具入口耦合 | `OrchestrateTask` 需要拿到 `AgentTool`、`ToolContext` 和事件发送器 | 将编排执行封装在独立 Tool 内，避免 Skill 直接拼接内部实现 |
| 失败语义变更 | 从“遇错即返回”改为“收集失败结果后上抛”可能影响现有错误处理 | 为调度器新增单测，明确成功/失败/取消行为 |
| 前端状态歧义 | 多 Plan 并发时仅靠 `agentId` 查找可能冲突 | 统一使用 `planId + agentId` 定位，至少在内部状态里保留 plan 维度 |
| 文档迁移 | 旧设计文档仍使用 snake_case/旧事件描述 | 本轮先新增修复文档，实施时同步更新原设计文档状态与备注 |

