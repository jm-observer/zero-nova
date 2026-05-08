# Plan 3: 测试与观测补齐

| 章节 | 内容 |
|---|---|
| Plan 编号与标题 | Plan 3: 测试与观测补齐 |
| 前置依赖 | Plan 1、Plan 2 |
| 本次目标 | 补齐 prompt、协议、运行时和事件层测试，并新增必要日志/观测字段，让 agent catalog 选择问题能被快速定位。 |
| 涉及文件 | `crates/nova-agent/src/prompt.rs`、`crates/nova-agent/src/orchestrator/planner.rs`、`crates/nova-agent/src/tool/builtin/agent.rs`、`crates/nova-agent/src/orchestrator/mod.rs`、`crates/nova-protocol/src/orchestration.rs`、`schemas/fixtures/*` |

## 详细设计

### 3.1 Prompt 测试

需要补充的测试点：

1. `PromptConfig.available_agents` 有值时，`SystemPromptBuilder::from_config()` 会注入 `Available Agents` section。
2. section 内容包含默认 agent 标记与禁止编造 agent 的规则。
3. 非 orchestrator 场景若不传 `available_agents`，不会引入空 section。

这类测试优先放在 `prompt.rs`，因为它们验证的是纯组装逻辑。

### 3.2 planner / 迁移测试

需要覆盖：

1. 新字段 `executor` 的正常路径。
2. 旧字段 `subagent_type` 的兼容路径。
3. 冲突输入、不存在 agent、空字符串等错误路径。
4. 默认 agent 回填逻辑。

重点不是只验证 JSON 能反序列化，而是要验证“最终解析后的执行 agent”语义正确。

### 3.3 AgentTool 测试

需要补充：

1. `agent` 新字段优先于 `subagent_type`。
2. 旧字段仍能触发回退 warning。
3. 未知新字段值直接失败。
4. 事件里上报的执行 agent 与最终解析结果一致。

必要时可把“输入归一化”提取成小函数，降低测试复杂度。

### 3.4 Event / schema / fixture 更新

`nova-protocol::orchestration` 当前多处 event args 带有 `subagentType`。迁移期建议：

1. 若字段继续保留，语义改成“resolved execution agent id”。
2. 如需保留历史兼容，可以新增 `requestedSubagentType` 或 `rawRequestedAgent` 用于调试，不参与执行。

同时要更新：

1. schema 导出测试
2. contract fixture
3. 前端依赖的 fixture 样例

避免“代码已经改成 executor，fixture 还是旧字段”的半迁移状态。

### 3.5 观测与日志

建议在关键节点统一输出以下信息：

1. plan 解析后：`requested_executor`, `resolved_executor`, `source=new|legacy|default`
2. AgentTool 启动前：`agent_id`, `resolved_agent`, `fallback_used`
3. review 阶段：`review_executor`, `config_source`

示例：

```text
[OrchestratorEngine] resolved executor plan_id=... agent_id=agent-2 requested=Reviewer resolved=nova source=legacy_fallback
```

这样能快速识别：

1. 模型是否仍在生成旧字段
2. 是否在频繁命中 fallback
3. 哪些 prompt 没有正确拿到 catalog

## 测试案例

1. prompt 单测验证 `Available Agents` section 存在且内容正确。
2. planner 单测验证新旧字段解析和冲突错误。
3. AgentTool 单测验证 `agent` 新字段优先级与 fallback/warning 行为。
4. orchestration event 单测验证输出的是最终执行 agent。
5. schema / fixture 测试验证导出结果与新协议一致。
