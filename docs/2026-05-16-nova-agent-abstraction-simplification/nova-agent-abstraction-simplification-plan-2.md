# Plan 2: 收缩 AgentTool 与 built-in wiring 扩展点

## 前置依赖

Plan 1

## 任务目标

把 `AgentTool` 从“通用可插拔框架”收缩为“围绕当前主路径工作的明确服务对象”。完成后：

- 删除 `AgentPromptLoader`、`SubagentRuntimeFactory` 两个宽泛 trait。
- 删除 `UnconfiguredAgentPromptLoader`、`UnconfiguredSubagentRuntimeFactory` 默认失败实现。
- `AgentTool` 构造路径收敛到单一主构造函数。
- `tool/builtin/mod.rs` 不再通过 `BuiltinToolWiring` 暴露 trait object 细节。

## 执行范围

| 类别 | 路径 | 说明 |
| --- | --- | --- |
| 必须修改 | `crates/nova-agent/src/tool/builtin/agent.rs` | 收缩构造路径与依赖表达 |
| 必须修改 | `crates/nova-agent/src/tool/builtin/mod.rs` | 更新 built-in tools 注册装配 |
| 允许修改 | `crates/nova-agent/src/tool/builtin/orchestrate_task.rs` | 若需要适配共享 `AgentTool` 实例 |
| 允许修改 | `crates/nova-agent/tests/**` | 更新测试构造方式 |
| 禁止修改 | `crates/nova-agent/src/orchestrator/**` | 不改编排协议 |
| 禁止修改 | `crates/nova-agent/src/prompt/**` | 不改 prompt 语义 |

## Agent 执行步骤

1. 在 `tool/builtin/agent.rs` 中删除 `AgentPromptLoader` trait。
2. 在 `tool/builtin/agent.rs` 中删除 `SubagentRuntimeFactory` trait。
3. 删除 `UnconfiguredAgentPromptLoader` 和 `UnconfiguredSubagentRuntimeFactory`。
4. 新增一个具体依赖聚合结构，集中表达 `AgentTool` 运行所需能力；该结构必须只包含当前真实需要的依赖，禁止继续暴露“未来可能支持”的宽泛扩展点。
5. 将 `AgentTool::new`、`new_with_prompt_loader`、`new_with_prompt_loader_and_factory` 收敛为单一主构造函数；如需便于测试，可保留一个仅供测试或内部装配使用的辅助构造函数，但必须明确语义，禁止继续使用 `with_*_and_*` 级联命名。
6. 将“能力未配置”的情况改为：
   - 在构造阶段保证必需依赖齐备；或
   - 在字段类型上显式表达为 `Option<...>`，并在调用前直接判断
   禁止通过默认失败实现延迟到运行时深层报错。
7. 修改 `tool/builtin/mod.rs`：
   - 删除 `BuiltinToolWiring` 中对 trait object 的直接暴露。
   - 改为传入 `Option<AgentToolServices>` 或语义等价的具体装配参数。
8. 保留 `OrchestrateTaskTool` 复用同一个 `AgentTool` 实例的机制，但不要借此保留旧注入框架。
9. 调整 `AgentTool` 相关测试，验证未知 agent fallback、背景运行和能力缺失报错。

## 目标数据结构 / 接口契约

示意契约如下：

```rust
#[derive(Clone)]
pub struct AgentToolServices {
    pub prompt_service: Arc<SubagentPromptService>,
    pub runtime_builder: Arc<SubagentRuntimeBuilder>,
}

#[derive(Clone)]
pub struct AgentTool {
    config_store: Arc<AppConfigStore>,
    agent_types: HashMap<String, AgentSpec>,
    primary_agent_type: String,
    services: AgentToolServices,
}
```

若 `runtime_builder` 或 `prompt_service` 本身仍需要少量内部抽象，必须保持为 crate 内具体类型，禁止再提升为对外 trait 边界。

## 行为规则

| 输入 / 场景 | 处理路径 | 期望结果 |
| --- | --- | --- |
| 正常创建 `AgentTool` | 提供完整具体依赖 | 构造成功 |
| 缺失必需子代理依赖 | 构造阶段或调用前显式检查 | 返回明确错误，不经过 `Unconfigured*` |
| 前台运行子代理 | 走当前主路径 | 行为与现状保持一致 |
| 后台运行子代理 | 复用当前事件转发逻辑 | 行为与现状保持一致 |
| 请求未知 `agent_selection` | 保持 fallback | 返回主 agent，并保留 warning |
| `OrchestrateTask` 需要共享 Agent tool | 复用同一个具体 `AgentTool` 实例 | 行为保持一致 |

## 禁止事项

- 不要把 `AgentTool` 改造成新的通用 service locator。
- 不要在本 Plan 中修改子代理事件格式。
- 不要删除未知 agent fallback 逻辑。
- 不要新增依赖。
- 不要把 `AgentTool` 和 `OrchestrateTaskTool` 合并。

## 测试要求

| 测试文件 | 测试名称 | 输入 | 期望断言 |
| --- | --- | --- | --- |
| `crates/nova-agent/src/tool/builtin/agent.rs` | `resolve_agent_spec_uses_requested_registered_agent` | `developer` | 继续通过 |
| `crates/nova-agent/src/tool/builtin/agent.rs` | `resolve_agent_spec_falls_back_to_default_for_unknown_agent` | 未知 agent | 继续通过 |
| `crates/nova-agent/src/tool/builtin/agent.rs` | `agent_tool_returns_clear_error_when_required_services_are_missing` | 缺失必需依赖 | 返回明确错误 |
| `crates/nova-agent/src/tool/builtin/mod.rs` | 现有 `orchestrate_task_is_visible_when_whitelisted` | 显式白名单 | 继续通过 |

必须执行的验证命令：

```powershell
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test --workspace
```

## 完成条件

- [ ] `AgentPromptLoader` 已删除
- [ ] `SubagentRuntimeFactory` 已删除
- [ ] `UnconfiguredAgentPromptLoader` 已删除
- [ ] `UnconfiguredSubagentRuntimeFactory` 已删除
- [ ] `AgentTool` 构造路径已收敛
- [ ] `BuiltinToolWiring` 不再暴露宽泛 trait object 依赖
- [ ] `OrchestrateTaskTool` 仍可复用共享 `AgentTool`
- [ ] 测试覆盖能力缺失路径
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过
