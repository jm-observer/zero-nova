# Plan 1: 协议契约收敛

- **前置依赖**：无
- **状态**：待开始

---

## 本次目标

统一多 Agent 编排相关的协议模型、事件字段和前端消费约定，消除“后端能发、前端不认”和“文档能写、解析器不收”的契约漂移。

**可验证标准：**
- `crates/nova-protocol` 中存在明确的编排 payload 类型
- 编排事件字段统一为 camelCase
- `deskapp` 对 `orchestration_*` 的消费字段与 Rust 协议一致
- schema 重新生成后无手工 patch 需求

---

## 涉及文件

| 文件 | 操作 | 说明 |
|---|---|---|
| `crates/nova-protocol/src/orchestration.rs` | 修改 | 收敛编排事件结构，必要时补全缺失 payload |
| `crates/nova-protocol/src/chat.rs` | 修改 | 明确 `ProgressEvent.args` 对编排事件的契约说明 |
| `crates/nova-protocol/src/lib.rs` | 修改 | 导出新增或调整后的协议类型 |
| `deskapp/src/core/types.ts` | 修改 | 前端事件类型与字段说明对齐 Rust 协议 |
| `deskapp/src/generated/schema-types.ts` | 生成 | 重新生成 schema 类型 |
| `deskapp/src/ui/orchestration-view.ts` | 修改 | 统一读取 camelCase 字段 |

---

## 详细设计

### 1. 定义最小完备的编排 payload 集

当前 `orchestration.rs` 只覆盖了：

- `OrchestrationPlanEvent`
- `SubAgentCompleteArgs`
- `StageCompleteArgs`
- `OrchestrationCompleteArgs`

但实际链路还需要至少补齐：

- `SubAgentSpawnArgs`
- `SubAgentLogArgs`
- `OrchestrationReviewStartArgs`

建议将编排事件相关 args 都收敛到 `crates/nova-protocol/src/orchestration.rs`，避免继续在注释里描述“匿名 JSON 对象”。

### 2. 统一字段命名为 camelCase

所有结构体字段保持 Rust 侧 snake_case 命名，但通过：

```rust
#[serde(rename_all = "camelCase")]
```

对外统一为 camelCase。前端和 Skill 文档均以序列化后的字段名为准。

### 3. 统一前端读取逻辑

`deskapp/src/ui/orchestration-view.ts` 当前读取：

- `plan_id`
- `stage_id`
- `agent_id`
- `output_summary`

需全部替换为：

- `planId`
- `stageId`
- `agentId`
- `outputSummary`

同时审查 `chat-service.ts` 中转发时是否仍携带错误字段。

### 4. 约束新增消费点

后续任何编排事件消费代码都不得再从 `event.log` 中解析 JSON。结构化数据只能来自：

- `ProgressEvent.args`
- `ProgressEvent.log`（仅纯文本日志）
- `ProgressEvent.output`（仅文本结果）

---

## 测试案例

### T1-01：协议 roundtrip
- 输入：各类编排 args 结构体实例
- 预期：`serde_json::to_value` / `from_value` 均成功，JSON 字段为 camelCase

### T1-02：前端字段消费一致性
- 输入：`orchestration_plan` 事件，`args = { planId, stages: [...] }`
- 预期：`OrchestrationView` 能创建 PlanState，不再因字段缺失直接 return

### T1-03：schema 再生成
- 输入：执行 schema 生成流程
- 预期：`schema-types.ts` 中 `chat.progress` 对应 payload 包含更新后的 `args`

### T1-04：旧日志链路剔除保护
- 输入：`system_log` 中包含伪造编排 JSON
- 预期：编排 UI 不依赖该日志触发

