# 子 Agent Agent Routing 设计

- **时间**：2026-05-07
- **状态**：Plan 1 / Plan 2 已完成

---

## 项目现状

当前多 Agent 编排方案已具备 `AgentRegistry` 和 `[[gateway.agents]]` 配置基础，顶级 Agent 可通过 `.nova/config.toml` 预注册并绑定 `prompt_file`。但现有方案默认所有子 Agent 共享同一类系统提示词，缺少对子任务类型的区分，导致：

1. 开发类子任务拿不到足够强的工程约束
2. 通用提示词被重复下发给所有子 Agent，造成 token 浪费
3. 首版若直接引入独立 `subagent_profiles` 体系，会额外增加配置和运行时复杂度

结合当前实现阶段，首版目标不是构建完整 persona 平台，而是先验证“开发类子任务使用专门 Agent”这条最核心路径。

---

## 整体目标

在不新增独立子 Agent 配置体系的前提下，复用 `.nova/config.toml` 中的 `[[gateway.agents]]`：

1. 增加一个开发型 Agent，例如 `developer`
2. 编排器在生成子任务时，只在 `nova` 与 `developer` 之间做分配
3. 开发类子任务默认路由到 `developer`，其他子任务回退到 `nova`
4. 运行时对无效或缺失的子 Agent 标识提供稳定回退，避免编排失败

---

## Plan 拆分

| Plan | 标题 | 职责 | 依赖 | 状态 |
|---|---|---|---|---|
| **Plan 1** | 配置模型与提示词约束 | 定义 `gateway.agents` 复用策略、`developer` Agent 配置结构、提示词边界 | 无 | 已完成 |
| **Plan 2** | 编排分配与运行时回退 | 定义 Orchestrator 的 Agent 选择规则、运行时校验与降级路径 | Plan 1 | 已完成 |

执行顺序：Plan 1 → Plan 2

---

## 核心设计决策

### 1. 首版复用 `gateway.agents`

不新增 `subagent_profiles`。原因：

1. 首版只需要区分“开发类”与“其他”两类任务，单独抽一套 profile 体系收益不足
2. 现有配置已经支持 `id`、`description`、`prompt_file` 绑定，可直接承载开发型 Agent
3. 运行时只需按 `agent_id` 查表，不需要新增第二套配置解析逻辑

### 2. 首版只支持两类 Agent

- `nova`：默认通用 Agent，也是所有未命中规则任务的回退目标
- `developer`：开发型子 Agent，仅用于实现、修改、修复、补测试等工程任务

这意味着首版不单独引入 reviewer / researcher。需要评审、总结、普通问答的子任务，仍然走 `nova`。

### 3. 编排器负责“分配执行 Agent”，不负责生成系统提示词全文

实现阶段为了兼容现有协议，编排器通过 `subagent_type` 选择执行 Agent；`agent_id` 继续保留为 Plan 内唯一实例标识。真正的系统提示词仍由被选中的 Agent 通过 `prompt_file` 提供。

这样可以保持边界清晰：

1. 编排层：决定任务拆分和 Agent 分配
2. 配置层：定义 Agent 身份与提示词
3. 运行时：校验 `subagent_type`，加载对应 Agent 配置并执行

### 4. 默认回退必须内建

如果模型生成了未知 `subagent_type`，或根本没有填该字段，运行时必须自动回退到 `nova`，而不是让整个编排任务失败。

---

## 数据模型调整

首版实际落地时保留了两层标识：

```rust
pub struct SubAgentRequest {
    pub agent_id: String,
    pub subagent_type: String,
    pub description: String,
    pub prompt: String,
    pub context_files: Vec<String>,
}
```

说明：

1. `agent_id` 继续表示编排 Plan 内唯一实例标识，例如 `a1`、`a2`
2. `subagent_type` 用于选择已注册 Agent，首版只使用 `developer` 与 `nova`
3. 当 `subagent_type` 缺失时，解析层默认补成 `nova`
4. 当 `subagent_type` 非法时，运行时记录 warning 并回退 `nova`

---

## 路由流程

```text
用户复杂任务
    ↓
Orchestrator 拆分子任务
    ↓
根据规则给每个子任务分配 subagent_type
    ├─ 开发类任务 → developer
    └─ 其他任务 / 不确定 → nova
    ↓
运行时校验 subagent_type
    ├─ 配置存在 → 加载对应 prompt_file
    └─ 缺失 / 无效 → 回退 nova
    ↓
执行子 Agent
```

---

## 风险与待定项

| 类型 | 描述 | 缓解措施 |
|---|---|---|
| **角色不足** | 只有 `nova` 和 `developer`，后续评审型任务可能仍不够精细 | 首版先验证开发类收益，再评估是否新增 reviewer |
| **配置语义混用** | `gateway.agents` 同时承载顶级 Agent 和子 Agent | 设计文档明确 `developer` 首版主要供编排器内部使用 |
| **分类误判** | 非开发任务被分到 `developer`，或反之 | 首版采用保守规则，无法确认时统一回退 `nova` |
| **字段语义分裂** | `agent_id` 与 `subagent_type` 分别承担“实例标识”和“执行 Agent 选择”，命名不够直观 | 首版先保持兼容，后续再统一协议命名 |
| **提示词漂移** | `developer` prompt 过长或与 `nova` 重复太多 | 明确 `developer` 只补充开发任务必要约束，避免复制整份通用提示词 |
