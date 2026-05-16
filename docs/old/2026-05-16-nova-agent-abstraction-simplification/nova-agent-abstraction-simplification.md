# Nova Agent Abstraction Simplification

## 时间

- 创建日期：2026-05-16
- 最后更新：2026-05-16

## 项目现状

`crates/nova-agent` 当前已经完成一轮配置与 prompt loader 外移，但 crate 内仍保留多处“为了注入而注入”的抽象：

- `app` 层使用 `ConfigSnapshot`、`AgentRegistrySnapshot`、`TurnPromptMaterialLoader`、`SessionPromptReloader` 等 trait 包装本来可以直接依赖的服务。
- `tool/builtin/agent.rs` 使用 `AgentPromptLoader`、`SubagentRuntimeFactory` 两层 trait 和 `Unconfigured*` 占位实现来支撑唯一主路径。
- `tool/builtin/bash` 与 `tool/builtin/web_search` 使用运行时 trait object 表达当前编译期已知、集合封闭的后端类型。
- `tool/builtin/mod.rs` 为 Agent tool 再包一层 `BuiltinToolWiring`，把实现细节暴露到 built-in tools 注册边界。

这些抽象带来的直接问题：

- 服务构造函数层层分叉，`new` / `new_with_*` / `new_with_*_and_*` 持续增长。
- 默认实现并不工作，而是在运行时通过 “not configured” 失败，导致类型系统没有表达真实前置条件。
- 代码阅读时必须同时理解“真正业务逻辑”和“假想扩展点”，抬高维护成本。
- 测试替身被迫实现完整 trait，而不是只注入本测试需要的最小行为。

## 整体目标

收缩 `nova-agent` 中缺乏现实收益的抽象层，把当前唯一稳定实现恢复为明确的直接依赖，把真正需要保留的多态边界限制在：

- `Tool`
- `LlmClient`
- `StreamReceiver`
- `SttProvider`
- `TtsProvider`
- 其他已经存在多个长期共存实现、且调用边界天然稳定的 trait

本次设计不追求一次性重写所有模块，而是分阶段收缩最明显的过度抽象，目标状态如下：

```text
app
 ├── ConversationService
 │    ├── 直接依赖 AgentRegistry / Config store / PromptMaterial service
 │    └── 不再持有仅做转发的 snapshot trait
 ├── AgentWorkspaceService
 │    ├── 直接依赖 AgentRegistry / Config store / Prompt reload service
 │    └── 不再通过默认失败的 placeholder trait 兜底
tool::builtin::agent
 ├── 直接依赖具体 Subagent services
 ├── 用显式 Optional capability 表达“支持/不支持”
 └── 不再暴露 Unconfigured* trait 实现
tool::builtin::bash / web_search
 └── 用 enum 表达封闭后端集合，而不是 dyn trait
```

## Plan 拆分

| Plan | 描述 | 依赖关系 | 执行顺序 | 状态 |
| --- | --- | --- | --- | --- |
| Plan 1 | 收缩 `app` 层的 snapshot / loader / reloader 抽象，恢复直接依赖 | 无 | 1 | 已完成 |
| Plan 2 | 收缩 `AgentTool` 与 built-in wiring 的扩展点，移除默认失败的占位实现 | Plan 1 | 2 | 已完成 |
| Plan 3 | 将 `bash` / `web_search` 的封闭后端集合改为 enum 表达 | Plan 1 | 3 | 已完成 |

执行顺序说明：

- Plan 1 先收紧上层服务依赖边界，减少后续 Agent tool 和 built-in wiring 的被动兼容面。
- Plan 2 再处理子代理路径，避免同时修改 `app` 和 `tool` 两侧的装配逻辑造成冲突。
- Plan 3 独立于子代理逻辑，可在 Plan 1 完成后并行实施，但提交顺序仍建议放在 Plan 2 之后，降低 review 面积。

## 需要优化的点

| 优化点 | 当前位置 | 现状问题 | 目标状态 |
| --- | --- | --- | --- |
| `ConfigSnapshot` | `crates/nova-agent/src/app/config_snapshot.rs` | 用 async trait 包装配置读取，默认 `apply` 直接失败 | 替换为具体配置存储类型，或明确的 `ArcSwap` / `RwLock` 持有者 |
| `AgentRegistrySnapshot` | `crates/nova-agent/src/app/agent_registry_snapshot.rs` | 仅有 `current() -> AgentRegistry`，实际只返回 clone | 直接持有 `AgentRegistry` 或显式 registry store |
| `TurnPromptMaterialLoader` | `crates/nova-agent/src/app/conversation_service.rs` | 抽象层只服务单一路径，导致服务初始化额外复杂 | 替换为具体 prompt material service |
| `SessionPromptReloader` | `crates/nova-agent/src/app/agent_workspace_service.rs` | 默认实现始终失败，前置条件未体现在类型层 | 改为显式可选能力，调用前先判断支持性 |
| `AgentPromptLoader` | `crates/nova-agent/src/tool/builtin/agent.rs` | 抽象过宽，只有一条稳定调用路径 | 收缩为具体服务依赖或小范围函数对象 |
| `SubagentRuntimeFactory` | `crates/nova-agent/src/tool/builtin/agent.rs` | 默认实现始终失败，构造器分叉过多 | 用显式 `SubagentServices` 聚合结构替代 |
| `BuiltinToolWiring` | `crates/nova-agent/src/tool/builtin/mod.rs` | 把 Agent tool 细节暴露到 built-in tools 注册层 | 改为传入 `Option<AgentToolServices>` 或具体装配器 |
| `ShellBackend` | `crates/nova-agent/src/tool/builtin/bash/mod.rs` | 当前平台后端集合封闭，但使用 `Arc<dyn ShellBackend>` | 改为 `enum ShellBackend` + `match` |
| `SearchBackend` | `crates/nova-agent/src/tool/builtin/web_search/types.rs` | 后端集合封闭，但使用 `Box<dyn SearchBackend>` | 改为 `enum SearchBackend` + `match` |

## 风险与待定项

- `ConfigSnapshot` 若已被桌面端热更新流程隐式依赖，需要先确认是否存在运行中替换配置的真实场景；若存在，需将“可热更新”落到具体 store 类型，而不是保留通用 trait。
- `AgentTool` 背景执行、skill 注入、模型覆盖和环境继承都在同一实现内，Plan 2 需要避免把“收缩抽象”与“重构业务逻辑”混在一起。
- `bash` / `web_search` 若未来确实计划支持插件式外部后端，需要把扩展点上移到 crate 边界，而不是保留在当前工具内部。
- 本设计预计会影响长期设计资产。实施完成后应新增或更新：
  - `docs/design/system-overview.md`
  - `docs/design/nova-agent-engine-boundaries.md`
  - `docs/adr/2026-05-16-nova-agent-abstraction-simplification.md`

## 非目标

- 不修改 `Tool`、`LlmClient`、`StreamReceiver`、`SttProvider`、`TtsProvider` 的公开抽象边界。
- 不顺手拆分 `ConversationService`、`AgentWorkspaceService`、`AgentTool` 的大函数，除非是完成本次依赖收缩所必需的最小变更。
- 不调整 prompt 内容、skill 注入语义、子代理协议字段或前端观测事件格式。
- 不新增依赖。
- 不将本次任务扩展为通用依赖注入框架改造。

## 验收标准

- `app` 层不再保留只做转发或 clone 的 snapshot trait。
- `app` 层不再使用默认失败的 placeholder reloader / loader 实现。
- `AgentTool` 的构造路径收敛为一条主构造路径，禁止再保留 `Unconfigured*` 默认实现。
- `BuiltinToolWiring` 不再暴露宽泛 trait object 依赖。
- `BashTool` 和 `WebSearchTool` 的封闭后端集合改为 enum 表达。
- 新增或调整的测试覆盖正常路径、能力缺失路径和错误路径。
- `cargo clippy --workspace -- -D warnings` 通过。
- `cargo fmt --check --all` 通过。
- `cargo test --workspace` 通过。
