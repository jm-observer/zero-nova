# Plan 3: 抽 `SubagentRuntimeFactory`，AgentTool / OrchestrateTaskTool 解耦 `AppConfig`

## 前置依赖

Plan 1。可与 Plan 2 并行。

## 本次目标

把 `AgentTool::run_subagent` 与 `OrchestrateTaskTool` 中持有的 `AppConfig` 替换成更小的 handle 集合，并把 sub-agent runtime 构造逻辑（与 `bootstrap.rs::build_application` 重复的部分）抽到 `nova-agent-loader` 的统一 factory。

完成后：

- `AgentTool` 字段不再有 `config: AppConfig`。
- `AgentTool::run_subagent` 内不再有 `AgentConfig { trimmer, loop_guard, prompt_diagnostics, tool_result_compaction }` 的硬编码字段拷贝。
- `OrchestrateTaskTool` 持有 `Arc<AgentTool>` 或 `Arc<dyn SubagentSpawner>`，不再持有 `AppConfig`。
- `bootstrap.rs::build_application` 调用同一 factory，DRY 一处。

## 涉及文件

| 文件 | 变更类型 | 说明 |
| --- | --- | --- |
| `crates/nova-agent-loader/src/subagent_factory.rs` | 新增 | `SubagentRuntimeFactory` 与 `SubagentRuntimeRequest` |
| `crates/nova-agent-loader/src/lib.rs` | 修改 | 暴露 factory |
| `crates/nova-agent/src/tool/builtin/agent.rs` | 重构 | 字段与构造改造；调用 factory 而非自建 |
| `crates/nova-agent/src/tool/builtin/orchestrate_task.rs` | 重构 | 不再持有 `AppConfig` |
| `crates/nova-agent/src/tool/builtin/mod.rs` | 修改 | `register_builtin_tools_with_agent_prompt_loader` 接受 factory handle |
| `crates/nova-agent-loader/src/bootstrap.rs` | 修改 | 复用 factory，替换 inline `AgentConfig` 构造 |

## 详细设计

### `SubagentRuntimeFactory` 接口

新文件 `crates/nova-agent-loader/src/subagent_factory.rs`：

```rust
pub struct SubagentRuntimeRequest<'a> {
    pub spec: &'a AgentSpec,
    pub binding: &'a ResolvedAgentBinding,
    pub model_override: Option<&'a str>,
    pub environment: EnvironmentSnapshot,
    pub project_dir: Option<&'a Path>,
    pub tool_context: Option<&'a ToolContext>,
}

pub struct BuiltSubagentRuntime<C: LlmClient> {
    pub runtime: AgentRuntime<C>,
    pub model_config: ModelConfig,
}

#[async_trait]
pub trait SubagentRuntimeFactory: Send + Sync {
    async fn build(&self, request: SubagentRuntimeRequest<'_>) -> Result<BuiltSubagentRuntime<OpenAiCompatClient>>;
}

pub struct DefaultSubagentRuntimeFactory {
    runtime_template: SubagentRuntimeTemplate,
    providers: Arc<ProviderRegistry>,
    http_clients: HttpClients,
    outbound_headers_enabled: bool,
}

pub struct SubagentRuntimeTemplate {
    pub max_iterations: usize,
    pub subagent_timeout: Duration,
    pub max_tokens: usize,
    pub trimmer: TrimmerConfig,
    pub loop_guard: LoopGuardConfig,
    pub prompt_diagnostics: PromptDiagnosticsConfig,
    pub tool_result_compaction: ToolResultCompactionConfig,
    pub config_dir: PathBuf,
    pub prompts_dir: PathBuf,
    pub project_context_file: Option<PathBuf>,
}

impl SubagentRuntimeTemplate {
    pub fn from_config(config: &AppConfig) -> Self { /* one-time projection */ }
}
```

`DefaultSubagentRuntimeFactory::build` 内部负责：

- 用 `binding` + `model_override` + `spec.model_config` 组装 `ModelConfig`。
- 构造 `OpenAiCompatClient`（用预先共享的 `http_clients.provider`，避免每次 `build_provider_client`）。
- 构造空 `ToolRegistry` 子注册表。
- 用 `runtime_template` 字段+ environment 组装 `AgentConfig`。
- 创建 `AgentRuntime`，注入 `task_store`、`skill_registry`、`read_files`（来自 `tool_context`）。
- 返回 `BuiltSubagentRuntime`。

### `AgentTool` 字段瘦身

`crates/nova-agent/src/tool/builtin/agent.rs::AgentTool`：

```rust
pub struct AgentTool {
    agent_specs: Arc<HashMap<String, AgentSpec>>,
    primary_agent_id: String,
    catalog_hint: String,             // 缓存 build_agent_catalog_hint 输出
    catalog_section: Option<String>,  // 缓存 build_agent_catalog_section
    binding_resolver: Arc<dyn AgentBindingResolver>,  // 提供 resolve_agent_binding / resolve_model_override
    prompt_loader: Arc<dyn AgentPromptLoader>,
    subagent_factory: Arc<dyn SubagentRuntimeFactory>,
}
```

新增 trait `AgentBindingResolver`（位于 `nova-agent::config`，因为 AgentSpec/ResolvedAgentBinding 都是 config crate 类型；可放在 `nova-agent::config` 的转换层，由 `AppConfig` 实现）：

```rust
#[async_trait]
pub trait AgentBindingResolver: Send + Sync {
    fn resolve(&self, agent_id: &str) -> Result<ResolvedAgentBinding>;
    fn resolve_override(
        &self,
        base: &ResolvedAgentBinding,
        provider: &str,
        model: &str,
    ) -> Result<ResolvedAgentBinding>;
    fn outbound_headers_enabled(&self) -> bool;
}

pub struct AppConfigBindingResolver { config: Arc<RwLock<AppConfig>> }
```

> 备注：若引入 trait 改动面过大，可先用具体类型 `Arc<RwLock<AppConfig>>` 代替 trait；trait 等待 Plan 4 落地 `ConfigStore` 再统一。本 Plan 接受任一形式，关键是 AgentTool 不再有 `AppConfig` 值字段。

### `run_subagent` 改造

去掉硬编码字段拷贝，仅做编排：

```rust
async fn run_subagent(&self, prompt, subagent_type, model_override, context) {
    let (spec, warnings) = self.resolve_agent_spec(subagent_type)?;
    let binding = self.binding_resolver.resolve(&spec.id)?;
    let env = ... /* same as today */;
    let project_dir = ... ;

    let BuiltSubagentRuntime { mut runtime, .. } = self.subagent_factory
        .build(SubagentRuntimeRequest {
            spec, binding: &binding, model_override, environment: env.clone(),
            project_dir: project_dir.as_deref(), tool_context: context.as_ref(),
        })
        .await?;

    // prompt loading 与现有一致：self.prompt_loader.load_agent_material / load_turn_material
    // run_turn_with_context_and_model_config 调用一致
}
```

### `OrchestrateTaskTool` 改造

把 `config: AppConfig` 替换为 `agent_tool_factory: Arc<dyn Fn() -> Arc<AgentTool>>` 或更简单的 `agent_tool: Arc<AgentTool>`（共享同一份）。

```rust
pub struct OrchestrateTaskTool {
    agent_tool: Arc<AgentTool>,
}

impl OrchestrateTaskTool {
    pub fn new(agent_tool: Arc<AgentTool>) -> Self { Self { agent_tool } }
}

impl Tool for OrchestrateTaskTool {
    async fn execute(...) {
        let engine = OrchestratorEngine::new(self.agent_tool.clone(), tool_context.event_tx.clone(), Some(tool_context));
        ...
    }
}
```

`OrchestratorEngine::new` 当前签名为 `(Arc<AgentTool>, mpsc::Sender, Option<ToolContext>)`，保持不变即可。

### `register_builtin_tools_with_agent_prompt_loader` 改造

签名增加 `subagent_factory: Arc<dyn SubagentRuntimeFactory>` 参数：

```rust
pub fn register_builtin_tools_with_agent_prompt_loader(
    registry: &ToolRegistry,
    config: &AppConfig,
    task_store: TaskStoreHandle,
    skill_registry: Arc<SkillRegistry>,
    tool_whitelist: Option<&[String]>,
    project_dir_service: Arc<dyn ProjectDirService>,
    http_clients: &HttpClients,
    agent_prompt_loader: Option<Arc<dyn AgentPromptLoader>>,
    subagent_factory: Arc<dyn SubagentRuntimeFactory>,
    binding_resolver: Arc<dyn AgentBindingResolver>,
)
```

`AgentTool::new_with_prompt_loader` 改名 `AgentTool::new(...)` 接收上述 handle，`OrchestrateTaskTool::new(agent_tool)`。

`register_builtin_tools`（不带 prompt loader 的简化版本）调用时给 factory 一个 `UnconfiguredSubagentRuntimeFactory`（永远 `bail!`），用于 CLI / 测试中不需要 Agent tool 的场景。

### `bootstrap.rs` 适配

`build_application` 在 `SkillRegistry` 创建后构造一次 `DefaultSubagentRuntimeFactory`、`AppConfigBindingResolver`，传给 `register_builtin_tools_with_agent_prompt_loader`。

主 runtime 的 `AgentConfig` 构造可以继续使用 `SubagentRuntimeTemplate::from_config(config).into_main_agent_config(env_snapshot, root_binding)`，与 sub-runtime 共享同一组字段映射，**消除两段重复字段拷贝**。

### 错误与日志保持

- `[Agent] Subagent ... resolved provider=... llm=... model=...` 日志保留。
- 模型 override 失败时仍报 `binding.model_config.max_tokens` 等不变。
- warnings 列表语义不变。

## 测试案例

### 编译

- `cargo build --workspace`：通过。
- `cargo clippy -p nova-agent -- -D warnings`：通过。

### 单测

- `crates/nova-agent/src/tool/builtin/agent.rs::tests`：
  - `build_tool()` 改为构造 `AgentTool` 时注入 mock `BindingResolver` 与 mock `SubagentRuntimeFactory`。
  - 保留 `resolve_agent_spec_*` 与 `selected_agent_type_*` 四个测试。
  - 新增：`run_subagent` 使用 mock factory，验证传入的 `SubagentRuntimeRequest` 字段。
- `crates/nova-agent/src/tool/builtin/orchestrate_task.rs::tests`：
  - 用 mock `AgentTool` 构造 `OrchestrateTaskTool`。
  - `orchestrate_task_accepts_minimal_valid_plan` 仍可通过空 plan 验证。

### 字段去除自检

```bash
rg "config: AppConfig" crates/nova-agent/src/tool
```

预期 0 命中。

```bash
rg "AgentConfig \{" crates/nova-agent/src/tool/builtin/agent.rs
```

预期 0 命中（runtime 由 factory 构造）。

### 行为不回归

- `cargo test --workspace`：包含 orchestrator engine 集成测试，全部通过。
- 手工跑一次 stdio gateway + 一次 Agent tool 调用：sub-agent 行为与重构前一致（trimmer、loop guard、超时等参数生效）。

### 异常路径

- 未注册 `SubagentRuntimeFactory` 时（CLI 简化场景）调用 Agent tool：返回 `AgentPromptLoader is not configured` 或新增的 `SubagentRuntimeFactory is not configured` 错误，行为明确。
- `binding_resolver.resolve(unknown_id)` 错误传播到 Agent tool 调用方。
