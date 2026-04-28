# .nova 配置说明

## 配置消费边界

- `crates/nova-agent/src/config.rs`：负责 `config.toml` 的 load/migrate/env override/validate。
- `crates/nova-server/src/bin/nova_gateway_ws.rs`：读取 `OriginAppConfig`，再组装运行时 `AppConfig`。
- `deskapp/src-tauri/src/config.rs`：只消费 `[remote]`、`[gateway].port`、`[sidecar]`，其他区块会被忽略。

## 最小可运行配置

```toml
[provider]
# api_key = ""
base_url = "http://127.0.0.1:8082/v1"

[llm]
model = "gpt-oss-120b"
max_tokens = 8192

[search]
backend = "tavily"
# tavily_api_key = ""

[gateway]
host = "127.0.0.1"
port = 18801

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "默认助手"
prompt_file = "agent-nova.md"

[remote]
host = "127.0.0.1"
port = 18801

[sidecar]
mode = "auto"
name = "Built-in Gateway"
command = "nova_gateway_ws"
```

## 为什么拆分 `provider` 与 `llm`

- `provider` 仅描述连接信息（`api_key`、`base_url`）。
- `llm` 仅描述模型参数（`model`、`max_tokens` 等）。
- 这样可以避免“切模型时误改连接配置”，并让迁移与覆盖规则更清晰。

## Agent Prompt 两种方式

- `prompt_file`：引用 prompts 目录中的模板文件，适合长期维护。
- `prompt_inline`：内联短 prompt，适合快速实验。
- 两者互斥，同时设置会在校验阶段报错。

## `remote` 与 `sidecar` 的区别

- `remote`：DeskApp 连接哪个网关地址。
- `sidecar`：DeskApp 是否自动拉起本地网关进程，以及如何拼接启动参数。

## 环境变量覆盖

```powershell
$env:NOVA_API_KEY = "sk-xxx"
$env:TAVILY_API_KEY = "tvly-xxx"
```

- `NOVA_API_KEY` 会覆盖 `provider.api_key`。
- `TAVILY_API_KEY` 会覆盖 `search.tavily_api_key`。

## 旧字段迁移表

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

- 标记为“自动映射”的字段当前版本仍可用，但启动会输出 warning。
- 计划在下一个 major 版本标记 deprecated，再下一个 major 版本移除兼容映射。