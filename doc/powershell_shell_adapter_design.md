# Shell Adapter 设计文档：Windows PowerShell 平替方案

## 1. 背景与动机

当前 `BashTool`（`src/tool/builtin/bash.rs`）在 Windows 上通过 `cmd.exe /C` 执行命令。`cmd.exe` 存在以下局限性：

| 问题 | 说明 |
|------|------|
| 功能受限 | 不支持管道对象、JSON 解析、正则表达式等现代特性 |
| 编码问题 | 中文环境下 stdout 默认 GBK，容易乱码 |
| 脚本能力弱 | 缺少变量类型、错误处理机制、模块系统 |
| 与 LLM 协作差 | 大模型生成的命令往往是 bash/powershell 语法，cmd.exe 兼容性差 |

PowerShell（pwsh / powershell.exe）是 Windows 上更合适的 shell 选择：
- PowerShell 7+ (pwsh) 跨平台，输出默认 UTF-8
- 原生支持 JSON (`ConvertTo-Json`, `ConvertFrom-Json`)
- 与 Windows 系统管理深度集成
- LLM 更容易生成正确的 PowerShell 命令

## 2. 设计目标

1. **不破坏现有接口**：Tool trait 不变，对 LLM 和上层透明
2. **运行时自动选择**：根据平台和可用 shell 自动选择最佳执行器
3. **支持用户覆盖**：通过环境变量或配置允许用户强制指定 shell
4. **保持简洁**：最小改动，避免过度抽象

## 3. 架构设计

### 3.1 核心思路

将 `BashTool` 中硬编码的 shell 选择逻辑抽取为 `ShellBackend` trait，按平台提供不同实现。

```
┌─────────────┐
│  BashTool    │  ← Tool trait 实现不变，名称保持 "bash"
│  (execute)   │
└──────┬──────┘
       │ 委托
       ▼
┌─────────────────┐
│  ShellBackend   │  ← 新增 trait
│  + spawn()      │
└──────┬──────────┘
       │
  ┌────┴─────────────┬──────────────────┐
  ▼                  ▼                  ▼
┌──────┐     ┌────────────┐    ┌──────────────┐
│  Sh  │     │ PowerShell │    │  Cmd (降级)   │
│ Unix │     │  Windows   │    │  Windows     │
└──────┘     └────────────┘    └──────────────┘
```

### 3.2 ShellBackend trait

```rust
/// Shell 执行后端
trait ShellBackend: Send + Sync {
    /// 返回 shell 名称（用于日志/调试）
    fn name(&self) -> &str;

    /// 构建 Command，将 command_str 作为参数传入
    fn build_command(&self, command_str: &str) -> tokio::process::Command;
}
```

### 3.3 各后端实现

#### UnixSh

```rust
struct UnixSh;

impl ShellBackend for UnixSh {
    fn name(&self) -> &str { "sh" }

    fn build_command(&self, command_str: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", command_str]);
        cmd
    }
}
```

#### PowerShellBackend

```rust
struct PowerShellBackend {
    executable: String, // "pwsh" 或 "powershell"
}

impl PowerShellBackend {
    fn detect() -> Option<Self> {
        // 优先检测 pwsh (PowerShell 7+, 跨平台, UTF-8)
        if which("pwsh").is_ok() {
            return Some(Self { executable: "pwsh".into() });
        }
        // 降级到 Windows PowerShell 5.x
        if cfg!(windows) && which("powershell").is_ok() {
            return Some(Self { executable: "powershell".into() });
        }
        None
    }
}

impl ShellBackend for PowerShellBackend {
    fn name(&self) -> &str { &self.executable }

    fn build_command(&self, command_str: &str) -> Command {
        let mut cmd = Command::new(&self.executable);
        cmd.args([
            "-NoProfile",        // 跳过配置文件加载，加速启动
            "-NonInteractive",   // 非交互模式
            "-Command",          // 执行命令字符串
            command_str,
        ]);
        // 强制 UTF-8 输出
        cmd.env("PYTHONIOENCODING", "utf-8");
        if self.executable == "powershell" {
            // Windows PowerShell 5.x 需要额外设置编码
            let wrapped = format!(
                "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; {}",
                command_str
            );
            cmd = Command::new(&self.executable);
            cmd.args(["-NoProfile", "-NonInteractive", "-Command", &wrapped]);
        }
        cmd
    }
}
```

#### CmdBackend（降级方案）

```rust
struct CmdBackend;

impl ShellBackend for CmdBackend {
    fn name(&self) -> &str { "cmd" }

    fn build_command(&self, command_str: &str) -> Command {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", command_str]);
        cmd
    }
}
```

### 3.4 Shell 选择逻辑

```rust
fn select_shell() -> Box<dyn ShellBackend> {
    // 1. 环境变量覆盖
    if let Ok(shell) = std::env::var("ZERO_NOVA_SHELL") {
        match shell.to_lowercase().as_str() {
            "sh" | "bash" => return Box::new(UnixSh),
            "pwsh" | "powershell" => {
                if let Some(ps) = PowerShellBackend::detect() {
                    return Box::new(ps);
                }
            }
            "cmd" => return Box::new(CmdBackend),
            _ => {} // 忽略无效值，走自动检测
        }
    }

    // 2. 平台自动检测
    if cfg!(windows) {
        // Windows: pwsh > powershell > cmd
        if let Some(ps) = PowerShellBackend::detect() {
            return Box::new(ps);
        }
        Box::new(CmdBackend)
    } else {
        // Unix: sh
        Box::new(UnixSh)
    }
}
```

### 3.5 修改后的 BashTool

```rust
pub struct BashTool {
    shell: Box<dyn ShellBackend>,
}

impl BashTool {
    pub fn new() -> Self {
        Self { shell: select_shell() }
    }
}

#[async_trait]
impl Tool for BashTool {
    fn definition(&self) -> ToolDefinition {
        // 不变：名称仍然是 "bash"，保持 LLM 兼容
        ToolDefinition {
            name: "bash".to_string(),
            description: format!(
                "Execute a shell command (using {}). Returns stdout, stderr and exit code.",
                self.shell.name()
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The shell command to execute" },
                    "timeout_ms": { "type": "integer", "description": "Timeout in milliseconds (default 30000)" }
                },
                "required": ["command"]
            }),
        }
    }

    async fn execute(&self, input: Value) -> Result<ToolOutput> {
        let command_str = input["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' field"))?;
        let timeout_ms = input["timeout_ms"].as_u64().unwrap_or(30000);

        let mut cmd = self.shell.build_command(command_str);
        let fut = cmd.output();

        match timeout(Duration::from_millis(timeout_ms), fut).await {
            // ... 与现有逻辑一致
        }
    }
}
```

## 4. 注册方式变更

```rust
// mod.rs
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    registry.register(Box::new(bash::BashTool::new())); // 原来是 BashTool (unit struct)
    // ... 其余不变
}
```

## 5. 文件改动清单

| 文件 | 改动 |
|------|------|
| `src/tool/builtin/bash.rs` | 主要改动：新增 ShellBackend trait 和三个实现，BashTool 改为持有 shell 字段 |
| `src/tool/builtin/mod.rs` | 微调：`BashTool` → `BashTool::new()` |
| `Cargo.toml` | 新增依赖：`which = "7"` （用于检测可执行文件是否存在） |

不需要新建文件，所有改动集中在 `bash.rs` 内部。

## 6. 对其他工具的影响

| 工具 | 是否需要改动 | 说明 |
|------|-------------|------|
| `file_ops.rs` | 否 | 使用 `tokio::fs`，与 shell 无关 |
| `web_fetch.rs` | 否 | 使用 `reqwest`，与 shell 无关 |
| `web_search.rs` | 否 | 使用 `reqwest`，与 shell 无关 |

## 7. 环境变量说明

| 变量名 | 值 | 说明 |
|--------|-----|------|
| `ZERO_NOVA_SHELL` | `pwsh` / `powershell` / `cmd` / `sh` / `bash` | 强制指定 shell 后端，不设置则自动检测 |

## 8. 测试策略

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_shell_selection_default() {
        let shell = select_shell();
        if cfg!(windows) {
            // 应优先选择 pwsh 或 powershell
            assert!(["pwsh", "powershell", "cmd"].contains(&shell.name()));
        } else {
            assert_eq!(shell.name(), "sh");
        }
    }

    #[tokio::test]
    async fn test_powershell_echo() {
        if let Some(ps) = PowerShellBackend::detect() {
            let mut cmd = ps.build_command("Write-Output 'hello'");
            let output = cmd.output().await.unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.contains("hello"));
        }
    }

    #[tokio::test]
    async fn test_utf8_chinese_output() {
        if let Some(ps) = PowerShellBackend::detect() {
            let mut cmd = ps.build_command("Write-Output '你好世界'");
            let output = cmd.output().await.unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.contains("你好世界"));
        }
    }
}
```

## 9. 后续扩展点

- **Shell 信息传递给 LLM**：在 system prompt 中告知当前使用的 shell 类型，让 LLM 生成更准确的命令
- **Shell 语法校验**：在发送命令前做基本语法检查
- **多 shell 支持**：未来可扩展 zsh、fish 等后端，trait 设计已预留扩展能力
