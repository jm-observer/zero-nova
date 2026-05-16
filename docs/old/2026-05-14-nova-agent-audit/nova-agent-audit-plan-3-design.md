# Plan 3 详细设计：收敛结构性问题与误导性抽象

## 时间

2026-05-14

## Plan 编号与标题

Plan 3：结构性问题收敛（重复定义、空壳文件、参数爆炸、默认值碎片化、命名歧义、无效入口、参数过多）

## 前置依赖

- 不依赖 Plan 1/2 完成，可独立评审。
- 不引入新依赖。
- 本 Plan 仅处理问题 10、11、12、13、14、15、16，不包含问题 9 的 crate 拆分。
- 除命名去歧义与空壳清理外，优先保持外部接口兼容；如必须调整接口，先提供过渡层。

## 当前状态概览

Plan 3 原始审计列出 8 个结构性问题，其中问题 9 是长期 crate 拆分议题，本次明确排除。其余问题可以分为三类：

1. 模型与类型层问题：问题 10、12、13、14。
2. 文件与模块结构问题：问题 11、15。
3. 调用面与接口设计问题：问题 16。

这些问题共同特征是：

- 不一定立即导致错误，但持续提高理解成本。
- 多数属于“名字看起来合理，但实际职责不清或边界漂移”。
- 修复成本可控，适合拆成多个小步独立落地。

## 本次目标（可验证）

1. `ToolDefinition` 只保留一个领域模型作为 source of truth，注册层元数据不再伪装成同名模型。
2. 删除或迁移空壳 placeholder / 单行 re-export 文件，避免误导模块边界。
3. `PromptConfig` 与“文件加载上下文”分离，构建配置与加载配置各司其职。
4. 配置默认值不再散落为 30+ 个零散函数，而是按结构体或常量集中管理。
5. `agent_catalog.rs` 中的 catalog 专用模型配置完成去歧义命名。
6. 删除 `lib.rs` 中无实际意义的 `run()` 占位入口。
7. 用结构体参数替代 `#[allow(clippy::too_many_arguments)]` 的 8 参数函数，并移除对应 `#[allow]`。

## 涉及文件

### 重点文件

- `crates/nova-agent/src/tool/registry.rs`
- `crates/nova-agent/src/provider/types.rs`
- `crates/nova-agent/src/prompt/mod.rs`
- `crates/nova-agent/src/app/bootstrap.rs`
- `crates/nova-agent/src/config/models.rs`
- `crates/nova-agent/src/agent_catalog.rs`
- `crates/nova-agent/src/lib.rs`
- `crates/nova-agent/src/agent/runtime.rs`
- `crates/nova-agent/src/agent/turn_executor.rs`

### 预计删除或合并的文件

- `crates/nova-agent/src/agent/stream_bridge.rs`
- `crates/nova-agent/src/agent/turn_executor.rs` 中的占位实现若仍为空，则与实际实现重新归位
- `crates/nova-agent/src/skill/model.rs`
- `crates/nova-agent/src/skill/policy.rs`
- `crates/nova-agent/src/conversation/repository/message_repo.rs`
- `crates/nova-agent/src/conversation/repository/session_repo.rs`

说明：若 `agent/turn_executor.rs` 已在 Plan 2 中承载真实逻辑，则本 Plan 仅清理仍为空壳的其他文件，不强制删除该文件。

---

## Plan 拆分与执行顺序

| 子计划 | 标题 | 目标 | 依赖 |
|--------|------|------|------|
| Plan 3.1 | `ToolDefinition` 统一建模 | 清除同名双模型与手工转换歧义 | 无 |
| Plan 3.2 | 空壳文件与伪模块清理 | 删除 placeholder / 单行 re-export 间接层 | Plan 3.1 可并行 |
| Plan 3.3 | `PromptConfig` 拆分 | 区分 prompt 构建参数与加载上下文 | Plan 3.1 |
| Plan 3.4 | 配置默认值收敛 | 默认值集中化、减少碎片函数 | 无 |
| Plan 3.5 | catalog `ModelConfig` 去歧义 | 解除与 provider 运行时模型配置重名 | 无 |
| Plan 3.6 | 无效入口与参数爆炸修复 | 删除空 `run()`、引入 `TurnParams` | Plan 2.3 最好已完成 |

说明：Plan 3.6 与 Plan 2 的 runtime 单路径化存在局部耦合。若 Plan 2 尚未落地，仍可先设计并局部实施 `TurnParams`，但建议排在运行路径统一之后。

---

# Plan 3.1：`ToolDefinition` 统一建模

## 目标

消除 `tool::registry::ToolDefinition` 与 `provider::types::ToolDefinition` 的同名双定义问题，确保“给 LLM 的工具 schema”与“工具注册时的额外元数据”在模型边界上清晰分离。

## 当前问题

当前存在两个层次不同、名字却完全相同的结构：

- `provider::types::ToolDefinition`：用于 provider 请求载荷。
- `tool::registry::ToolDefinition`：用于本地工具注册，额外携带 `defer_loading` 等本地元数据。

风险：

- 新增字段时容易只改一边。
- 调用方 import 时容易误用错误类型。
- 手工转换逻辑会重复散落在 registry / provider / runtime 之间。

## 设计

### 建模原则

1. 统一以 `provider::types::ToolDefinition` 作为“LLM 工具定义”的唯一规范结构。
2. 工具注册层不再复用相同名字，而是显式表达“注册项”与“工具 schema”的包含关系。
3. 本地注册元数据仅存在于 registry 层，不透传到 provider。

### 新结构建议

```rust
pub(crate) struct RegisteredTool {
    pub definition: provider::types::ToolDefinition,
    pub defer_loading: bool,
    pub category: ToolCategory,
    pub loader: Arc<dyn ToolLoader>,
}
```

若当前结构还承载更多 registry-only 字段，可继续保留，但 `definition` 字段必须成为 provider schema 的单一载体。

### API 调整

| 旧接口 | 新接口 | 说明 |
|--------|--------|------|
| `register(tool_def: tool::registry::ToolDefinition, ...)` | `register(definition: provider::types::ToolDefinition, metadata: ToolRegistrationMeta, ...)` | 显式拆分 schema 与元数据 |
| `tool_definitions()` 返回 registry 本地定义 | `tool_definitions()` 返回 provider schema 列表 | 对齐方法语义 |
| 各类手工 `impl From<registry::ToolDefinition>` | 删除或收敛为 `RegisteredTool::definition.clone()` | 减少转换层 |

可引入：

```rust
pub(crate) struct ToolRegistrationMeta {
    pub defer_loading: bool,
    pub category: ToolCategory,
}
```

用于避免构造 `RegisteredTool` 时参数继续膨胀。

### 迁移步骤

1. 给 registry 新增 `RegisteredTool` / `ToolRegistrationMeta`。
2. 将原 `tool::registry::ToolDefinition` 替换为 `RegisteredTool` 或更具体命名。
3. 编译器驱动修正所有 import 与构造调用点。
4. 删除旧同名结构及其转换实现。

## 测试

- 工具注册后 `tool_definitions()` 返回的 schema 与 provider 请求内容一致。
- deferred tool 元数据仍能驱动延迟加载。
- 新增一个带可选字段的工具定义时，只需维护一处 schema 结构。

---

# Plan 3.2：空壳文件与伪模块清理

## 目标

删除无内容 placeholder 与单行 re-export 模块，恢复“文件存在即有真实职责”的结构约定。

## 当前问题

当前空壳文件分两类：

1. 纯占位注释文件，例如 `agent/stream_bridge.rs`。
2. 纯 re-export 文件，例如 `skill/model.rs`、`message_repo.rs`。

这类文件的问题不是体量，而是误导：

- 阅读者会误以为模块已拆分完成。
- IDE 跳转会多一层无价值中转。
- 后续真实迁移时容易在错误文件上继续累积代码。

## 设计

### 处理策略

| 文件类型 | 处理方式 | 原则 |
|----------|----------|------|
| Placeholder 注释文件 | 直接删除 | 若未承载真实实现，不保留“未来计划”式文件 |
| 单行 re-export 文件 | 合并回源文件并删除 | 仅当该模块形成真实封装边界时才保留 |
| 已有部分实现但命名错误的文件 | 迁入真实逻辑后保留 | 文件名应对应职责 |

### `skill/model.rs` / `skill/policy.rs`

建议：

1. 调用方直接改为从 `skill::types` import。
2. 若过渡期确实需要兼容，一轮版本内可在 `skill/mod.rs` 做 re-export，而不是单独保留空文件。

示例：

```rust
pub use self::types::{
    CapabilityPolicy,
    FileToolPriority,
    PolicySource,
    Skill,
    SkillPackage,
    ToolPolicy,
    ToolStatus,
};
```

### `conversation/repository/message_repo.rs` / `session_repo.rs`

若 repository 已在 `mod.rs` 实现全部逻辑，则：

1. 直接删除单行文件。
2. 调整模块声明，避免 `pub mod message_repo;` 继续指向空文件。

若审计后发现未来确实要迁入独立实现，则应在真正迁移当次创建文件，而不是提前占位。

## 测试

- `cargo check` 确认模块路径与 import 全部更新。
- `cargo test -p nova-agent skill`
- `cargo test -p nova-agent conversation`

---

# Plan 3.3：`PromptConfig` 与加载上下文拆分

## 目标

将 prompt 构建参数与文件加载上下文解耦，减少 `PromptConfig` 的职责漂移和 builder 膨胀。

## 当前问题

`PromptConfig` 当前同时承载两类信息：

1. Prompt 构建阶段真正需要的行为参数。
2. Bootstrap 阶段用于查找和预加载开发者 prompt 文件的路径信息。

例如：

- `project_dir`
- `developer_prompt_files`
- `project_context_path`

这些字段更接近“输入来源”而非“构建配置”。它们混入 `PromptConfig` 后会导致：

- builder 方法数量膨胀。
- 调用方必须知道过多底层加载细节。
- 测试构造 prompt 时也要填充与构建无关的路径字段。

## 设计

### 新结构边界

```rust
pub struct PromptConfig {
    // 仅保留 prompt 拼装真正需要的参数
}

pub struct PromptLoadContext {
    pub project_dir: Option<PathBuf>,
    pub developer_prompt_files: Vec<String>,
    pub project_context_path: Option<PathBuf>,
    pub preloaded_developer_prompts: Vec<String>,
    pub preloaded_project_context: Option<String>,
}
```

关键点：

- `PromptConfig` 负责“怎么拼 prompt”。
- `PromptLoadContext` 负责“从哪里拿原始文本，以及是否已预加载”。
- bootstrap 层先把文件读取完成，再把纯文本交给 prompt builder。

### 组装流程

```text
Bootstrap/AppConfig
  -> 解析文件路径配置
  -> 异步读取 developer prompt / project context
  -> 生成 PromptLoadContext
  -> 与 PromptConfig 一起传给 PromptBuilder
```

### API 设计建议

方案 A：Builder 接收两个参数

```rust
pub async fn build_prompt(
    &self,
    config: &PromptConfig,
    load_context: &PromptLoadContext,
) -> Result<CompiledPrompt>
```

方案 B：在 bootstrap 完成预加载，prompt 层仅接收纯文本片段

```rust
pub struct PromptSources {
    pub developer_prompts: Vec<String>,
    pub project_context: Option<String>,
}
```

推荐方案 B。原因：

- prompt 模块不应再感知路径类型。
- 更利于测试，调用方直接传字符串即可。
- 更符合“禁止阻塞 async runtime”的边界要求，I/O 完全停留在 app/bootstrap 层。

### 迁移规则

1. 先新增 `PromptSources` 或 `PromptLoadContext`。
2. 将路径读取逻辑从 `prompt` 模块迁至 `app/bootstrap` 或专用 loader。
3. 删除 `PromptConfig` 中与路径、文件发现、预加载相关字段和 builder 方法。
4. 更新相关测试，使其直接构造纯文本输入。

## 测试

- 正常路径：预加载 developer prompt 与 project context 后，最终 prompt 内容不变。
- 边界路径：无 `project_dir`、无 `project_context_path` 时仍可构建 prompt。
- 错误路径：文件读取失败在 bootstrap 层返回带 context 的错误，不在 prompt 层吞错。

---

# Plan 3.4：配置默认值集中化

## 目标

消除 `config/models.rs` 中零散的 `default_xxx()` 函数，将默认值集中为结构体 `Default` 实现或具名常量，提升可读性和可维护性。

## 当前问题

当前模式通常是：

```rust
fn default_host() -> String { "127.0.0.1".to_string() }
fn default_port() -> u16 { 8080 }
```

问题：

- 默认值分散，无法一眼看出某个配置对象的整体默认行为。
- 字段新增时容易忘记同步相关默认函数。
- 函数名数量庞大，污染文件纵向空间。

## 设计

### 规则

1. 标量默认值优先提取为具名常量。
2. 每个配置结构体实现 `Default`，在实现中集中填充字段默认值。
3. `serde(default)` 优先落到结构体级别或字段级默认表达式，不再到处引用散落函数。

### 建议模式

```rust
const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8080;

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
        }
    }
}
```

### 对 `serde` 的处理

若当前依赖字段级 `#[serde(default = "default_host")]`，可改为两种方式之一：

方案 A：保留少量必要 helper，但按结构体邻近放置。

方案 B：使用 `#[serde(default)]` + 结构体 `Default`。

推荐优先方案 B。仅当某字段需要区别于结构体整体默认策略时，才保留局部 helper。

### 文件组织

如果 `models.rs` 仍然过长，可按配置域拆分：

- `config/models/server.rs`
- `config/models/agent.rs`
- `config/models/provider.rs`

但本 Plan 的核心目标是默认值集中化，不强制同时做目录级拆分，避免把问题 13 混成大重构。

## 测试

- 反序列化缺省字段时得到的值与重构前一致。
- `Default::default()` 生成的配置可直接用于现有测试。
- 对关键配置增加回归测试，验证端口、迭代次数、超时等默认值未意外变化。

---

# Plan 3.5：catalog `ModelConfig` 去歧义

## 目标

将 `agent_catalog.rs` 中仅用于 catalog 序列化的 `ModelConfig` 重命名，避免与 `provider::ModelConfig` 混淆。

## 当前问题

当前存在两个语义不同但名称相同的 `ModelConfig`：

- `provider::ModelConfig`：运行时请求模型参数。
- `agent_catalog::ModelConfig`：catalog 中的 agent 级覆盖配置。

这会导致：

- import 时必须频繁起别名。
- 搜索 `ModelConfig` 时结果噪音大。
- 审阅时难以快速判断该配置是否会直接影响 provider 调用。

## 设计

### 重命名建议

优先命名：

- `AgentModelOverride`

备选：

- `CatalogModelConfig`

推荐 `AgentModelOverride`，因为它直接表达“catalog 对默认模型配置的覆盖项”，语义比 “CatalogModelConfig” 更具体。

### 迁移方式

1. 在 `agent_catalog.rs` 中重命名结构体与相关字段类型。
2. 更新 serde 序列化测试，确认外部 JSON 字段名保持不变。
3. 若该类型被公开导出，过渡期可提供 type alias：

```rust
#[deprecated(note = "use AgentModelOverride instead")]
pub type ModelConfig = AgentModelOverride;
```

若项目要求严格避免保留歧义名称，也可直接删除旧 alias，并同步修正调用方。

### 兼容策略

结构体 Rust 类型名变化不影响 JSON schema，前提是字段名与 serde 属性不变。因此此改动应优先视为源码层去歧义，不扩大为协议变更。

## 测试

- catalog 配置文件反序列化前后兼容。
- 生成的 schema 或导出配置示例无字段变化。
- 相关调用方不再需要 `use ... as ...` 处理同名冲突。

---

# Plan 3.6：删除无效入口并收敛参数爆炸

## 目标

删除 `lib.rs` 中无意义 `run()` 占位函数，并将 8 参数 turn 执行函数改造为结构体入参，去除 `#[allow(clippy::too_many_arguments)]`。

## 当前问题

### 问题 15：空 `run()`

`lib.rs` 中的：

```rust
pub async fn run() -> anyhow::Result<()> {
    log::info!("nova-core started");
    Ok(())
}
```

没有业务价值，且日志内容错误。

### 问题 16：参数爆炸 + `#[allow]`

`run_turn_with_model_config` 与 `run_turn_with_context_and_model_config` 各有 8 个参数，通过 `#[allow(clippy::too_many_arguments)]` 压制告警，直接违反项目规范。

## 设计

### 删除 `run()`

处理方式：

1. 搜索调用方确认无依赖。
2. 直接删除函数。
3. 若 crate 需要统一入口，应由 `src/main.rs` 或 app/bootstrap 提供真正运行入口，而非保留空函数。

### 引入 `TurnParams`

建议新增：

```rust
pub(crate) struct TurnParams<'a> {
    pub session: &'a SessionState,
    pub input: &'a TurnInput,
    pub turn_context: Option<&'a TurnContext>,
    pub model_config: &'a provider::ModelConfig,
    pub event_tx: &'a EventDispatcher,
    pub tool_registry: &'a ToolRegistry,
    pub loop_guard: &'a LoopGuard,
    pub conversation_store: &'a ConversationStore,
}
```

实际字段以现有函数签名为准，核心要求是：

- 调用层不再按位置传 8 个参数。
- 共享依赖按借用传递，避免不必要 `.clone()`。
- 当新字段出现时，只扩展结构体，不继续增加函数参数个数。

### 函数签名调整

```rust
async fn run_turn_with_model_config(&self, params: TurnParams<'_>) -> Result<TurnResult>
async fn run_turn_with_context_and_model_config(&self, params: TurnParams<'_>) -> Result<TurnResult>
```

如果 Plan 2 已完成单路径化，甚至可进一步收敛为：

```rust
async fn run_turn_with_context(&self, params: TurnParams<'_>) -> Result<TurnResult>
```

但本 Plan 不强制把“删旧路径”和“参数收敛”绑死为一次提交；优先先移除 `#[allow]`，再视运行路径状态继续合并函数。

### 构造方式

建议在调用点使用具名字面量构造，提升可读性：

```rust
let params = TurnParams {
    session,
    input,
    turn_context: Some(&turn_context),
    model_config,
    event_tx: &self.events,
    tool_registry: &self.tool_registry,
    loop_guard: &self.loop_guard,
    conversation_store: &self.conversation_store,
};
```

### 生命周期与所有权约束

优先使用借用而非拥有值，理由：

- turn 执行链中大部分对象本来由 runtime 持有。
- 避免为了满足结构体字段而把共享组件改成 `Arc` clone 传递。
- 让 `TurnParams` 明确表达“调用期临时视图”，而不是长期保存状态。

## 测试

- 搜索确认 `lib::run()` 无调用方后删除，`cargo test` 通过。
- turn 相关测试覆盖正常路径、tool call 路径、带 context 路径。
- `cargo clippy --workspace -- -D warnings` 不再出现 `too_many_arguments`，且代码中不再保留对应 `#[allow]`。

---

## 分阶段验收标准

### Plan 3.1 验收

- crate 内不再存在两个同名 `ToolDefinition` 结构。
- provider schema 只有一套定义。
- registry 元数据通过包装结构表达。

### Plan 3.2 验收

- 所列 placeholder / 单行 re-export 文件被删除或迁入真实实现。
- import 路径不再经过无价值中间层。

### Plan 3.3 验收

- `PromptConfig` 不再包含文件路径或预加载来源字段。
- prompt 层不直接负责文件读取。

### Plan 3.4 验收

- `config/models.rs` 中零散默认函数显著减少，默认值按结构体集中。
- 关键默认值行为保持不变。

### Plan 3.5 验收

- catalog 配置类型不再使用 `ModelConfig` 这一歧义名称。
- provider 运行时配置与 catalog 覆盖配置搜索结果可明显区分。

### Plan 3.6 验收

- `lib.rs` 中无空 `run()` 占位入口。
- `too_many_arguments` 的 `#[allow]` 被移除。
- turn 执行核心函数使用结构体参数。

---

## 总体验证命令

```bash
cargo clippy --workspace -- -D warnings
cargo fmt --all --check
cargo test --workspace
```

---

## 风险与规避

| 风险 | 影响 | 规避 |
|------|------|------|
| `ToolDefinition` 统一后影响注册与 provider 请求构造 | 中 | 先引入包装结构，再由编译器驱动迁移调用点 |
| 删除空壳文件时触发大量 import 变更 | 低 | 一次只删一类模块，保持变更聚焦 |
| `PromptConfig` 拆分影响调用链较广 | 高 | 先增加新结构与兼容构造，再删除旧字段 |
| 默认值集中化导致 serde 缺省行为变化 | 中 | 用回归测试锁定重构前默认值 |
| catalog 配置类型重命名影响外部 import | 低 | 需要时保留一轮 deprecated alias |
| `TurnParams` 生命周期设计不当导致借用冲突 | 中 | 先以只借用只读依赖为主，必要时拆成更小上下文结构 |

---

## 非目标

1. 不处理问题 9 的 crate 拆分与 workspace 结构调整。
2. 不重写 provider 协议。
3. 不在本 Plan 中顺带做大规模目录重组。
4. 不改变 catalog 或 provider 的外部 JSON 字段名，除非已有错误必须修正。
5. 不在本 Plan 中引入新配置格式或新运行模式。
