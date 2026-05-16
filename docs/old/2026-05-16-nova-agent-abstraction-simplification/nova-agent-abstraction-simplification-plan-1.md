# Plan 1: 收缩 app 层 snapshot / loader / reloader 抽象

## 前置依赖

无

## 任务目标

将 `ConversationService` 和 `AgentWorkspaceService` 从“依赖多个仅做转发的 trait object”调整为“依赖明确的具体服务或具体存储类型”。完成后：

- `ConfigSnapshot` 和 `AgentRegistrySnapshot` 从 `app` 公共边界中移除。
- `TurnPromptMaterialLoader` 与 `SessionPromptReloader` 不再以默认失败的 placeholder 形式存在。
- 服务是否支持“reload prompt”由显式能力字段表达，而不是运行时调用到默认失败实现。

## 执行范围

| 类别 | 路径 | 说明 |
| --- | --- | --- |
| 必须修改 | `crates/nova-agent/src/app/config_snapshot.rs` | 删除或替换 trait |
| 必须修改 | `crates/nova-agent/src/app/agent_registry_snapshot.rs` | 删除或替换 trait |
| 必须修改 | `crates/nova-agent/src/app/conversation_service.rs` | 收缩 `TurnPromptMaterialLoader` 依赖 |
| 必须修改 | `crates/nova-agent/src/app/agent_workspace_service.rs` | 收缩 `SessionPromptReloader` 依赖 |
| 必须修改 | `crates/nova-agent/src/app/mod.rs` | 更新导出边界 |
| 允许修改 | `crates/nova-agent/src/app/application.rs` | 适配新的服务构造方式 |
| 允许修改 | `crates/nova-agent/tests/integration/*` | 调整测试注入方式 |
| 禁止修改 | `crates/nova-agent/src/prompt/**` | 不做 prompt 语义改造 |
| 禁止修改 | `crates/nova-agent/src/provider/**` | 不改 provider 能力边界 |

## Agent 执行步骤

1. 删除 `AgentRegistrySnapshot` trait，并在 `ConversationService`、`AgentWorkspaceService` 中改为直接持有 `AgentRegistry` 或一个明确命名的具体 registry store。
2. 删除 `ConfigSnapshot` trait，并把配置读取替换为具体配置存储类型。若需要共享可变配置，必须引入项目内已有的具体 store；禁止重新定义新的通用 snapshot trait。
3. 在 `ConversationService` 中删除 `TurnPromptMaterialLoader` trait 定义，改为依赖一个具体 prompt material service。
4. 在 `ConversationService::new` 中保留单一主构造路径。禁止继续保留 `new_with_registry_snapshot` 这类仅服务注入抽象的构造函数。
5. 在 `AgentWorkspaceService` 中删除 `SessionPromptReloader` trait 和 `StaticSessionPromptReloader`。
6. 将 session prompt reload 能力改为显式可选能力：
   - 服务字段必须表达为 `Option<ConcretePromptReloadService>` 或语义等价的明确结构。
   - 调用 `reload_session_system_prompt` 时，若未配置该能力，必须返回明确错误信息，且不得通过占位实现间接报错。
7. 更新 `app/mod.rs` 的 re-export，禁止继续对外导出已删除的 snapshot trait。
8. 调整单元测试和集成测试中的测试替身，改为构造具体数据或轻量测试服务；禁止为了兼容旧测试再引入新的空转发 trait。

## 目标数据结构 / 接口契约

以下为目标方向，命名可微调，但语义必须一致：

```rust
pub struct ConversationService<C: LlmClient> {
    pub agent: AgentRuntime<C>,
    pub agent_registry: AgentRegistry,
    pub sessions: SessionService,
    pub config_store: Arc<AppConfigStore>,
    turn_prompt_service: Arc<TurnPromptService>,
}

pub struct AgentWorkspaceService {
    pub agent_registry: AgentRegistry,
    pub sessions: SessionService,
    pub config_store: Arc<AppConfigStore>,
    pub skill_registry: Arc<SkillRegistry>,
    prompt_reload_service: Option<Arc<SessionPromptReloadService>>,
}
```

若项目内已有等价具体类型，可以直接复用；禁止为了满足这里的命名再次创建新的泛化抽象。

## 行为规则

| 输入 / 场景 | 处理路径 | 期望结果 |
| --- | --- | --- |
| 创建 `ConversationService` | 使用单一路径构造具体依赖 | 成功创建服务，不再经过 snapshot adapter |
| 读取 agent registry | 直接从具体 registry / registry store 读取 | 不再 clone 一层 snapshot wrapper |
| 读取 config | 直接从具体 config store 读取 | 不再通过 async snapshot trait |
| 调用 `reload_session_system_prompt` 且 reload 能力已配置 | 调用具体 reload service | 正常返回 reload 结果 |
| 调用 `reload_session_system_prompt` 且 reload 能力未配置 | 显式检测 `None` | 返回明确的 “session prompt reload is not configured” 类错误 |
| 测试中需要定制配置 | 构造具体测试 config store | 不要求实现额外 trait |

## 禁止事项

- 不要新增新的 `*Snapshot` trait 替换旧 trait。
- 不要为了兼容旧构造器继续保留 `new_with_*snapshot`、`new_with_*loader` 之类的多分支构造函数。
- 不要在本 Plan 中修改 prompt 文本拼装规则。
- 不要顺手重构 `ConversationService::execute_agent_turn` 的业务流程。
- 不要新增依赖。

## 测试要求

| 测试文件 | 测试名称 | 输入 | 期望断言 |
| --- | --- | --- | --- |
| `crates/nova-agent/src/app/agent_workspace_service.rs` | `reload_session_system_prompt_returns_error_when_reload_service_is_missing` | 未配置 reload service | 返回明确错误 |
| `crates/nova-agent/src/app/agent_workspace_service.rs` | `inspect_agent_returns_real_provider_id` | 现有测试场景 | 继续通过，且不再依赖 `ConfigSnapshot` |
| `crates/nova-agent/tests/integration/session_project_runtime.rs` | 相关现有测试 | 现有构造路径 | 通过，且不需要实现旧 trait |
| `crates/nova-agent/tests/integration/session_project_lineage.rs` | 相关现有测试 | 现有构造路径 | 通过，且不需要实现旧 trait |

必须执行的验证命令：

```powershell
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test --workspace
```

## 完成条件

- [ ] `ConfigSnapshot` 已删除或不再出现在 `app` 对外边界
- [ ] `AgentRegistrySnapshot` 已删除或不再出现在 `app` 对外边界
- [ ] `TurnPromptMaterialLoader` 已替换为具体服务依赖
- [ ] `SessionPromptReloader` 与 `StaticSessionPromptReloader` 已删除
- [ ] `ConversationService` 构造路径已收敛
- [ ] `AgentWorkspaceService` 用显式可选能力代替默认失败占位实现
- [ ] 相关测试已更新并通过
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过
