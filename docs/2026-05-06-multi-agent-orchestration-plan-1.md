# Plan 1: 协议与数据模型

- **前置依赖**：无
- **状态**：待实施

---

## 本次目标

定义多 Agent 编排系统所需的全部协议消息和数据结构，为 Plan 2（核心逻辑）和 Plan 4（前端）提供类型契约。

**可验证标准：**
- `nova-protocol` crate 新增编排相关消息类型，通过 `cargo check`
- `AgentTool` 的 `input_schema` 新增 `agent_id` 字段（子 Agent 标识）
- 协议 TypeScript 类型（`generated/`）可由现有生成脚本输出

---

## 涉及文件

| 文件 | 操作 | 说明 |
|---|---|---|
| `crates/nova-protocol/src/orchestration.rs` | **新增** | 编排相关协议消息类型 |
| `crates/nova-protocol/src/lib.rs` | **修改** | pub mod orchestration |
| `crates/nova-protocol/src/chat.rs` | **修改** | `ProgressEvent.kind` 新增编排 kind 值说明 |
| `crates/nova-agent/src/tool/builtin/agent.rs` | **修改** | `input_schema` 新增 `agent_id`、`parent_agent_id` 字段 |
| `crates/nova-agent/src/tool/builtin/task.rs` | **修改** | `Task` 新增 `orchestration_stage_id` 元数据 key 规范 |

---

## 详细设计

### 1. 新增协议消息：`orchestration.rs`

```rust
use serde::{Deserialize, Serialize};

/// 编排计划发布事件（Orchestrator 完成拆分后广播）
/// kind = "orchestration_plan"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationPlanEvent {
    pub session_id: String,
    pub plan_id: String,
    pub description: String,
    pub stages: Vec<StageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageSummary {
    pub stage_id: String,
    pub mode: String,               // "parallel" | "serial"
    pub depends_on: Vec<String>,
    pub agents: Vec<AgentSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSummary {
    pub agent_id: String,           // Plan 内唯一，如 "agent-1"
    pub description: String,
    pub subagent_type: String,
}

/// 子 Agent 生命周期事件
/// 复用 ProgressEvent，通过 kind 区分：
///
///   kind = "sub_agent_spawn"
///     → agent_id, stage_id, description
///
///   kind = "sub_agent_log"
///     → agent_id, stage_id, log (子 Agent 的流式输出)
///
///   kind = "sub_agent_complete"
///     → agent_id, stage_id, status ("success"|"failed"|"cancelled"), output_summary
///
///   kind = "stage_complete"
///     → stage_id, mode ("parallel"|"serial"), all_success: bool
///
///   kind = "orchestration_review_start"
///     → plan_id
///
///   kind = "orchestration_complete"
///     → plan_id, overall_success: bool, summary
```

**设计决策**：复用现有 `ProgressEvent` 结构，通过 `kind` 扩展，**不新增顶层消息类型**。
- 优点：前端无需新增事件路由分支，已有流式渲染管道可直接复用
- `ProgressEvent` 已有 `args` 字段（`Option<Value>`），用于携带编排专属数据

扩展后 `ProgressEvent` 语义：

```rust
// 现有字段已足够，通过 args 携带编排数据：
// kind="sub_agent_spawn":  args = { agent_id, stage_id, description, subagent_type }
// kind="sub_agent_log":    args = { agent_id, stage_id }; log = <流式文本>
// kind="sub_agent_complete": args = { agent_id, stage_id, status, output_summary }
// kind="stage_complete":   args = { stage_id, mode, all_success }
// kind="orchestration_plan": args = OrchestrationPlanEvent
// kind="orchestration_complete": args = { plan_id, overall_success, summary }
```

### 2. `AgentTool` input_schema 扩展

```json
{
  "properties": {
    "prompt":           { "type": "string" },
    "description":      { "type": "string" },
    "subagent_type":    { "type": "string" },
    "run_in_background":{ "type": "boolean", "default": false },
    "isolation":        { "type": "string", "enum": ["none", "worktree"] },
    "model":            { "type": "string" },

    // 新增：编排专用字段
    "agent_id":         {
      "type": "string",
      "description": "在当前编排 Plan 内的唯一标识符（如 'agent-1'）。编排模式下必填。"
    },
    "parent_plan_id":   {
      "type": "string",
      "description": "所属编排 Plan 的 ID。编排模式下必填。"
    },
    "stage_id":         {
      "type": "string",
      "description": "所属 Stage 的 ID。编排模式下必填。"
    },
    "output_format": {
      "type": "string",
      "enum": ["full", "summary"],
      "default": "full",
      "description": "summary 模式下子 Agent 只返回结构化摘要，节省 Review Agent 上下文"
    }
  },
  "required": ["prompt", "description"]
}
```

### 3. `TaskStore` 语义扩展（元数据约定）

不修改 `Task` 结构体，通过 `metadata` HashMap 约定以下 key：

```
"orchestration_plan_id"   → String  所属编排 Plan ID
"orchestration_stage_id"  → String  所属 Stage ID
"orchestration_agent_id"  → String  子 Agent 标识
"orchestration_role"      → "orchestrator" | "sub_agent" | "reviewer"
```

`TaskCreate` 工具在编排模式下由 Orchestrator 统一创建任务，子 Agent 通过 `blocked_by` 表达依赖。

### 4. 事件流示意（串行 + 并行混合场景）

```
session_id: "sess-abc"

# Orchestrator 拆分完成
→ { kind: "orchestration_plan", args: { plan_id: "plan-1", stages: [...] } }

# Stage 1（并行）- 两个 Agent 同时启动
→ { kind: "sub_agent_spawn",    args: { agent_id: "a1", stage_id: "s1", ... } }
→ { kind: "sub_agent_spawn",    args: { agent_id: "a2", stage_id: "s1", ... } }

# 并行流式日志（交错到达）
→ { kind: "sub_agent_log",      args: { agent_id: "a1", stage_id: "s1" }, log: "reading file..." }
→ { kind: "sub_agent_log",      args: { agent_id: "a2", stage_id: "s1" }, log: "analyzing..." }

# 并行完成
→ { kind: "sub_agent_complete", args: { agent_id: "a1", status: "success", output_summary: "..." } }
→ { kind: "sub_agent_complete", args: { agent_id: "a2", status: "success", output_summary: "..." } }
→ { kind: "stage_complete",     args: { stage_id: "s1", mode: "parallel", all_success: true } }

# Stage 2（串行）
→ { kind: "sub_agent_spawn",    args: { agent_id: "a3", stage_id: "s2", ... } }
→ { kind: "sub_agent_log",      args: { agent_id: "a3", ... }, log: "..." }
→ { kind: "sub_agent_complete", args: { agent_id: "a3", status: "success", ... } }
→ { kind: "stage_complete",     args: { stage_id: "s2", mode: "serial", all_success: true } }

# Review
→ { kind: "orchestration_review_start", args: { plan_id: "plan-1" } }
→ { kind: "token", token: "所有子任务已完成..." }  # Review Agent 输出
→ { kind: "orchestration_complete", args: { plan_id: "plan-1", overall_success: true } }
```

---

## 测试案例

### T1-01：Schema 合规性
- **输入**：`AgentTool` 收到包含 `agent_id`、`parent_plan_id`、`stage_id` 的调用
- **预期**：字段正确解析，无 panic，不影响现有不含这些字段的调用

### T1-02：ProgressEvent 向后兼容
- **输入**：现有不含 `args` 的 `ProgressEvent`（旧版 kind 如 `tool_start`）
- **预期**：前端渲染不受影响，`args: null` 时不崩溃

### T1-03：编排事件序列化
- **输入**：`OrchestrationPlanEvent` 实例
- **预期**：`serde_json::to_value()` 成功，JSON 包含所有字段，反序列化后值相等

### T1-04：TaskStore metadata 写入
- **输入**：`TaskCreate` 携带 `metadata.orchestration_plan_id = "plan-1"`
- **预期**：`TaskStore::get()` 返回的 `Task.metadata` 中包含该键值

### T1-05：`cargo check` 全量通过
- **预期**：引入新类型后 `cargo check --workspace` 零错误零警告
