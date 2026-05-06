# 多 Agent 并行编排系统

- **时间**：2026-05-06
- **状态**：设计阶段，待评审

---

## 项目现状

### 已有能力（可直接复用）

| 能力 | 位置 | 说明 |
|---|---|---|
| `AgentTool` 子代理执行 | `nova-agent/src/tool/builtin/agent.rs` | 可同步调用子 Agent，`run_in_background` 字段已定义但**未实现** |
| `TaskStore` 依赖图 | `nova-agent/src/tool/builtin/task.rs` | `blocks`/`blocked_by` 字段已存在，支持 DAG 表达 |
| `FuturesUnordered` 并行工具调用 | `nova-agent/src/agent.rs:execute_tool_calls()` | 工具层已并行，需扩展至 Agent 层 |
| `CancellationToken` | `nova-agent/src/agent.rs` | 支持中途取消单个 turn |
| `AgentRegistry` 多 Agent 注册 | `nova-agent/src/agent_catalog.rs` | 支持多类型 Agent 配置和切换 |
| Skill 系统 | `nova-agent/src/skill.rs` | `CapabilityPolicy` 动态控制工具可见性 |
| 协议事件流 | `nova-protocol/src/chat.rs` | `ProgressEvent` 已支持多种 kind 字段 |

### 核心缺口

1. `AgentTool.run_in_background` 未实现 → 无法并行执行多个子 Agent
2. 无 Orchestrator 角色定义 → 无任务拆分/分派/聚合逻辑
3. 无子 Agent 间协调协议 → 结果回传、错误传播机制缺失
4. 协议层无编排事件 → 前端无法感知 Agent 树结构和执行状态
5. 前端无 Agent 图可视化组件

---

## 整体目标

实现一个**多 Agent 编排系统**，当本地大模型面对复杂任务时，能够：

1. **自动或显式拆分**任务为 DAG（有向无环图），独立子任务并行执行，有依赖的子任务串行执行
2. **透明展示**子 Agent 的生成、执行、结果，用户在前端看到完整 Agent 树和执行进度
3. **Review Agent** 汇总所有子 Agent 输出，进行一致性检查并决策是否需要重试
4. 整个能力通过 **Skill 激活 + 提示词感知** 的混合方式触发

---

## 触发机制决策

**结论：混合方式（提示词感知 + Skill 触发）**

```
agent-nova.md（基础提示词）
  └─ 简短声明：复杂任务可使用编排 Skill
        ↓ 触发条件满足
  Orchestrator Skill（.nova/skills/orchestrator/SKILL.md）
        └─ 完整的编排协议、任务分解格式、并行/串行规则
```

**为何不纯提示词：**
- 编排协议完整描述约 500-1000 token，每轮请求都携带会浪费上下文
- 简单任务无需感知编排逻辑

**为何不纯 Skill：**
- 用户无法预知何时应触发编排，Agent 应能自主判断
- 基础提示词需声明"有此能力"，否则 Agent 不会考虑使用

**混合方案：**
- `agent-nova.md` 新增一节（约 5 行），声明复杂任务的编排能力
- Agent 在判断任务复杂度足够时，主动通过 `Skill` 工具激活 Orchestrator
- 用户也可显式输入 `/orchestrator <任务>` 强制激活

---

## Plan 拆分

| Plan | 标题 | 职责 | 依赖 |
|---|---|---|---|
| **Plan 1** | 协议与数据模型 | 新增编排相关协议事件、扩展 `AgentTool` 输入 Schema | 无 |
| **Plan 2** | Orchestrator 核心逻辑 | 实现 `run_in_background`、编排器 crate、DAG 调度 | Plan 1 |
| **Plan 3** | 提示词与 Skill 设计 | 更新 `agent-nova.md`、创建 `orchestrator` Skill | Plan 1 |
| **Plan 4** | 前端展示 | Agent 树 UI、并行/串行视觉区分、进度流 | Plan 1 |

执行顺序：Plan 1 → Plan 3（可与 Plan 2 并行）→ Plan 2 → Plan 4

---

## 系统架构总览

```
用户输入（复杂任务）
        │
        ▼
  Orchestrator Agent（激活 Orchestrator Skill）
        │
        │  1. 任务分析 → 输出 OrchestrationPlan JSON
        │  2. 构建 DAG（Stage 有依赖 → 串行；无依赖 → 并行组）
        │
        ├──────────────────────────────────┐
        │         Stage 1（并行组）         │
        │  ┌─────────┐  ┌─────────┐       │
        │  │SubAgent A│  │SubAgent B│       │
        │  │处理模块X │  │处理模块Y │       │
        │  └────┬────┘  └────┬────┘       │
        │       └──────┬─────┘            │
        │              │ 等待全部完成       │
        │              ▼                   │
        │        Stage 2（串行）            │
        │         SubAgent C               │
        │      依赖 A、B 的输出            │
        └──────────────────────────────────┘
                       │
                       ▼
              Review Agent（汇总评审）
                       │
              ┌────────┴────────┐
              │ 通过             │ 失败
              ▼                 ▼
           返回结果         重试失败子任务
```

---

## 核心数据结构（跨 Plan 公用）

```rust
/// 编排计划（Orchestrator → 调度器）
pub struct OrchestrationPlan {
    pub plan_id: String,
    pub description: String,             // 整体任务描述
    pub stages: Vec<ExecutionStage>,     // 按执行顺序排列
}

pub struct ExecutionStage {
    pub stage_id: String,
    pub mode: StageMode,                 // Parallel | Serial
    pub agents: Vec<SubAgentRequest>,
    pub depends_on: Vec<String>,         // 依赖的 stage_id
}

pub enum StageMode { Parallel, Serial }

pub struct SubAgentRequest {
    pub agent_id: String,               // 在 Plan 内唯一
    pub subagent_type: String,
    pub description: String,
    pub prompt: String,
    pub context_files: Vec<String>,     // 相关文件路径（辅助上下文裁剪）
}

/// 子 Agent 执行结果
pub struct SubAgentResult {
    pub agent_id: String,
    pub stage_id: String,
    pub status: SubAgentStatus,
    pub output: String,
    pub error: Option<String>,
}

pub enum SubAgentStatus { Success, Failed, Cancelled }
```

---

## 风险与待定项

| 类型 | 描述 | 缓解措施 |
|---|---|---|
| **文件冲突** | 并行子 Agent 可能同时写同一文件 | Orchestrator 分配互斥文件范围；或 `worktree isolation` |
| **上下文爆炸** | Review Agent 需汇总所有子任务输出 | 每个子 Agent 强制输出结构化摘要而非全文 |
| **Orchestrator 幻觉** | Orchestrator 本身也是 LLM，分解结果可能不合理 | 提供严格的 JSON Schema 约束 + 分解前确认步骤 |
| **本地模型 JSON 能力** | 本地模型生成合规 JSON 能力弱 | 提供少样本示例；支持解析失败时降级为单 Agent |
| **worktree isolation 未实现** | 并行写入存在竞争 | Plan 2 需评估是否先实现文件锁或分配互斥范围 |
| **前端性能** | 大量 Agent 同时流式输出 | 前端限流 + 折叠展示 |
