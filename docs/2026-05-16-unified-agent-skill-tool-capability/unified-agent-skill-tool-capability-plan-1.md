# Plan 1: 统一 Tool 注册模型

## 前置依赖

无

## 任务目标

移除 agent 级工具白名单带来的注册差异，使根 agent runtime 与子 agent runtime 都共享同一套基础工具注册策略。

完成后应满足：

- `tool_whitelist` 不再控制工具注册
- 子 agent runtime 注册的工具集合与根 runtime 在类别上保持一致
- `Skill`、`Agent`、`Task*`、`OrchestrateTask` 的可注册性不再依赖当前 agent 规格差异

## 执行范围

- 必须修改：
  - `crates/nova-agent/src/tool/builtin/mod.rs`
  - `crates/nova-agent/src/tool/builtin/agent.rs`
  - `crates/nova-agent-config/src/models.rs`
  - `crates/nova-agent-config/src/loaders.rs`
  - `crates/nova-agent-loader/src/descriptor_factory.rs`
- 允许修改：
  - 相关测试文件
- 禁止修改：
  - skill 文件内容
  - agent prompt 文案
  - orchestrator 执行逻辑

## Agent 执行步骤

1. 在 `crates/nova-agent-config/src/models.rs` 中删除或废弃 `AgentSpec.tool_whitelist`
2. 在 `crates/nova-agent-config/src/loaders.rs` 中移除 `tool_whitelist` 的配置装载路径
3. 在 `crates/nova-agent-loader/src/descriptor_factory.rs` 中删除 `tool_whitelist` 透传
4. 在 `crates/nova-agent/src/tool/builtin/agent.rs` 中修改 sub runtime 构建逻辑，调用 `register_builtin_tools()` 时不再传入 agent 特定 whitelist
5. 在 `crates/nova-agent/src/tool/builtin/mod.rs` 中收敛 `register_builtin_tools*()` 接口，移除依赖 whitelist 的分支判定
6. 补充测试，验证根 runtime 与子 runtime 都能注册统一工具集合

## 目标数据结构 / 接口契约

目标 `AgentSpec`：

```rust
pub struct AgentSpec {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub aliases: Vec<String>,
    pub provider: String,
    pub llm: String,
    pub prompt_file: Option<String>,
    pub prompt_inline: Option<String>,
    pub system_prompt_template: Option<String>,
    pub model_config: ConfiguredAgentModel,
    pub enable_project_developer_prompt: bool,
}
```

目标工具注册原则：

```rust
register_builtin_tools*(..., /* no agent-specific tool whitelist */)
```

## 行为规则

| 输入 / 场景 | 期望结果 |
|------|----------|
| 根 runtime 启动 | 注册统一工具集合 |
| 子 agent runtime 启动 | 注册与根 runtime 同类别的工具集合 |
| 任意 agent 配置 | 不再通过配置裁剪工具注册范围 |
| `OrchestrateTask` | 是否注册只取决于系统统一注册规则，不取决于 agent 配置 |

## 禁止事项

- 不要在本 Plan 中修改 turn 级工具可见性逻辑
- 不要在本 Plan 中删除 `ToolPolicy`
- 不要顺手改 prompt 注入逻辑
- 不要修改会话 skill 路由逻辑

## 测试要求

- 补充或修改测试，覆盖：
  - 根 runtime 工具注册快照
  - 子 agent runtime 工具注册快照
  - `OrchestrateTask` 在统一模型下的注册可见性
- 必须执行：
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --check --all`
  - `cargo test --workspace`

## 完成条件

- [x] `tool_whitelist` 不再参与工具注册
- [x] 根 runtime 与子 runtime 工具注册路径已统一
- [x] 工具注册测试覆盖主 / 子 runtime
- [x] `cargo clippy --workspace -- -D warnings` 通过
- [x] `cargo fmt --check --all` 通过
- [x] `cargo test --workspace` 通过
