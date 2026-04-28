# Plan 1：配置模型对齐与分层重构

## 前置依赖
- 无

## 本次目标
- 为 `.nova/config.toml` 定义一套与当前项目代码一致的目标结构。
- 消除"字段存在但不会生效"的配置项。
- 明确每个配置区块的消费方，避免一个字段被多个模块以不同语义解释。
- 提供目标 Rust 结构体定义，确保 TOML 结构与代码模型一一对应。
- 修正代码默认值与实际使用值不一致的问题。

## 涉及文件
- `.nova/config.toml`
- `.nova/examples/agents.toml`
- `.nova/README.md`
- `crates/nova-agent/src/config.rs`
- `crates/nova-agent/src/app/bootstrap.rs`
- `crates/nova-agent/src/provider/mod.rs`（`ModelConfig` 定义所在）
- `deskapp/src-tauri/src/config.rs`
- `crates/nova-server/src/bin/nova_gateway_ws.rs`

## 详细设计

### 1. 配置按消费者分层

保留单文件方案，但在文档和结构上明确三类配置所有者：

- `provider`、`llm`、`search`、`tool`、`gateway`、`voice`：由 `nova-agent` / `nova-server` 消费
- `remote`：由 `deskapp` 消费，用于连接已运行的 Gateway
- `sidecar`：由 `deskapp` 消费，用于拉起内置 Gateway 进程

其中 `provider` 与 `llm` 进一步拆分职责：

- `provider`：描述供应商接入信息与协议端点，例如 `api_key`、`base_url`
- `llm`：描述默认模型与推理参数，例如 `model`、`max_tokens`、`temperature`、`top_p`、`thinking_budget`、`reasoning_effort`

这样可以避免未来接入多个 provider 时，把"认证/路由配置"和"调用参数模板"绑死在一个区块里。

目标 TOML 结构示意：

```toml
# ── 供应商接入 ──────────────────────────────────────────────────
[provider]
api_key = ""
base_url = "http://127.0.0.1:8082/v1"

# ── 模型调用参数 ────────────────────────────────────────────────
[llm]
model = "gpt-oss-120b"
max_tokens = 8192
temperature = 0.7
top_p = 1.0
# 按 provider 能力二选一
thinking_budget = 4096
# reasoning_effort = "medium"

# ── 搜索 ───────────────────────────────────────────────────────
[search]
backend = "tavily"
tavily_api_key = ""

# ── 工具与能力 ──────────────────────────────────────────────────
[tool]
skills_dir = "skills"
prompts_dir = "prompts"
project_context_file = "PROJECT.md"
default_policy = "workflow"

[tool.bash]
shell = "powershell"
sandbox = false

# ── 语音 ───────────────────────────────────────────────────────
# [voice] 区块本次不做改动，保持现有结构
# voice.enabled / stt_model / tts_model / tts_voice 等字段已与代码对齐

# ── 网关 ───────────────────────────────────────────────────────
[gateway]
host = "127.0.0.1"
port = 18801
max_iterations = 30
tool_timeout_secs = 3600
subagent_timeout_secs = 300
max_tokens = 4096
skill_routing_enabled = false
skill_history_strategy = "global"
use_turn_context = true

[gateway.trimmer]
enabled = true
context_window = 128000
output_reserve = 8192
min_recent_messages = 10

[gateway.side_channel]
enabled = false
skill_reminder_interval = 5
inject_date = true

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "默认通用助手，负责日常问答与任务分发"
aliases = ["小助手", "助手"]
prompt_file = "agent-nova.md"

# ── 桌面端远程连接 ──────────────────────────────────────────────
[remote]
enabled = true
host = "127.0.0.1"
port = 18801

# ── Sidecar 启动 ───────────────────────────────────────────────
[sidecar]
mode = "auto"
name = "Built-in Gateway"
command = "nova_gateway_ws"
args = []
port_arg = "--port"
workspace_arg = "--workspace"
```

说明：

- `provider` 是"怎么连到模型服务"，`llm` 是"默认怎么调用模型"。
- `gateway.port` 作为网关实际监听端口，是服务端唯一真值来源。
- `remote.port` 表示桌面端默认连接端口；默认情况下应与 `gateway.port` 保持一致，但它属于"客户端默认值"，不应反向影响服务端。
- `sidecar` 只负责"如何启动 Gateway 进程"，不再重复声明 host/port 本体，而是由启动参数注入。

### 1.1 为什么要拆 `provider` / `llm`

当前 `[llm]` 混放存在三个具体问题：

- 切换供应商时，连接参数与模型参数总是一起改，难以表达"同一套推理参数跑在不同 provider 上"。
- `thinking_budget` 与 `reasoning_effort` 属于 provider/协议相关能力，不应和通用采样参数完全同层无约束混放。
- 后续若支持 agent 级 `model_config` 覆盖，覆盖的应当主要是 `llm` 参数，而不是连带覆盖 `api_key` / `base_url`。

拆分后建议的覆盖关系：

- 全局 `provider`：决定客户端如何建连
- 全局 `llm`：决定默认模型调用参数
- `[[gateway.agents]].model_config`：只覆盖 `llm` 层的局部字段，不覆盖 `provider`

### 1.2 关于 `provider.kind` 的决定

当前代码只有 `OpenAiCompatClient` 一个 provider 实现，暂不引入 `provider.kind` 字段。原因：

- 没有对应的代码分发逻辑，引入后会成为文档批评的"字段存在但不生效"同类问题。
- 当前所有 provider（包括 Anthropic 兼容端点）都走 OpenAI-compatible 协议，不存在需要分发的场景。

后续若接入原生 Anthropic SDK 或其他协议，届时再引入 `kind` 字段和对应的分发逻辑。在此之前，`[provider]` 区块只保留 `api_key` 和 `base_url`。

### 2. 移除或收敛当前无效区块

当前 `.nova/config.toml` 中的以下区块在现阶段不建议继续作为默认配置暴露：

- `[gateway.router]`
- `[gateway.interaction]`
- `[gateway.interaction.risk]`
- `[gateway.workflow]`

原因：

- 这些区块在 `crates/nova-agent/src/config.rs` 中没有对应模型。
- 它们出现在默认配置中，会给人"该能力已接入并可调节"的错误预期。

处理策略：

- 若短期不会落地实现：从默认 `config.toml` 中移除，仅保留在设计文档中作为未来扩展项。
- 若近期会实现：先在 Rust 配置模型中补齐结构体与默认值，再重新暴露到 TOML 中。

### 3. 修正 Agent Prompt 配置语义

#### 3.1 当前问题

当前 `system_prompt_template` 存在"文件路径"和"内联内容"双重语义混用问题。`bootstrap.rs:80-81` 的实际行为：

```rust
let agent_prompt = match &agent.system_prompt_template {
    Some(prompt) => prompt.clone(),  // 直接当字符串用，不读文件
    None => match tokio::fs::read_to_string(&prompt_path).await { ... }
};
```

这意味着配置中写 `system_prompt_template = "agent-nova.md"` 时，字符串 `"agent-nova.md"` 会被原样作为 prompt 发给模型，而不是去读取同名文件。

#### 3.2 新设计

改为二选一字段：

- `prompt_file`：指向 `tool.prompts_dir` 下的模板文件
- `prompt_inline`：直接内联 prompt 内容

约束：

- 两者最多只能设置一个，同时存在时启动报错。
- 两者都为空时，按 `agent-{id}.md` 自动推导（与当前 `None` 分支行为一致）。

#### 3.3 `AgentSpec` 结构体改造

```rust
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AgentSpec {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub aliases: Vec<String>,
    /// 指向 prompts_dir 下的模板文件名
    pub prompt_file: Option<String>,
    /// 直接内联的 prompt 内容
    pub prompt_inline: Option<String>,
    /// 旧字段，兼容期间保留，Plan 2 负责映射逻辑
    #[serde(default)]
    pub system_prompt_template: Option<String>,
    pub tool_whitelist: Option<Vec<String>>,
    pub model_config: Option<AgentModelConfig>,
}
```

#### 3.4 `bootstrap.rs` 加载逻辑改造

```rust
// 1. 优先使用新字段
let agent_prompt = if let Some(file) = &agent.prompt_file {
    let path = config.prompts_dir().join(file);
    tokio::fs::read_to_string(&path).await?
} else if let Some(inline) = &agent.prompt_inline {
    inline.clone()
} else if let Some(legacy) = &agent.system_prompt_template {
    // 兼容路径，详见 Plan 2
    handle_legacy_prompt(legacy, config).await
} else {
    // 自动推导：agent-{id}.md
    let prompt_file = format!("agent-{}.md", agent.id);
    let prompt_path = config.prompts_dir().join(&prompt_file);
    tokio::fs::read_to_string(&prompt_path).await.unwrap_or_default()
};
```

### 4. 路径字段统一为"相对 workspace 解析"

以下字段统一支持"相对 workspace 目录"解析。注意：代码中 `workspace` 即 `.nova/` 目录本身。

- `tool.skills_dir`：相对 workspace (`.nova/`) 解析，默认 `"skills"` → `.nova/skills`
- `tool.prompts_dir`：相对 workspace (`.nova/`) 解析，默认 `"prompts"` → `.nova/prompts`
- `tool.project_context_file`：相对 workspace (`.nova/`) 解析
- `sidecar.command`：仅当不是绝对路径时按 PATH 查找

规则：

- 绝对路径：直接使用
- 相对路径：相对 workspace（即 `.nova/`）解析
- 命令名（仅 `sidecar.command`）：按 PATH 查找

注意：当前代码中 `prompts_dir()` 已经是相对 workspace 解析（`config.rs:327`），本次只需确保文档描述与代码一致。`skills_dir()` 的行为相同（`config.rs:315`）。

### 5. 修正代码默认值

`GatewayConfig::default_port()` 当前返回 `9090`，与所有文档和配置文件中的 `18801` 不一致。

改动：

```rust
fn default_port() -> u16 {
    18801  // 原为 9090
}
```

虽然在配置文件存在的情况下默认值不会生效，但默认值应当反映项目约定的标准端口，避免在配置文件缺失场景下产生意外行为。

### 6. 修复 `config_path()` 方法

当前 `AppConfig::config_path()` 硬编码返回 `workspace.join("config.toml")`，完全忽略了 `self.config_path` 字段。

改动：

```rust
pub fn config_path(&self) -> PathBuf {
    match &self.config_path {
        Some(p) => {
            let path = PathBuf::from(p);
            if path.is_absolute() {
                path
            } else {
                self.workspace.join(path)
            }
        }
        None => self.workspace.join("config.toml"),
    }
}
```

### 7. `sidecar.workspace_arg` 新增字段

目标 TOML 中新增 `workspace_arg = "--workspace"`，用于 deskapp 在 auto 模式下拼接 sidecar 启动命令时注入 workspace 路径。

需要在 `deskapp/src-tauri/src/config.rs` 的 `SidecarConfig` 中新增：

```rust
pub struct SidecarConfig {
    // ... 现有字段 ...
    /// workspace 路径参数名。例如 "--workspace"。
    /// 若为 None，则不传递 workspace 参数给 sidecar。
    pub workspace_arg: Option<String>,
}
```

当前 `SidecarConfig.workspace` 字段保留，其语义从"硬编码 workspace 路径"调整为"覆盖自动检测的 workspace 路径"。

### 8. 配置职责边界

在文档中明确以下边界：

| 区块 | nova-agent / nova-server | deskapp | 说明 |
|---|---|---|---|
| `[provider]` | ✅ 消费 | ❌ 不解析 | 供应商连接信息 |
| `[llm]` | ✅ 消费 | ❌ 不解析 | 模型调用参数 |
| `[search]` | ✅ 消费 | ❌ 不解析 | 搜索后端 |
| `[tool]` | ✅ 消费 | ❌ 不解析 | 工具与能力 |
| `[voice]` | ✅ 消费 | ❌ 不解析 | 语音配置 |
| `[gateway]` | ✅ 消费 | ❌ 不解析 | 网关运行时 |
| `[gateway.trimmer]` | ✅ 消费 | ❌ 不解析 | 历史裁剪 |
| `[gateway.side_channel]` | ✅ 消费 | ❌ 不解析 | 侧信道注入 |
| `[[gateway.agents]]` | ✅ 消费 | ❌ 不解析 | Agent 注册 |
| `[remote]` | ❌ 不解析 | ✅ 消费 | 远程连接 |
| `[sidecar]` | ❌ 不解析 | ✅ 消费 | Sidecar 启动 |

- `crates/nova-server` 负责把 CLI 参数覆盖到 `gateway.host` / `gateway.port`，但不会反写配置文件。

### 9. 目标 Rust 结构体定义

#### 9.1 `OriginAppConfig`（nova-agent 侧）

```rust
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct OriginAppConfig {
    #[serde(default)]
    pub provider: ProviderConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub tool: ToolConfig,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub voice: VoiceConfig,
    #[serde(default)]
    pub config_path: Option<String>,
}
```

#### 9.2 `ProviderConfig`（新增）

```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProviderConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: default_base_url(),
        }
    }
}
```

#### 9.3 `LlmConfig`（瘦身后）

```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LlmConfig {
    #[serde(flatten)]
    pub model_config: ModelConfig,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            model_config: ModelConfig {
                model: "gpt-oss-120b".to_string(),
                max_tokens: 8192,
                temperature: None,
                top_p: None,
                thinking_budget: None,
                reasoning_effort: None,
            },
        }
    }
}
```

原 `LlmConfig` 中的 `api_key` 和 `base_url` 迁移到 `ProviderConfig`。`LlmConfig` 仅保留 `ModelConfig`（即 `provider/mod.rs:51` 中的结构体）的 flatten 嵌入。

#### 9.4 `AgentSpec`（改造后）

见第 3.3 节。

#### 9.5 不变的结构体

以下结构体本次不做改动，保持现有定义：

- `SearchConfig`、`ToolConfig`、`BashConfig`
- `GatewayConfig`、`TrimmerConfigToml`、`SideChannelConfigToml`
- `VoiceConfig`

## 测试案例

- 正常路径：新结构 `config.toml` 可被 `nova-agent` 与 `deskapp` 正确分别加载。
- 正常路径：`ProviderConfig` 和 `LlmConfig` 分别反序列化为预期值。
- 正常路径：`prompt_file = "agent-nova.md"` 能正确读取文件并填充 prompt。
- 正常路径：`prompt_file` 和 `prompt_inline` 均为空时，自动推导 `agent-{id}.md`。
- 边界条件：`prompt_file` 与 `prompt_inline` 同时存在时返回明确错误。
- 边界条件：相对路径与绝对路径均能解析到正确位置。
- 边界条件：`gateway.port` 缺失时使用默认值 `18801`。
- 边界条件：`config_path` 字段为相对路径和绝对路径时，`config_path()` 方法返回正确结果。
- 异常场景：保留旧的 `system_prompt_template = "agent-nova.md"` 时，系统给出兼容告警而不是静默错误。
- 异常场景：配置中仍出现 `[gateway.router]` 等未实现区块时，启动阶段输出一次性 warning。
- 兼容场景：deskapp 的 `TomlConfig` 反序列化包含 `[provider]`、`[llm]` 等新区块的 TOML 文件时不报错（serde 忽略未知字段）。
