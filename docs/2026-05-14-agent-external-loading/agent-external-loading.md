# Agent External Loading

## 时间

- 创建日期：2026-05-14
- 最后更新：2026-05-15

## 项目现状

`nova-agent` 正处在外部资源加载职责迁移的中间状态：

- skill discovery 已抽出 `crates/nova-skill-loader`，但 `nova-agent` 仍通过 `SkillRegistry::load_from_dir_async` 间接依赖 loader，`app/bootstrap.rs` 仍从 agent registry 发起目录加载。
- agent prompt 文件加载仍分散在 `app/bootstrap.rs`、`app/agent_workspace_service.rs`、`tool/builtin/agent.rs`。
- developer project prompt、project context、workflow prompt 仍由 `SystemPromptBuilder::from_config_async` 通过 `PromptLoadContext` 在 prompt builder 内部读取。
- config loader、tool read/write、SQLite、provider 网络访问仍包含 IO；其中 config loader 和运行时能力不是本设计要移除的对象，重点是 skill/prompt/project/workflow 这类外部资源 discovery/load。

当前问题不是单个文件读取点，而是边界不稳定：`nova-agent` 同时承担 agent engine、外部文件发现、外部文件解析、加载错误策略和日志职责，导致 bootstrap、conversation turn、workspace inspect、Agent tool 动态创建存在重复且不完全一致的 prompt 解析路径。

### 当前代码问题清单

| 问题 | 位置 | 描述 |
|------|------|------|
| 重复 prompt 加载 | `bootstrap.rs:225-262`、`agent_workspace_service.rs:620-642`、`agent.rs:377-397` | 三个 `load_agent_prompt*` 函数各自实现 `prompt_file > prompt_inline > legacy > default` 优先级，逻辑大体一致但细节有差异（例如 bootstrap 有 `.context()` 报错；Agent tool 不尝试默认文件而是返回固定字符串） |
| Skill loader 反向依赖 | `nova-agent/Cargo.toml:29`、`registry/discovery.rs`、`registry/parser.rs` | `nova-agent` 依赖 `nova-skill-loader`，违反"agent 只消费已加载内容"的目标 |
| Builder 内嵌 IO | `prompt/builder.rs:262-336` | `from_config_async` 在内部调用 `load_developer_project_prompt_async`、`load_project_context_with_config_async`、`WorkflowStagePrompts::load_from_file_async` |
| Turn 级重复加载 | `conversation_service.rs:300-304`、`conversation_service.rs:358-371` | conversation service 先加载 project context 再重复加载 developer prompt，与 builder 中的加载逻辑重叠 |
| `PromptConfig` 混合模型 | `prompt/types.rs:119-156` | `PromptConfig` 同时包含已加载内容字段（`project_context_content`、`developer_project_prompt_content`）和路径字段（`load_context`、`workflow_prompt_path`），职责不清 |
| From impl 绑定 | `registry/parser.rs:58-84` | `SkillPackage` 和 `ToolPolicy` 对 `nova-skill-loader` 类型实现 `From`，形成编译期反向耦合 |

## 整体目标

将 `nova-agent` 收敛为纯 agent engine。最终依赖方向固定为：

```text
nova-cli / nova-server-ws / deskapp / app bootstrap
    ↓
外部资源加载层
    ├── nova-skill-loader
    ├── PromptMaterialLoader
    └── AgentDescriptorFactory
    ↓
nova-agent
```

最终状态下：

- `nova-agent` 只消费已加载、已解析的 `SkillPackage`、`PromptMaterial`、`TurnPromptMaterial`、`LoadedAgentDescriptor`。
- `nova-agent` 不扫描 skill 目录，不读取 `SKILL.md` / `skill.toml`，不读取 agent `prompt_file`，不在 prompt builder 内读取 project context、developer prompt、workflow prompt。
- 外部资源加载层负责路径解析、目录扫描、格式兼容、文件读取、错误策略、日志和降级。
- 配置语义保持兼容：`prompt_file`、`prompt_inline`、legacy `system_prompt_template`、默认 `agent-<id>.md` 仍可存在，但只在 loader/factory 层解析。
- prompt 组装、skill prompt 注入、tool policy 派生、turn 执行、conversation 状态继续属于 `nova-agent`。

## 设计原则

1. 依赖单向：外层依赖 loader 与 `nova-agent`；`nova-agent` 不依赖 `nova-skill-loader` 或 prompt loader。
2. 输入显式：agent engine API 接收内容和结构化模型，不接收外部资源路径。
3. 加载集中：同一种资源只有一个解析优先级、一个错误语义、一个日志边界。
4. 行为等价：迁移不改变 skill 路由、tool policy、prompt section 顺序、裁剪策略和现有配置语义。
5. 渐进收敛：先补注入契约，再迁移调用点，最后删除兼容加载 API。

## 最终分层职责

### 外部资源加载层

- `nova-skill-loader`：发现 skill 目录，解析 `skill.toml` 和兼容 `SKILL.md`，返回中立 loaded model。
- `PromptMaterialLoader`：解析 agent prompt、developer project prompt、project context、workflow prompt，输出 prompt material。
- `AgentDescriptorFactory`：把 `AgentSpec`、provider/model binding、prompt material 转换成可注入 `AgentRegistry` 的 loaded descriptor。
- app/bootstrap 或上层应用：选择 warn-skip、fail-fast、空内容降级等策略并记录文件路径类日志。

### nova-agent

- `SkillRegistry`：存储 skill package，支持 slug/name/alias 查询、输入匹配、catalog prompt、active skill prompt、tool policy 派生。
- `SystemPromptBuilder`：从已加载 material 组装 section，处理模板变量、section 顺序、project instruction profile、prompt diagnostics。
- `AgentRuntime`：执行 turn、调用 provider、调度工具、处理 loop guard 和消息流。
- `AgentRegistry`：持有已加载 agent descriptor，不解析 prompt 文件。

## 注入模型

### SkillRegistry 输入

目标 API：

```rust
SkillRegistry::from_packages(Vec<SkillPackage>)
SkillRegistry::replace_packages(&mut self, Vec<SkillPackage>)
SkillRegistry::extend_packages(&mut self, Vec<SkillPackage>)
```

旧 API `load_from_dir*` / `load_single_skill*` 只作为迁移桥接存在，并且不能再出现在运行时调用链。

### PromptMaterial

启动期或 agent descriptor 构建所需的稳定输入：

- `agent_id`
- `agent_prompt`
- `agent_catalog`
- `environment_snapshot`
- `initial_template_vars`
- `skill_injection_mode`
- `project_instruction_profile`

### TurnPromptMaterial

每轮 turn 可能变化的输入：

- `developer_project_prompt`
- `project_context`
- `workflow_prompt`
- `turn_template_vars`
- `active_skill`

目标 API：

```rust
SystemPromptBuilder::from_material(prompt_material, turn_material, skills)
```

`from_config_async` 仅作为迁移桥接；最终 builder 不持有路径，不执行文件 IO。

### LoadedAgentDescriptor

外层 factory 输出已经加载好的 agent descriptor：

- prompt 内容已解析完成。
- provider/model binding 已解析完成。
- template vars 已初始化完成。
- `AgentRegistry` 只注册 descriptor，不读取 `prompt_file`。

## Plan 拆分

| Plan | 描述 | 依赖 | 执行顺序 | 状态 |
| --- | --- | --- | --- | --- |
| Plan 1 | 稳定 agent 注入契约与边界模型 | 无 | 1 | 待开始 |
| Plan 2 | Skill loader 与 `nova-agent` 脱钩 | Plan 1 | 2 | 待开始 |
| Plan 3 | Prompt material loader 与纯 prompt builder | Plan 1 | 3 | 待开始 |
| Plan 4 | Bootstrap、turn、inspect、Agent tool 调用链统一 | Plan 2、Plan 3 | 4 | 待开始 |
| Plan 5 | 删除旧 API、验证依赖边界、补迁移说明 | Plan 4 | 5 | 待开始 |

## 非目标

- 不改变现有 skill 格式兼容策略；`skill.toml` 与 `SKILL.md` 兼容仍由 loader 支持。
- 不改变 prompt section 的业务含义、顺序、裁剪策略和 diagnostics 输出目的。
- 不重构 tool 执行、provider、conversation repository、SQLite store。
- 不移除 tool builtin 的文件读写能力；工具读写是 agent runtime 能力，不属于外部资源 discovery/load。
- 不新增配置语义；只改变配置字段被解析的位置。

## 风险与决策

- `PromptMaterialLoader` 的位置需要选择：可以先落在 app 层，等边界稳定后再抽 crate，避免过早拆分。
- legacy `system_prompt_template` 仍需保留兼容读取，但只能在 factory 层转换成 prompt 内容。
- project developer prompt 和 project context 依赖 turn 的 project dir，必须支持 turn 级重新加载或缓存失效，不能只在启动期加载。
- loader 层应统一记录文件路径类错误，agent 层只保留 prompt 注入 diagnostics，避免重复日志。
- 删除旧 API 前必须确认 `build_application`、`conversation_service`、`agent_workspace_service`、`tool/builtin/agent` 均不再直接读 prompt 文件。

## 验收标准

- `crates/nova-agent/Cargo.toml` 不依赖 `nova-skill-loader`。
- `crates/nova-agent/src/skill` 不包含目录扫描、skill 文件读取、loader crate 调用。
- `SystemPromptBuilder` 构建路径不调用 `tokio::fs` / `std::fs` 读取 prompt/project/workflow 文件。
- bootstrap、conversation turn、workspace inspect、Agent tool 动态创建使用同一套 prompt material loader/factory。
- 无 skill、无 developer prompt、无 project context、无 workflow prompt 时 agent 可正常启动并保持确定输出。
- workspace 级 `cargo clippy --workspace -- -D warnings`、`cargo fmt --all`、`cargo test --workspace` 通过。
