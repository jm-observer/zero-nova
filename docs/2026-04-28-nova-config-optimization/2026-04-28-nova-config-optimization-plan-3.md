# Plan 3：示例配置、文档与测试收口

## 前置依赖
- Plan 1：配置模型对齐与分层重构
- Plan 2：加载、兼容与校验策略

## 本次目标
- 让仓库中的默认配置、示例文件、README 与实际代码行为保持一致。
- 提供迁移说明，降低已有本地环境升级成本。
- 补足配置相关测试，避免后续再次发生配置漂移。
- 验证 deskapp 在新配置结构下的兼容性。

## 涉及文件
- `.nova/config.toml`
- `.nova/examples/agents.toml`
- `.nova/README.md`
- `crates/nova-agent/src/config.rs`
- `deskapp/src-tauri/src/config.rs`
- `crates/nova-server/src/bin/nova_gateway_ws.rs`

## 详细设计

### 1. 默认配置模板改造

仓库中的 `.nova/config.toml` 应从"开发者本机可运行副本"改为"团队通用模板"，满足以下要求：

- 不包含真实或真实风格的密钥
- 不包含机器私有绝对路径
- 默认值尽可能与代码默认值一致
- 注释只保留当前已实现、已生效的字段

建议模板化示例：

```toml
# ── 供应商接入 ──────────────────────────────────────────────────
# api_key 建议通过环境变量 NOVA_API_KEY 设置，避免明文提交到仓库
[provider]
# api_key = ""
base_url = "http://127.0.0.1:8082/v1"

# ── 模型调用参数 ────────────────────────────────────────────────
[llm]
model = "gpt-oss-120b"
max_tokens = 8192
temperature = 0.7

# ── 搜索 ───────────────────────────────────────────────────────
# tavily_api_key 建议通过环境变量 TAVILY_API_KEY 设置
[search]
backend = "tavily"
# tavily_api_key = ""

# ── 工具与能力 ──────────────────────────────────────────────────
[tool]
skills_dir = "skills"
prompts_dir = "prompts"

# ── 网关 ───────────────────────────────────────────────────────
[gateway]
host = "127.0.0.1"
port = 18801
max_iterations = 30
tool_timeout_secs = 3600
subagent_timeout_secs = 300
max_tokens = 4096
use_turn_context = true

[gateway.trimmer]
enabled = true
context_window = 128000
output_reserve = 8192
min_recent_messages = 10

# ── Agent 注册 ──────────────────────────────────────────────────
[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "默认通用助手"
aliases = ["助手"]
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
```

关于密钥占位方式的说明：

- 使用注释掉的 `# api_key = ""` 而非空字符串 `api_key = ""`，避免被误认为"不需要填写"。
- 配合注释说明推荐使用环境变量，引导用户走安全路径。
- 若用户需要在配置文件中写明密钥（如纯本地开发），取消注释并填入即可。

### 2. README 内容重写方向

`.nova/README.md` 需要围绕"配置如何被项目消费"重写，而不只是罗列目录。

建议包含以下章节：

- 配置文件由哪些模块读取（引用 Plan 1 第 8 节的职责边界表）
- `.nova/config.toml` 的最小可运行示例
- 为什么拆分 `provider` 与 `llm`
- Agent Prompt 的两种配置方式：`prompt_file` / `prompt_inline`
- `remote` 与 `sidecar` 的差异
- 环境变量覆盖示例（`NOVA_API_KEY`、`TAVILY_API_KEY`）
- 旧字段迁移表（引用本文档第 5 节）

### 3. 示例文件治理

`.nova/examples/agents.toml` 应与正式配置字段完全一致，禁止继续保留旧语义示例。

建议：

- 用 `prompt_file` 替换 `system_prompt_template`
- 至少覆盖 3 类 Agent 示例：
  - 默认助手
  - 代码助手
  - 工具受限助手
- 对 `tool_whitelist`、`model_config` 提供最小可读案例

### 4. 测试补齐策略

#### 4.1 `crates/nova-agent/src/config.rs` 测试

新增或完善以下测试，每条附带具体断言：

**新配置结构反序列化测试：**

```rust
#[test]
fn new_config_deserializes_correctly() {
    let toml = r#"
        [provider]
        api_key = "test-key"
        base_url = "http://localhost:8082/v1"

        [llm]
        model = "test-model"
        max_tokens = 4096
    "#;
    let config: OriginAppConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.provider.api_key, "test-key");
    assert_eq!(config.llm.model_config.model, "test-model");
}
```

**旧字段兼容映射测试：**

```rust
#[test]
fn legacy_llm_api_key_migrates_to_provider() {
    let toml = r#"
        [llm]
        api_key = "old-key"
        base_url = "http://old-host/v1"
        model = "old-model"
        max_tokens = 2048
    "#;
    let raw: RawAppConfig = toml::from_str(toml).unwrap();
    let (config, warnings) = raw.migrate();
    assert_eq!(config.provider.api_key, "old-key");
    assert_eq!(config.provider.base_url, "http://old-host/v1");
    assert_eq!(config.llm.model_config.model, "old-model");
    assert!(!warnings.is_empty()); // 应产生迁移 warning
}
```

**`prompt_file` / `prompt_inline` 互斥测试：**

```rust
#[test]
fn prompt_file_and_inline_conflict_fails_validation() {
    let toml = r#"
        [[gateway.agents]]
        id = "test"
        display_name = "Test"
        description = "test"
        prompt_file = "test.md"
        prompt_inline = "You are a test agent."
    "#;
    let config: OriginAppConfig = toml::from_str(toml).unwrap();
    assert!(config.validate().is_err());
}
```

**`gateway.trimmer` 新旧字段回归测试：**

```rust
#[test]
fn legacy_trimmer_fields_migrate_correctly() {
    let toml = r#"
        [gateway.trimmer]
        max_history_tokens = 50000
        preserve_recent = 5
    "#;
    let raw: RawAppConfig = toml::from_str(toml).unwrap();
    let (config, warnings) = raw.migrate();
    assert_eq!(config.gateway.trimmer.context_window, 58192); // 50000 + 8192
    assert_eq!(config.gateway.trimmer.output_reserve, 8192);
    assert_eq!(config.gateway.trimmer.min_recent_messages, 5);
    assert!(config.gateway.trimmer.enabled);
    assert!(!warnings.is_empty());
}
```

**默认端口测试：**

```rust
#[test]
fn default_gateway_port_is_18801() {
    let config = GatewayConfig::default();
    assert_eq!(config.port, 18801);
}
```

**`config_path()` 方法修复测试：**

```rust
#[test]
fn config_path_uses_field_when_present() {
    let mut origin = OriginAppConfig::default();
    origin.config_path = Some("custom.toml".to_string());
    let config = AppConfig::from_origin(origin, PathBuf::from("D:/workspace"));
    assert_eq!(config.config_path(), PathBuf::from("D:/workspace/custom.toml"));
}

#[test]
fn config_path_defaults_when_field_absent() {
    let config = AppConfig::from_origin(OriginAppConfig::default(), PathBuf::from("D:/workspace"));
    assert_eq!(config.config_path(), PathBuf::from("D:/workspace/config.toml"));
}
```

**相对路径解析测试：**

```rust
#[test]
fn skills_dir_resolves_relative_to_workspace() {
    let mut origin = OriginAppConfig::default();
    origin.tool.skills_dir = Some("my-skills".to_string());
    let config = AppConfig::from_origin(origin, PathBuf::from("D:/workspace"));
    assert_eq!(config.skills_dir(), PathBuf::from("D:/workspace/my-skills"));
}
```

#### 4.2 `deskapp/src-tauri/src/config.rs` 测试

**deskapp 反序列化兼容性测试（关键）：**

验证 deskapp 的 `TomlConfig` 在 TOML 文件包含 `[provider]`、`[llm]`、`[gateway]` 等新区块时不会报错：

```rust
#[test]
fn deskapp_toml_ignores_unknown_sections() {
    let toml = r#"
        [provider]
        api_key = "test"
        base_url = "http://localhost:8082/v1"

        [llm]
        model = "test-model"
        max_tokens = 4096

        [gateway]
        host = "127.0.0.1"
        port = 18801

        [remote]
        host = "127.0.0.1"
        port = 18801

        [sidecar]
        mode = "auto"
        name = "Test"
        command = "nova_gateway_ws"
    "#;
    let config: TomlConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.remote.port, Some(18801));
    assert_eq!(config.sidecar.name, "Test");
}
```

**`remote` 默认值测试：**

```rust
#[test]
fn remote_defaults_to_localhost_18801() {
    let toml = r#"
        [remote]

        [sidecar]
        name = "Test"
        command = "test"
    "#;
    let config: TomlConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.remote.host, None); // load_config 中用 unwrap_or("localhost")
    assert_eq!(config.remote.port, None); // load_config 中用 unwrap_or(18801)
}
```

**`sidecar.command` 命令名测试：**

```rust
#[test]
fn sidecar_command_accepts_plain_name() {
    let toml = r#"
        [remote]

        [sidecar]
        mode = "auto"
        name = "Test"
        command = "nova_gateway_ws"
    "#;
    let config: TomlConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.sidecar.command, "nova_gateway_ws");
}
```

### 5. 迁移说明

面向已有使用者的迁移表：

| 旧写法 | 新写法 | 兼容状态 | 说明 |
|---|---|---|---|
| `[llm].api_key` | `[provider].api_key` | 自动映射 + warning | 连接信息迁移到 provider |
| `[llm].base_url` | `[provider].base_url` | 自动映射 + warning | 连接信息迁移到 provider |
| `system_prompt_template = "agent-nova.md"` | `prompt_file = "agent-nova.md"` | 自动映射 + warning | 文件语义显式化 |
| `system_prompt_template = "长文本..."` | `prompt_inline = "长文本..."` | 自动映射 + warning | 内联语义显式化 |
| `gateway.trimmer.max_history_tokens` | `gateway.trimmer.context_window` + `output_reserve` | 自动折算 + warning | 新模型与实现对齐 |
| `gateway.trimmer.preserve_recent` | `gateway.trimmer.min_recent_messages` | 自动映射 + warning | 名称与代码一致 |
| `gateway.trimmer.preserve_tool_pairs` | （移除） | warning 提示未实现 | 代码无消费路径 |
| `[gateway.router]` | （移除） | warning 提示未实现 | 代码无对应模型 |
| `[gateway.interaction]` | （移除） | warning 提示未实现 | 代码无对应模型 |
| `[gateway.workflow]` | （移除） | warning 提示未实现 | 代码无对应模型 |
| 绝对路径 `sidecar.command` | 命令名或相对路径 | 仍可使用 | 提升可移植性 |
| 仓库内明文密钥 | 注释占位 + 环境变量 | — | 提升安全性 |

迁移文档应说明：

- 上表中标记"自动映射"的字段在当前版本中仍可使用，启动时会输出一次 warning
- 计划在下一个主要版本中将废弃字段标记为 deprecated
- 计划在再下一个主要版本中移除兼容映射

### 6. 回归验证清单

Plan 3 完成后，需要手动验证以下场景：

- [ ] `cargo run --bin nova_cli -- chat` 使用新配置正常启动
- [ ] `cargo run -p nova-server --bin nova-server-ws` 使用新配置正常启动
- [ ] `pnpm tauri dev`（deskapp）使用新配置正常启动
- [ ] 使用旧格式 config.toml 时三个入口均能兼容启动并输出 warning
- [ ] 环境变量 `NOVA_API_KEY` 覆盖 `provider.api_key` 生效
- [ ] DeskApp auto sidecar 模式正确拼接 `--port` 与 `--workspace` 参数

## 测试案例

- 正常路径：仓库内默认 `.nova/config.toml` 能作为模板被 `OriginAppConfig::load_from_file` 成功解析（密钥字段为空或注释不影响解析）。
- 正常路径：README 示例中的 TOML 片段可被成功反序列化。
- 正常路径：新旧配置均能通过完整的 load → migrate → env_override → validate 流程。
- 边界条件：示例中使用相对路径 `command = "nova_gateway_ws"` 时不触发路径解析异常。
- 边界条件：`sidecar.command` 为命令名时，Windows 与 Linux 目标下均可正常查找（依赖 PATH）。
- 兼容场景：deskapp `TomlConfig` 反序列化包含所有新区块的完整 config.toml 不报错。
- 回归场景：DeskApp auto sidecar 模式仍能正确拼接 `--port` 与 `--workspace` 参数。
- 回归场景：`cargo clippy --workspace -- -D warnings` 通过。
- 回归场景：`cargo test --workspace` 通过。
