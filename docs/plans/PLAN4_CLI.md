# Plan 4: 测试二进制 nova-cli

## 目标

提供可交互的 CLI 工具，手动验证 zero-nova library 的所有功能。

## 前置

Plan 2 完成（基础工具可用）。Plan 3 完成后可补充 MCP 功能。

## 范围

| # | 文件 | 内容 |
|---|------|------|
| 1 | `src/bin/nova_cli.rs` | CLI 入口，所有逻辑集中单文件 |
| 2 | `Cargo.toml` 补充 | `[[bin]]` + `cli` feature |

## Cargo 配置

```toml
[[bin]]
name = "nova-cli"
path = "src/bin/nova_cli.rs"
required-features = ["cli"]

[features]
cli = ["builtin-tools", "mcp", "dep:clap", "dep:rustyline"]

[dependencies]
clap = { version = "4", features = ["derive"], optional = true }
rustyline = { version = "15", optional = true }
```

## 命令设计

```
nova-cli [OPTIONS] <COMMAND>

Options:
  --model <MODEL>       模型名称 [default: claude-sonnet-4-20250514]
  --base-url <URL>      自定义 API base URL
  --verbose             打印完整的 tool input/output

Commands:
  chat                  交互式对话（REPL 模式）
  run <PROMPT>          单轮执行（one-shot 模式）
  tools                 列出当前已注册的全部工具
  mcp-test <CMD>        测试 MCP server 连接与工具发现
```

### chat — REPL 交互

交互式多轮对话。启动后显示已加载的工具列表，进入 REPL 循环。

```
$ nova-cli chat
[nova] tools loaded: web_search, web_fetch, bash, read_file, write_file
[nova] type /help for commands, /quit to exit

you> 帮我搜索 Rust 2026 年的新特性
[tool: web_search] query="Rust programming language 2026 new features"
[tool: web_fetch] url="https://blog.rust-lang.org/..."

Rust 2026 edition 的主要新特性包括：
1. ...
2. ...

[tokens: input=1234, output=567]

you> 把上面的内容保存到 /tmp/rust2026.md
[tool: write_file] path="/tmp/rust2026.md"
已写入 /tmp/rust2026.md（1.2 KB）

you> /quit
```

**内置 REPL 斜杠命令**：

| 命令 | 作用 |
|------|------|
| `/quit` | 退出 |
| `/help` | 显示可用命令 |
| `/tools` | 列出当前已注册的所有工具 |
| `/clear` | 清空对话历史 |
| `/history` | 打印当前历史消息数量和 token 估算 |
| `/mcp add <cmd> [args...]` | 运行时连接一个 MCP server 并注入工具 |
| `/mcp list` | 列出已连接的 MCP server 及其工具 |
| `/mcp remove <name>` | 断开一个 MCP server 并移除其工具 |
| `/prompt` | 打印当前完整的 system prompt |

### run — One-shot 执行

单轮对话，执行完毕后退出。适用于脚本集成和自动化测试。

```
$ nova-cli run "搜索今天的科技新闻，整理成 5 条摘要"
[tool: web_search] ...
[tool: web_fetch] ...

1. Apple 发布...
2. ...

$ echo $?
0
```

退出码：0 成功，1 运行时错误，2 参数错误。

### tools — 工具清单

```
$ nova-cli tools
Built-in tools:
  web_search    Search the web using a search engine
  web_fetch     Fetch and extract content from a URL
  bash          Execute a shell command
  read_file     Read the contents of a file
  write_file    Write content to a file

MCP tools: (none connected)
```

### mcp-test — MCP 连接测试

连接指定的 MCP server，执行握手 + 工具发现 + 样例调用，输出结果后退出。

```
$ nova-cli mcp-test npx -y @modelcontextprotocol/server-filesystem /tmp
Connecting to MCP server: npx -y @modelcontextprotocol/server-filesystem /tmp
Connected. Server capabilities:
  tools: 4
    - read_file: Read a file from the filesystem
    - write_file: Write content to a file
    - list_directory: List directory contents
    - search_files: Search for files matching a pattern

Testing tool call: list_directory {"path": "/tmp"}
  Result: OK (12 entries)

All checks passed.
```

## 内部实现结构

```rust
// src/bin/nova_cli.rs

use zero_nova::*;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "nova-cli", about = "Zero-Nova agent test CLI")]
struct Cli {
    #[arg(long, default_value = "claude-sonnet-4-20250514")]
    model: String,
    #[arg(long)]
    base_url: Option<String>,
    #[arg(long)]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Interactive conversation (REPL)
    Chat,
    /// One-shot execution
    Run {
        /// The prompt to execute
        prompt: String,
    },
    /// List registered tools
    Tools,
    /// Test MCP server connection
    McpTest {
        /// Command and args to start the MCP server
        #[arg(trailing_var_arg = true)]
        cmd: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let client = make_client(&cli)?;
    let mut tools = ToolRegistry::new();
    register_builtin_tools(&mut tools);

    let prompt = SystemPromptBuilder::personal_assistant()
        .with_tools(&tools)
        .environment("date", current_date())
        .environment("platform", std::env::consts::OS)
        .build();

    let config = AgentConfig {
        model_config: ModelConfig { model: cli.model.clone(), ..Default::default() },
        ..Default::default()
    };
    let mut agent = AgentRuntime::new(client, tools, prompt, config);

    match cli.command {
        Command::Chat => run_repl(&mut agent, cli.verbose).await,
        Command::Run { prompt } => run_oneshot(&agent, &prompt, cli.verbose).await,
        Command::Tools => { print_tools(&agent); Ok(()) },
        Command::McpTest { cmd } => test_mcp(&cmd).await,
    }
}

// ---- 各子命令实现为独立函数 ----

async fn run_repl(agent: &mut AgentRuntime<impl LlmClient>, verbose: bool) -> Result<()> {
    // rustyline REPL 循环
    // 维护 history: Vec<Message>
    // 解析 /slash 命令
    // 对普通输入调用 agent.run_turn()
    // 渲染 AgentEvent 到 stdout
}

async fn run_oneshot(agent: &AgentRuntime<impl LlmClient>, prompt: &str, verbose: bool) -> Result<()> {
    // 空 history，单轮 run_turn
    // 渲染输出后退出
}

fn print_tools(agent: &AgentRuntime<impl LlmClient>) {
    // 遍历 agent.tools.definitions() 输出列表
}

async fn test_mcp(cmd: &[String]) -> Result<()> {
    // McpClient::connect_stdio()
    // list_tools()
    // 尝试调用第一个工具
    // 输出结果
}
```

## 流式事件渲染

`AgentEvent` → stdout 的渲染逻辑：

```rust
fn render_event(event: &AgentEvent, verbose: bool) {
    match event {
        AgentEvent::TextDelta(text) => {
            print!("{text}");
            // 不换行，增量输出
        }
        AgentEvent::ToolStart { name, input, .. } => {
            if verbose {
                println!("\n[tool: {name}] {input}");
            } else {
                println!("\n[tool: {name}]");
            }
        }
        AgentEvent::ToolEnd { name, output, is_error, .. } => {
            if verbose {
                let status = if *is_error { "ERROR" } else { "OK" };
                println!("[tool: {name}] {status}: {output}");
            }
        }
        AgentEvent::TurnComplete { usage, .. } => {
            println!("\n[tokens: input={}, output={}]",
                usage.input_tokens, usage.output_tokens);
        }
        AgentEvent::Error(e) => {
            eprintln!("[error] {e}");
        }
    }
}
```

## 验证矩阵

| 场景 | 命令 | 验证点 |
|------|------|--------|
| 纯对话 | `run "你好"` | LLM 调用 + 流式输出正常 |
| Web 搜索 | `run "搜索 Rust async"` | web_search 工具被调用，返回结果 |
| 网页抓取 | `run "总结 https://example.com 的内容"` | web_fetch 工具被调用 |
| 命令执行 | `run "列出当前目录的文件"` | bash 工具执行 `ls` |
| 文件读取 | `run "读取 /tmp/test.txt"` | read_file 工具调用 |
| 文件写入 | `run "把 hello 写入 /tmp/out.txt"` | write_file 工具调用，文件实际写入 |
| 多轮工具链 | `chat` 中连续指令 | 多轮 history 正确传递，工具链式调用 |
| MCP 连接 | `mcp-test npx ... server-filesystem` | stdio 子进程启动、工具发现、工具调用 |
| MCP 动态注入 | `chat` 中 `/mcp add ...` | 运行时工具集扩展，LLM 能使用新工具 |
| 多工具并行 | `run "搜索 X 然后保存到文件"` | 多个工具在一轮中被依次调用 |
| 错误恢复 | `run "读取 /nonexistent"` | 工具返回 is_error=true，LLM 能处理错误 |

## 交付物

`cargo run --bin nova-cli --features cli -- chat` 可用，全部验证场景通过。
