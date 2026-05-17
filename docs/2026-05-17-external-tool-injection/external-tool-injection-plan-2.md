# Plan 2: ExternalCommandTool 实现

## 前置依赖

Plan 1（`ExternalToolDefinition` 和 `CommandExecution` 结构已定义）

## 任务目标

实现 `ExternalCommandTool`，满足 `Tool` trait，能根据 LLM 传入的 JSON input 组装命令行参数并执行外部进程。

## 执行范围

- **必须新增**：`crates/nova-agent/src/tool/external/executor.rs`
- **允许修改**：`crates/nova-agent/src/tool/external/mod.rs`（导出 executor）
- **禁止修改**：`registry.rs`、builtin/

## Agent 执行步骤

### 步骤 1：实现 ExternalCommandTool

新增 `crates/nova-agent/src/tool/external/executor.rs`：

```rust
use crate::tool::{RegisteredToolDefinition, Tool, ToolContext, ToolOutput};
use super::schema::{CommandExecution, ExternalToolDefinition, ParamMapping};
use anyhow::Result;
use serde_json::Value;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_OUTPUT_BYTES: usize = 100_000;

pub struct ExternalCommandTool {
    name: String,
    description: String,
    input_schema: Value,
    execution: CommandExecution,
}

impl ExternalCommandTool {
    pub fn from_definition(def: ExternalToolDefinition) -> Self {
        Self {
            name: def.name,
            description: def.description,
            input_schema: def.input_schema,
            execution: def.execution,
        }
    }

    fn build_args(&self, input: &Value) -> Vec<String> {
        let mut args: Vec<String> = self.execution.subcommands.clone();
        let obj = input.as_object();
        for mapping in &self.execution.param_mappings {
            let Some(val) = obj.and_then(|o| o.get(&mapping.name)) else {
                continue;
            };
            match val {
                Value::Bool(true) => {
                    args.push(mapping.arg.clone());
                }
                Value::Bool(false) => {}
                Value::String(s) => {
                    args.push(mapping.arg.clone());
                    args.push(s.clone());
                }
                Value::Number(n) => {
                    args.push(mapping.arg.clone());
                    args.push(n.to_string());
                }
                _ => {}
            }
        }
        args
    }
}

#[async_trait::async_trait]
impl Tool for ExternalCommandTool {
    fn definition(&self) -> RegisteredToolDefinition {
        RegisteredToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
            defer_loading: true,
        }
    }

    async fn execute(&self, input: Value, _context: Option<ToolContext>) -> Result<ToolOutput> {
        let args = self.build_args(&input);

        let mut cmd = Command::new(&self.execution.command);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = tokio::time::timeout(DEFAULT_TIMEOUT, cmd.output())
            .await
            .map_err(|_| anyhow::anyhow!(
                "tool '{}' timed out after {}s",
                self.name,
                DEFAULT_TIMEOUT.as_secs()
            ))?
            .map_err(|e| anyhow::anyhow!(
                "failed to execute tool '{}': {}",
                self.name,
                e
            ))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let content = if output.status.success() {
            truncate_output(&stdout, MAX_OUTPUT_BYTES)
        } else {
            let mut msg = format!("Exit code: {}\n", output.status.code().unwrap_or(-1));
            if !stderr.is_empty() {
                msg.push_str(&truncate_output(&stderr, MAX_OUTPUT_BYTES));
            } else {
                msg.push_str(&truncate_output(&stdout, MAX_OUTPUT_BYTES));
            }
            msg
        };

        Ok(ToolOutput {
            content,
            is_error: !output.status.success(),
        })
    }
}

fn truncate_output(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        let truncated = &s[..max_bytes];
        format!("{}\n\n... (output truncated, {} bytes total)", truncated, s.len())
    }
}
```

### 步骤 2：导出 executor 模块

修改 `crates/nova-agent/src/tool/external/mod.rs`，新增：

```rust
pub mod executor;
```

## 目标数据结构

| 结构 | 用途 |
|------|------|
| `ExternalCommandTool` | 实现 `Tool` trait 的外部命令执行器 |

## 行为规则

| 输入 | 处理 | 输出 |
|------|------|------|
| `{"package": "nova-agent", "release": true}` | `cargo build --package nova-agent --release` | stdout 内容 |
| `{"url": "...", "days": 7}` | `github-commit-info --url ... --days 7` | stdout 内容 |
| 命令超时（>30s） | kill 进程 | `is_error: true`, 超时提示 |
| 命令退出码非 0 | 收集 stderr | `is_error: true`, stderr 内容 |
| stdout 超过 100KB | 截断 | 截断提示 + 部分内容 |
| bool 参数为 false | 不传对应 arg | 无该 flag |

## 禁止事项

- 禁止使用 `std::process::Command`（必须用 tokio 异步版本）
- 禁止将环境变量全量传递（后续迭代做白名单，本次先使用默认继承）
- 禁止在执行失败时 panic

## 测试要求

- 测试文件：`crates/nova-agent/src/tool/external/executor.rs` 内 `#[cfg(test)] mod tests`
- 测试名：`test_build_args_string`、`test_build_args_bool`、`test_build_args_mixed`
- 输入：构造 `ExternalCommandTool` 实例，验证 `build_args` 输出
- 验证命令：`cargo test -p nova-agent test_build_args`

## 完成条件

- [ ] `ExternalCommandTool` 实现 `Tool` trait
- [ ] `build_args` 正确映射 string/bool/integer 参数
- [ ] 超时保护工作正常
- [ ] 输出截断工作正常
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过
