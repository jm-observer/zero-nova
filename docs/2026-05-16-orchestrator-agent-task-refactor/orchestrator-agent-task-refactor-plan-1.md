# Plan 1: 收缩 AgentTool 为内部执行器

## 前置依赖

无

## 任务目标

将当前 `AgentTool` 从模型可见工具改造为 orchestrator 专用的内部执行器，使其不再暴露编排内部字段，也不再承担工具协议解析职责。

## 执行范围

- 必须修改：
  - `crates/nova-agent/src/tool/builtin/agent.rs`
  - `crates/nova-agent/src/tool/builtin/mod.rs`
  - `crates/nova-agent/src/tool/builtin/orchestrate_task.rs`
  - `crates/nova-agent/src/orchestrator/mod.rs`
- 允许修改：
  - `crates/nova-agent/src/orchestrator/planner.rs`
  - 与 `SubAgentExecutor` 相关的测试文件
- 禁止修改：
  - 不要修改非编排相关工具的公开行为
  - 不要在本 Plan 引入新的持久化机制
  - 不要新增依赖

## Agent 执行步骤

1. 在 `crates/nova-agent/src/tool/builtin/agent.rs` 中移除 `impl Tool for AgentTool`
2. 在 `crates/nova-agent/src/tool/builtin/agent.rs` 中保留并收缩 `AgentTool` 的内部执行职责，将其重命名或包裹为更明确的内部执行器类型；若保留原名，必须在注释中说明其仅供内部编排使用
3. 在 `crates/nova-agent/src/tool/builtin/agent.rs` 中新增结构化执行请求类型，禁止继续通过松散 `serde_json::Value` 直接表达内部执行参数
4. 在内部执行请求中只保留最小必要字段：`agent_id`、`stage_id`、`prompt`、`agent_selection` 或等价字段、可选 `model_override`、可选 `skill_id`
5. 删除内部执行请求中的 `description`、`run_in_background`、`parent_plan_id`、`output_format`、`subagent_type` / `agent_selection` 并存等冗余参数
6. 在 `crates/nova-agent/src/tool/builtin/mod.rs` 中停止将 `AgentTool` 注册到模型可见工具列表
7. 在 `crates/nova-agent/src/orchestrator/mod.rs` 与 `crates/nova-agent/src/tool/builtin/orchestrate_task.rs` 中改用新的内部执行接口
8. 保留 `SubAgentExecutor` 抽象，但其输入必须改为结构化请求，而不是工具输入 JSON

## 目标数据结构 / 接口契约

```rust
pub(crate) struct SubAgentExecutionRequest {
    pub agent_id: String,
    pub stage_id: String,
    pub prompt: String,
    pub agent_selection: Option<String>,
    pub model_override: Option<String>,
    pub skill_id: Option<String>,
}

pub(crate) struct SubAgentExecutionResult {
    pub output: String,
    pub duration_ms: u128,
    pub warnings: Vec<String>,
}

#[async_trait]
pub trait SubAgentExecutor: Send + Sync {
    async fn execute_agent(
        &self,
        request: SubAgentExecutionRequest,
        context: Option<ToolContext>,
    ) -> Result<SubAgentExecutionResult>;
}
```

## 行为规则

| 输入 | 处理路径 | 期望输出或状态变化 |
|------|----------|------------------|
| orchestrator 请求执行一个子 agent | 调用内部 `SubAgentExecutor` | 返回结构化 `SubAgentExecutionResult` |
| 模型尝试直接调用 `Agent` 工具 | 工具列表中不存在该工具 | 模型层无法直接创建子 agent |
| 未指定 `agent_selection` | 执行器使用默认 agent 配置 | 正常执行，且无 JSON 回退逻辑 |
| 指定未知 agent 类型 | 执行器走既有默认回退策略 | 返回 warning，但执行链路保持内部结构化 |

## 禁止事项

- 不要保留 `AgentTool` 的模型工具注册
- 不要继续让 orchestrator 通过 `json!({...})` 调用内部 agent 执行
- 不要在本 Plan 中修改 `TaskStore` 结构
- 不要实现新的前端展示协议

## 测试要求

- 修改或新增 `crates/nova-agent/src/tool/builtin/agent.rs` 单元测试：
  - 验证默认 agent 解析仍然正确
  - 验证未知 agent 仍然有 warning 回退
  - 验证内部执行请求构造不再依赖 `serde_json::Value`
- 修改或新增 `crates/nova-agent/src/orchestrator/mod.rs` 测试：
  - 验证 orchestrator 调用执行器时使用结构化请求
- 必须执行验证命令：
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --check --all`
  - `cargo test --workspace`

## 完成条件

- [ ] `AgentTool` 不再实现 `Tool`
- [ ] `AgentTool` 不再出现在模型可见工具注册列表中
- [ ] `SubAgentExecutor` 输入已改为结构化请求
- [ ] orchestrator 不再通过 JSON 拼装内部 agent 调用
- [ ] 单元测试覆盖默认回退与结构化执行路径
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过

