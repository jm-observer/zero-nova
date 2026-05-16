# Plan 2: `nova-cli` 切换到 `nova-agent-loader` 统一组装路径

## 前置依赖

Plan 1（依赖图已收敛到真正的 `nova-agent-config`）。

## 本次目标

让 `nova-cli` 与 stdio/ws gateway 共用同一个组装路径（`nova_agent_loader::build_application` 或它抽出的轻量 builder），不再自行 `register_builtin_tools + 拼装 SystemPromptBuilder + 手写 skill 转换`。CLI 输出的 system prompt 必须包含 agent prompt、project context、workflow prompt、environment snapshot、agent catalog，与 server 一致。

完成后：

- `crates/nova-cli/src/main.rs` 不再 `use nova_skill_loader::*`。
- CLI 与 Server 加载 skill / 装配 prompt 的代码路径只有一份（loader crate 提供）。
- `convert_loaded_skills` 重复实现被删除。
- CLI 仍保留 `--include-skill <path>` 调试能力。

## 涉及文件

| 文件 | 变更类型 | 说明 |
| --- | --- | --- |
| `crates/nova-agent-loader/src/lib.rs` | 修改 | 暴露轻量 builder（如 `build_repl_runtime`）或新增 `pub fn load_skills_for(config)` |
| `crates/nova-agent-loader/src/bootstrap.rs` | 拆分 | 把 skill 加载、descriptor 构建、runtime 构造抽成可复用 helper |
| `crates/nova-agent-loader/src/skill_adapter.rs` | 扩展 | 新增 `pub fn load_skills(config_skill_dir, extra_skill_paths) -> Vec<SkillPackage>` |
| `crates/nova-cli/Cargo.toml` | 修改 | 新增 `nova-agent-loader = { workspace = true }`；移除 `nova-skill-loader`（若不再直用） |
| `crates/nova-cli/src/main.rs` | 重构 | 删除本地 `convert_*` 函数；改走 loader 提供的 API；system prompt 走 `SystemPromptBuilder::from_material` |

## 详细设计

### 选项 A：CLI 直接复用 `build_application`

`build_application` 当前返回 `Arc<dyn AgentApplication>`，内含 `ConversationService`、`AgentWorkspaceService`、`AgentApplicationImpl`，对 CLI 过重（CLI 不需要 SQLite session、WebSocket app facade）。

不推荐直接走 `build_application`，否则 CLI 会被迫初始化 sqlite、session cache、application facade 等不需要的组件。

### 选项 B（推荐）：抽出 `build_agent_runtime`

在 `nova-agent-loader::bootstrap` 中把当前 `build_application` 的前半段抽成独立函数：

```rust
pub struct BuiltAgentRuntime<C: LlmClient> {
    pub runtime: AgentRuntime<C>,
    pub agent_registry: AgentRegistry,
    pub skill_registry: Arc<SkillRegistry>,
    pub primary_binding: ResolvedAgentBinding,
    pub initial_env: EnvironmentSnapshot,
    pub prompt_loader: PromptMaterialLoader,
    pub task_store: TaskStoreHandle,
}

pub struct AgentRuntimeBuildOptions {
    pub extra_skill_paths: Vec<PathBuf>,
    pub project_dir_service: Arc<dyn ProjectDirService>,
    pub include_orchestrate_task: bool,
}

pub async fn build_agent_runtime(
    config: &AppConfig,
    options: AgentRuntimeBuildOptions,
) -> Result<BuiltAgentRuntime<OpenAiCompatClient>> { /* ... */ }
```

职责：

- 加载 skill（含 `--include-skill` 的额外路径）。
- 构造 `SkillRegistry`、`HttpClients`、`PromptMaterialLoader`、`AgentDescriptorFactory`。
- 创建 `AgentRegistry`、`AgentRuntime`、注册 builtin tools。
- 不创建 SQLite / session cache / application facade。

`build_application` 改为在 `build_agent_runtime` 之上叠加 sqlite/session/application 层。

### CLI 主流程改造

`crates/nova-cli/src/main.rs::main`：

```rust
let workspace = resolve_workspace(&cli.workspace, ".nova")?;
let config_path = workspace.join("config.toml");
let config = AppConfig::load_from_file(&config_path, workspace.clone())?;
let _ = config.selected_agent(cli.agent.as_deref())?;

let built = build_agent_runtime(
    &config,
    AgentRuntimeBuildOptions {
        extra_skill_paths: cli.include_skill.iter().map(PathBuf::from).collect(),
        project_dir_service: Arc::new(UnavailableProjectDirService::new(
            "ProjectManager is unavailable in CLI mode",
        )),
        include_orchestrate_task: false,
    },
).await?;

let primary_agent = built.agent_registry.primary();
let system_prompt = primary_agent.system_prompt_template.clone();
```

CLI 不再自行构造 `AgentConfig`、`SystemPromptBuilder`、`register_builtin_tools`。

### system prompt 生成

`AgentDescriptor::system_prompt_template` 已由 `AgentDescriptorFactory::build_descriptor` 通过 `SystemPromptBuilder::from_material` 生成（含 environment、catalog、initial_template_vars）。CLI 直接拿这个字符串塞到 history 的 system 消息即可。

注意：

- 当前 CLI `with_tools(&tools)` 把工具简介塞入 prompt，本质是 system prompt 内 tool guidance。`SystemPromptBuilder::from_material` 已经按 `ToolGuidanceMode` 处理，CLI 不要重复加。
- 如果 CLI 模式希望强制 `ToolGuidanceMode::Full`，应通过 config（`prompt_compaction.enabled = false`）控制，不在 CLI 主流程硬编码。

### 删除重复 skill 转换

`crates/nova-cli/src/main.rs` 删除：

- `convert_loaded_skills`
- `convert_package`
- `convert_tool_policy`

包括相关 `use nova_skill_loader::{...}` import。

`--include-skill` 仍需调用 `nova_skill_loader::load_single_skill`，方案：

- 把 `load_skills` helper 升级为同时接受 base dir + extra paths：

```rust
pub fn load_skills(skills_dir: &Path, extra_paths: &[PathBuf]) -> Vec<SkillPackage> {
    let mut loaded = nova_skill_loader::load_skills_from_dir(skills_dir).unwrap_or_default();
    for path in extra_paths {
        match nova_skill_loader::load_single_skill(path) {
            Ok(Some(skill)) => loaded.push(skill),
            Ok(None) => log::warn!(...),
            Err(e) => log::error!(...),
        }
    }
    convert_loaded_skills(loaded)
}
```

并放在 `nova-agent-loader::skill_adapter`。CLI 与 bootstrap 都用同一份。

### nova-cli Cargo.toml

- 添加 `nova-agent-loader = { workspace = true }`。
- 移除 `nova-skill-loader` 依赖（若 CLI 不再直接 import）。
- 检查 `nova-agent` 是否还需要保留：`AgentRuntime`、`Message`、`AgentEvent` 等类型仍需，保留。

### CLI 输出行为对齐

- 现有 `--output-format stream-json` 模式逐字段打印 `AgentEvent`：与 server 路径一致。
- `print_skills` / `print_tasks` / `print_status` 接口不变（依赖 `agent.skill_registry`、`agent.task_store`）。
- `with_tools` 调用删除后，CLI 默认 system prompt 内容会变化（增加 environment / project context 等）。在 README 或 CHANGELOG 标注一次，避免用户惊讶。

## 测试案例

### 编译与依赖图

- `cargo build -p nova-cli`：通过。
- `cargo tree -p nova-cli`：包含 `nova-agent-loader`；不再直接包含 `nova-skill-loader`（间接依赖通过 loader）。
- `rg "convert_loaded_skills|convert_tool_policy|convert_package" crates/nova-cli`：0 命中。
- `rg "use nova_skill_loader" crates/nova-cli`：0 命中。

### CLI 端到端

- `cargo run -p nova-cli -- chat`：能进入 REPL，`/skills` 列出与 server 相同 skill 集合。
- `cargo run -p nova-cli -- run "echo hi"`：能完整执行一次 turn，stream-json 输出包含 `ToolStart`、`ToolEnd`、`TurnComplete`。
- 用 `--include-skill <path>` 指向一个临时 skill 目录，验证 `/skills` 中出现该 skill。

### system prompt 内容对齐

- 用一份相同的 `.nova/config.toml` 分别启动 CLI 与 stdio gateway，捕获各自第一条 system message，diff 结果应仅在不可避免的 dynamic 字段（如时间戳、模型 id）上有差异。

### 异常路径

- 删除 `skills/` 目录：CLI 仍能启动，`/skills` 提示空集合，不 panic。
- `--include-skill` 指向不存在的路径：日志 `error`，CLI 仍正常启动。
- 配置中无任何 agent：CLI 启动报错信息与 server 路径一致（来自 `build_agent_runtime`）。

### 现有单测

- `cargo test -p nova-cli`：现有 `CliCommand::parse` 等测试无关组装，应继续通过。
- `cargo test -p nova-agent-loader`：新增 `build_agent_runtime` 的单测，覆盖：
  - 仅 base skill 路径。
  - base + extra skill 路径合并。
  - 配置无 skill 目录时的容错。
