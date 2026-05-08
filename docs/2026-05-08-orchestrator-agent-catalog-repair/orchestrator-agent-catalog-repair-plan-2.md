# Plan 2: 运行时选路与兼容迁移

| 章节 | 内容 |
|---|---|
| Plan 编号与标题 | Plan 2: 运行时选路与兼容迁移 |
| 前置依赖 | Plan 1 |
| 本次目标 | 将执行 agent 的选择收回到运行时受控路径，完成新旧字段兼容迁移，并清理硬编码/虚构 agent 值。 |
| 涉及文件 | `crates/nova-agent/src/orchestrator/planner.rs`、`crates/nova-agent/src/orchestrator/mod.rs`、`crates/nova-agent/src/tool/builtin/agent.rs`、`crates/nova-protocol/src/orchestration.rs`、相关 schema / fixture 文件 |

## 详细设计

### 2.1 AgentRequest 引入受控执行字段

在迁移期内建议双字段并存：

```rust
pub struct AgentRequest {
    pub agent_id: String,
    pub executor: Option<String>,
    #[serde(default)]
    pub subagent_type: Option<String>, // deprecated
    pub description: String,
    pub prompt: String,
}
```

解析优先级：

1. `executor` 存在且非空，优先使用。
2. 否则回退 `subagent_type`。
3. 若两者都缺失，填充默认 agent。
4. 若两者同时存在但值不一致，直接报错，避免静默漂移。

Plan 2 完成后，运行时内部统一只保留解析后的 `executor_agent_id`，不再让后续链路感知旧字段差异。

### 2.2 planner 负责早期合法性校验

`parse_and_validate()` 目前只校验 stage/agent id 和拓扑关系，不校验 `subagent_type` 是否有效。修复后应新增：

1. 对新字段 `executor` 做非空校验。
2. 对 executor 是否属于可用 catalog 做校验。
3. 对旧字段迁移输入记录 warning。

由于 `planner.rs` 当前只接收 `plan_json`，没有 catalog，上述校验可通过两种方式实现：

1. 给 `parse_and_validate()` 增加 `available_agents: &[String]` 参数。
2. 保持解析与校验分离，新增 `validate_against_catalog(plan, catalog)`。

建议采用第二种，避免把纯 JSON 解析函数和运行时配置过度耦合。

### 2.3 OrchestratorEngine 统一使用解析后的 executor

当前执行闭包会把：

```json
{
  "subagent_type": agent_req.subagent_type
}
```

直接传给 `AgentTool`。修复后需要改为：

```json
{
  "agent": agent_req.executor_agent_id
}
```

并保证：

1. spawn event / complete event 中暴露的是“最终执行 agent”，而不是原始模型输出字段。
2. review 阶段也走同一套 executor 解析，不再硬编码 `"Reviewer"`。
3. 前端如果仍需要展示兼容字段，可在事件层单独附带 `requestedExecutor` 与 `resolvedExecutor`，但执行链路只使用后者。

### 2.4 AgentTool schema 迁移

`Agent` 工具当前 schema 对外暴露：

```json
"subagent_type": {
  "type": "string",
  "description": "Registered agent id ..."
}
```

迁移方案：

1. 新增 `agent` 字段作为主字段。
2. `subagent_type` 保留一段时间作为 deprecated alias。
3. `resolve_agent_spec()` 改为接收解析后的 `agent_id`，不再承载“自由角色名兜底”的职责。

建议警告策略：

- 通过旧字段传值：`warn!("'subagent_type' is deprecated; use 'agent' instead")`
- 传入未知 agent：新字段直接报错；旧字段保留 fallback，但必须 `warn!`

### 2.5 Review executor 去硬编码

当前 review 直接传 `"subagent_type": "Reviewer"`，这是最容易失真的点。修复后有两个可选方案：

1. 始终使用默认 agent 执行 review。
2. 在 catalog 中允许配置 `review_agent_id`，未配置时回退默认 agent。

建议优先采用方案 2，对配置最小增量：

```toml
[gateway]
review_agent_id = "nova"
```

若暂时不加配置项，则至少统一回退 primary agent，并把该选择显式写进 prompt 和日志，避免伪装成存在一个 Reviewer agent。

## 测试案例

1. 仅提供 `executor` 时，plan 可通过校验并使用对应 agent 执行。
2. 仅提供旧 `subagent_type` 时，plan 仍可执行，但会记录 deprecation warning。
3. 同时提供 `executor` 与 `subagent_type` 且不一致时，解析失败。
4. 新字段传入未知 agent 时，直接返回错误。
5. review 阶段的实际执行 agent 与配置一致，不再出现硬编码 `"Reviewer"` fallback。
