# Plan 2: 编排分配与运行时回退

- **前置依赖**：Plan 1
- **状态**：已完成（2026-05-07）

---

## 本次目标

1. 定义 Orchestrator 为子任务选择执行 Agent 的规则
2. 定义运行时对缺失或非法执行 Agent 标识的回退逻辑
3. 确保首版只在 `nova` 与 `developer` 之间路由，保持实现面最小

---

## 涉及文件

| 文件 | 操作 | 说明 |
|---|---|---|
| `crates/nova-agent/src/tool/builtin/agent.rs` | 修改 | `subagent_type` 到已注册 Agent 的映射与默认回退 |
| `crates/nova-agent/src/orchestrator/planner.rs` | 修改 | 缺失 `subagent_type` 时补默认值 |
| `.nova/skills/orchestrator/SKILL.md` | 修改 | 要求 Orchestrator 为开发类子任务显式分配 `developer` |

---

## 详细设计

### 1. Orchestrator 分配规则

首版采用保守规则分类，不依赖额外 persona 推理：

当子任务描述或 prompt 中明确出现以下意图时，分配 `subagent_type = "developer"`：

1. `实现`
2. `修改`
3. `修复`
4. `新增测试` / `补测试`
5. `重构` 且明确限制在局部文件范围内

其他情况统一分配 `subagent_type = "nova"`，包括：

1. 需求梳理
2. 结果汇总
3. 一致性检查
4. 不确定是否需要改代码的子任务

规则的核心原则是：宁可保守地回退 `nova`，也不要过度把任务分给 `developer`。

### 2. Plan JSON 结构

编排 JSON 中的单个子任务形态建议为：

```json
{
  "agentId": "a1",
  "subagentType": "developer",
  "description": "实现认证中间件",
  "prompt": "在 src/auth/ 中实现认证中间件，并补充测试",
  "contextFiles": ["src/auth/", "tests/auth/"]
}
```

要求：

1. `agentId` 是 Plan 内唯一实例标识，必须存在
2. `subagentType` 用于选择执行 Agent；首版只允许 `nova` 或 `developer`
3. `subagentType` 缺失时，解析层默认补成 `nova`
4. `subagentType` 非法时，运行时不直接报错中断，而是进入默认回退逻辑

### 3. 运行时回退逻辑

执行子 Agent 前，统一做以下处理：

1. 若 `subagentType` 缺失，解析层直接补成 `nova`
2. 若 `subagentType` 不存在于注册表，记录 warning 并回退为 `nova`
3. 若 `subagentType=developer`，则加载 `agent-developer.md`
4. 若 `subagentType=nova`，则加载 `agent-nova.md`

这里的回退属于运行时容错，而不是模型错误。目的是保证编排系统在本地模型输出不稳定时依旧可用。

### 4. Skill 侧约束

`orchestrator` Skill 需要补充一条明确规则：

1. 开发类子任务优先分配 `developer`
2. 非开发类任务默认分配 `nova`
3. 不要输出未注册的 `subagentType`

同时在示例 JSON 中直接展示 `subagentType` 字段，降低本地模型遗漏概率。

---

## 测试案例

### T2-01：开发类任务分配 `developer`

- **输入**：`在 src/auth.rs 中实现 token 刷新逻辑并补充测试`
- **预期**：生成的子任务 `subagentType=developer`

### T2-02：非开发类任务回退 `nova`

- **输入**：`汇总前两个子任务的结果，并判断是否满足需求`
- **预期**：生成的子任务 `subagentType=nova`

### T2-03：缺失 `subagentType`

- **输入**：模型输出子任务时遗漏 `subagentType`
- **预期**：解析层自动补成 `nova`，任务继续执行

### T2-04：非法 `subagentType`

- **输入**：模型输出 `subagentType="coder-plus"`
- **预期**：记录 warning 并回退 `nova`，而不是让整条编排失败

### T2-05：注册表一致性

- **前提**：配置中只存在 `nova` 和 `developer`
- **预期**：编排器和运行时都不会依赖其他隐藏枚举值
