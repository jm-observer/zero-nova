# OpenAI 环境变量读取调用链说明

本文档描述了 **`rusty-claude-cli`** 从 CLI 入口到最终读取 `OPENAI_BASE_URL` 环境变量的完整调用路径。文档使用中文并提供关键文件、函数、行号以及调用顺序，便于快速定位实现细节。

---

## 1️⃣ 调用顺序概览

```
rusty-claude-cli/src/main.rs
│   parse_args → 构造 CliAction::Prompt（行 422‑508）
│   └─ LiveCli::new(model, …)（行 540‑560）
│       └─ ApiProviderClient::from_model(model)（api/src/client.rs，行 16‑47）
│           ├─ detect_provider_kind → ProviderKind::OpenAi（api/src/providers/mod.rs，行 31‑36）
│           └─ OpenAiCompatClient::from_env(config)（同上文件，行 31‑45）
│               └─ OpenAiCompatClient::new(api_key, config)（api/src/providers/openai_compat.rs，行 107‑119）
│                   └─ read_base_url(config)（行 106‑110）
│                       └─ std::env::var("OPENAI_BASE_URL")
│                           └─ 若不存在则返回默认值 "https://api.openai.com/v1"
```

> **关键点**：只有 `read_base_url` 读取了 `OPENAI_BASE_URL`，其它路径（Anthropic、xAI）则读取各自对应的环境变量。

---

## 2️⃣ 详细代码片段与行号

| 步骤 | 文件路径 | 函数 | 行号范围 | 关键代码 |
|------|----------|------|----------|----------|
| 1. CLI 参数解析 | `rusty-claude-cli/src/main.rs` | `parse_args` → `CliAction::Prompt` | 422‑508 | ```rust
let prompt = args[index..].join(" ");
return Ok(CliAction::Prompt { prompt, model: resolve_model_alias_with_config(&model), … });
``` |
| 2. 创建 LiveCli 实例 | `rusty-claude-cli/src/main.rs` | `LiveCli::new` | 540‑560 | ```rust
let mut cli = LiveCli::new(model, true, allowed_tools, permission_mode)?;
``` |
| 3. 生成 ProviderClient | `api/src/client.rs` | `ProviderClient::from_model` | 16‑47 | ```rust
match providers::detect_provider_kind(&resolved_model) {
    ProviderKind::OpenAi => {
        let config = if providers::metadata_for_model(&resolved_model).map_or(false, |meta| meta.auth_env == "DASHSCOPE_API_KEY") {
            OpenAiCompatConfig::dashscope()
        } else { OpenAiCompatConfig::openai() };
        Ok(Self::OpenAi(OpenAiCompatClient::from_env(config)?))
    }
    // …其他分支
}
``` |
| 4. 读取 API Key 并初始化客户端 | `api/src/providers/openai_compat.rs` | `OpenAiCompatClient::from_env` → `OpenAiCompatClient::new` | 30‑45 / 107‑119 | ```rust
let Some(api_key) = read_env_non_empty(config.api_key_env)? else { return Err(...); };
Ok(Self::new(api_key, config))
```
```rust
pub fn new(api_key: impl Into<String>, config: OpenAiCompatConfig) -> Self {
    Self {
        http: build_http_client_or_default(),
        api_key: api_key.into(),
        config,
        base_url: read_base_url(config), // ← 关键调用
        max_retries: DEFAULT_MAX_RETRIES,
        …
    }
}
``` |
| 5. 读取 `OPENAI_BASE_URL` 环境变量 | `api/src/providers/openai_compat.rs` | `read_base_url` | 106‑110 | ```rust
pub fn read_base_url(config: OpenAiCompatConfig) -> String {
    std::env::var(config.base_url_env)
        .unwrap_or_else(|_| config.default_base_url.to_string())
}
```
| 6. 配置常量 | `api/src/providers/openai_compat.rs` | `OpenAiCompatConfig::openai` | 57‑58 | ```rust
pub fn openai() -> Self {
    Self {
        provider_name: "OpenAI",
        api_key_env: "OPENAI_API_KEY",
        base_url_env: "OPENAI_BASE_URL",
        default_base_url: DEFAULT_OPENAI_BASE_URL,
        …
    }
}
``` |
| 7. 默认 URL 常量 | `api/src/providers/openai_compat.rs` | `DEFAULT_OPENAI_BASE_URL` | 20 | `pub const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";` |

---

## 3️⃣ Anthropic 与 xAI 的对应路径（供参考）

| Provider | 检测分支 | 初始化函数 | 读取环境变量函数 | 环境变量名称 | 默认 URL |
|----------|----------|------------|----------------|------------|----------|
| Anthropic | `ProviderKind::Anthropic`（`api/src/providers/mod.rs` 第 31‑36 行） | `AnthropicClient::from_env` → `AnthropicClient::new`（`api/src/providers/anthropic_compat.rs`） | `read_base_url`（同文件） | `ANTHROPIC_BASE_URL` | `https://api.anthropic.com/v1` |
| xAI | `ProviderKind::Xai`（`api/src/providers/mod.rs` 第 31‑36 行） | `XaiClient::from_env` → `XaiClient::new`（`api/src/providers/xai_compat.rs`） | `read_base_url`（同文件） | `XAI_BASE_URL` | `https://api.x.ai/v1` |

---

## 4️⃣ 常用调试技巧

- **日志打印**：在 `OpenAiCompatClient::new` 中已经有 `info!` 语句。确保环境变量 `RUST_LOG=info` 生效，即可在运行时看到实际使用的 `base_url`。
- **手动覆盖**：在运行 CLI 前设置环境变量，例如：
  ```bash
  export OPENAI_BASE_URL="https://my-proxy.example.com/v1"
  ./claw prompt "hello" --model gpt-4o-mini
  ```
  观察日志确认使用了自定义 URL。

---

## 5️⃣ 结论

`OPENAI_BASE_URL` 的读取流程全部位于 **`api/src/providers/openai_compat.rs::read_base_url`**，而该函数是由 `OpenAiCompatClient::new`（在 `from_model` 分支中）间接调用的。了解上述调用链可帮助快速定位与调试 OpenAI 客户端的 URL 配置。

---

## 6️⃣ MCP 配置指南

以下是为 `rusty-claude-cli` 添加 MCP（Model‑Context‑Protocol）服务器的完整配置方法。只需在项目根目录的 `.claw/settings.json`（或用户层 `$HOME/.claw/settings.json`）中加入 `mcpServers` 键即可。详细的 JSON 示例已在本文档后提供，直接复制即可使用。

### 6.1 关键结构

> **重要提示**：当前实现仅支持 `stdio` 类型的 MCP 服务器。`http`、`sse`、`ws`、`sdk`、`claudeai‑proxy` 等传输方式在 `McpServerManager` 中尚未实现，仅在配置解析阶段被接受，但在运行时会被标记为 *unsupported*，导致 "server not found" 错误。若需使用 HTTP，请改用 `stdio`（如 `command = "python3"`、`args = ["path/to/mcp_server.py"]`）或自行实现相应的 HTTP 处理层。

- **根键**：`"mcpServers"` → 对象，键为自定义服务器名称。
- **支持的 `type`**：`stdio`, `http`, `sse`, `ws`, `sdk`, `claudeai-proxy`（即 `managedProxy`）。
- **必填字段**：
  - `stdio` → `command`
  - `http`/`sse`/`ws` → `url`
  - `sdk` → `name`
  - `claudeai-proxy` → `url`, `id`
- **可选字段**：`args`, `env`, `toolCallTimeoutMs`, `headers`, `headersHelper`, `oauth` 等。

### 6.2 完整示例

```json
{
  "mcpServers": {
    "local-demo": {
      "type": "stdio",
      "command": "./target/debug/demo-mcp",
      "args": [],
      "env": { "RUST_LOG": "info" },
      "toolCallTimeoutMs": 8000
    },
    "claude-desktop": {
      "type": "http",
      "url": "http://127.0.0.1:8080/mcp",
      "headers": {
        "Authorization": "Bearer YOUR_TOKEN_HERE"
      },
      "oauth": {
        "clientId": "my-client-id",
        "callbackPort": 4567,
        "authServerMetadataUrl": "https://auth.example.com/.well-known/openid-configuration",
        "xaa": true
      }
    },
    "example-sdk": {
      "type": "sdk",
      "name": "example"
    }
  }
}
```

### 6.3 常用 CLI

- `claw mcp list` 查看已注册服务器
- `claw mcp list-resources --server local-demo` 列出服务器工具
- `claw mcp tool mcp.echo '{"message":"hi"}'` 调用工具

### 6.4 调试要点/

| 症状 | 排查 |
|---|---|
| “unknown MCP server” | 检查 `settings.json` 中的键名是否正确 |
| 进程启动失败 | 手动运行 `command` 检查错误 |
| 远程连接失败 | `curl <url>` 检查可达性 |
| OAuth 不生效 | 确认 `clientId`、`callbackPort`、网络通路 |

只要把上述 JSON 加入 `.claw/settings.json`，Claw 启动时会自动创建 `McpServerManager` 并在后续 `claw mcp …` 命令中使用。祝使用愉快！

---

---

*此文档已保存在 `claw-code-doc/openai_base_url_flow.md`，供开发者快速查阅。*
