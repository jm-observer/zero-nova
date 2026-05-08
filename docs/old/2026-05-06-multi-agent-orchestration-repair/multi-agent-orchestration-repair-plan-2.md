# Plan 2: 工具入口与 Skill 对齐

- **前置依赖**：Plan 1
- **状态**：待开始

---

## 本次目标

让 `orchestrator` Skill 真正可执行：Skill 引用的 `OrchestrateTask` 必须存在、可注册、可调用，并且其输入契约与 `planner.rs` 和文档示例一致。

**可验证标准：**
- builtin tool 注册表中可见 `OrchestrateTask`
- Skill 激活后工具白名单包含真实存在的工具
- `OrchestrateTask` 能接收 `planJson` 并调用 `OrchestratorEngine`
- Skill 文档示例 JSON 可被解析器直接接受

---

## 涉及文件

| 文件 | 操作 | 说明 |
|---|---|---|
| `crates/nova-agent/src/tool/builtin/mod.rs` | 修改 | 注册 `OrchestrateTask` |
| `crates/nova-agent/src/tool/builtin/orchestrate_task.rs` | 新增 | 编排执行工具 |
| `crates/nova-agent/src/orchestrator/mod.rs` | 修改 | 暴露可供工具调用的执行入口 |
| `crates/nova-agent/src/lib.rs` | 修改 | 导出新增模块 |
| `.nova/skills/orchestrator/SKILL.md` | 修改 | 文档字段改为 camelCase，并与真实工具名保持一致 |
| `.nova/prompts/agent-nova.md` | 复核 | 只保留触发条件，不重复写旧契约 |

---

## 详细设计

### 1. 实现真实 `OrchestrateTask` 工具

建议新增独立 builtin tool，而不是让 Skill 通过隐藏约定直接调用 `OrchestratorEngine`。

建议输入 schema：

```json
{
  "type": "object",
  "properties": {
    "planJson": {
      "type": "string",
      "description": "符合 OrchestrationPlan 协议的 JSON 字符串"
    }
  },
  "required": ["planJson"]
}
```

工具内部职责：

1. 从 `ToolContext` 获取事件发送器、任务存储、技能注册表等依赖
2. 创建 `OrchestratorEngine`
3. 调用 `execute_plan`
4. 返回结构化执行摘要

### 2. Skill 文档与实现同步

`.nova/skills/orchestrator/SKILL.md` 中所有 JSON 示例改为 camelCase：

- `planId`
- `stageId`
- `dependsOn`
- `agentId`
- `subagentType`
- `contextFiles`

同时删除“系统会自动降级”这类当前还未实现的承诺，或明确标注为后续能力，避免继续文档超前。

### 3. 工具可见性约束

`OrchestrateTask` 默认不应对普通会话暴露；仅在：

- 显式白名单
- 或 `orchestrator` Skill 激活

时可见。这样能避免模型在非编排场景误用该工具。

---

## 测试案例

### T2-01：工具注册可见性
- 输入：普通工具列表、Skill 激活后的工具列表
- 预期：普通列表不可见 `OrchestrateTask`，Skill 激活后可见

### T2-02：Skill 示例可解析
- 输入：使用更新后的 `SKILL.md` 示例 JSON
- 预期：`planner::parse_and_validate()` 成功

### T2-03：工具入口联通
- 输入：调用 `OrchestrateTask { planJson: ... }`
- 预期：成功进入 `OrchestratorEngine::execute_plan`

### T2-04：缺少上下文时安全失败
- 输入：无 `ToolContext` 场景调用 `OrchestrateTask`
- 预期：返回带上下文的错误，不 panic

