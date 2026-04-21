# 长耗时后台任务的实时进度上报与可见性方案设计

| 章节 | 说明 |
|-----------|------|
| 时间 | 2026-04-20 |
| 项目现状 | 1. `BashTool` 当前使用 `cmd.output().await` 同步等待模式（`bash.rs:169`），任务执行期间前端无任何中间状态反馈。<br>2. 自动化测试或 Benchmark 等长耗时任务在执行时，用户无法判断脚本是正在运行还是已经崩溃/被挂起。<br>3. `AgentEvent`（`event.rs:6-30`）仅支持 `ToolStart`/`ToolEnd` 两个工具生命周期事件，缺乏过程中的增量输出能力。<br>4. `Tool` trait 的 `execute` 签名（`tool.rs:24`）为 `async fn execute(&self, input: Value) -> Result<ToolOutput>`，**不接受 `event_tx`**，工具无法向外发送中间事件。 |
| 本次目标 | 1. **状态透明化**：实现 `BashTool` 的流式日志输出，实时反馈脚本运行情况。<br>2. **全链路打通**：从后端脚本到 Gateway 再到前端，建立一套完整的中间状态冒泡机制。<br>3. **Skill 适配**：让技能编写的脚本能够主动上报进度。 |

---

## 设计评审意见

### 原方案核心问题

1. **`Tool` trait 签名缺口**：原方案提到 BashTool 通过 `event_tx` 发送 `LogDelta`，但当前 `Tool::execute()` 签名中**没有** `event_tx` 参数。这是最关键的架构障碍，原文档未涉及。
2. **stderr 处理缺失**：原方案仅提到 stdout 的流式读取，未说明 stderr 的处理策略。stderr 同样需要流式输出（编译错误、脚本报错等关键信息都在 stderr）。
3. **网关层映射不精确**：原方案提到”打包为 `chat.log`”，但实际协议使用 `chat.progress` + `ProgressEvent`。应复用现有 `ChatProgress` 机制而非引入新的顶层消息类型。
4. **限流策略缺乏具体方案**：原文档在风险项中提到日志量过载，但未给出具体的限流方案。
5. **超时场景的日志保留**：原方案未说明超时发生时，已经通过 `LogDelta` 发出的日志如何保留、最终的 `ToolEnd` 结果中是否包含完整日志。

---

## 细化设计

### 第 1 层：`Tool` trait 扩展与事件定义

#### 1.1 `AgentEvent` 新增 `LogDelta` 变体 (`src/event.rs`)

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AgentEvent {
    // ... 现有 7 个变体保持不变 ...

    /// 工具执行过程中的流式输出（如 bash 的 stdout/stderr）
    LogDelta {
        /// 对应 ToolStart 中的 tool_use_id
        id: String,
        /// 工具名称
        name: String,
        /// 日志内容（一行或多行聚合）
        log: String,
        /// 来源流: “stdout” | “stderr”
        stream: String,
    },
}
```

与原方案的差异：增加了 `stream` 字段，区分 stdout/stderr 来源，便于前端用不同样式展示。

#### 1.2 `Tool` trait 签名扩展 (`src/tool.rs`)

这是原方案**未涉及**的核心改动。需要让工具能够在执行过程中发送中间事件。

**方案 A（推荐）：新增可选的上下文参数**

```rust
use tokio::sync::mpsc;

/// 工具执行上下文，传递事件通道等运行时信息
pub struct ToolContext {
    /// 用于发送中间事件的通道（如日志流）
    pub event_tx: mpsc::Sender<crate::event::AgentEvent>,
    /// 当前 tool_use_id，用于关联 LogDelta 事件
    pub tool_use_id: String,
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;

    /// 执行工具。默认实现忽略 context，保持向后兼容。
    async fn execute(&self, input: Value, _context: Option<ToolContext>) -> Result<ToolOutput> {
        // 提供默认实现，调用无 context 的版本——但这需要重构
        // 实际上应直接改签名，所有 Tool 实现都更新
        unimplemented!()
    }
}
```

> **实际落地**：由于项目内 Tool 实现数量有限，建议直接修改签名为 `execute(&self, input: Value, context: Option<ToolContext>)`，所有现有 Tool 的 `context` 参数直接标 `_context` 忽略即可。

#### 1.3 `ToolRegistry::execute` 相应更新 (`src/tool.rs:60-70`)

```rust
pub async fn execute(
    &self,
    name: &str,
    input: serde_json::Value,
    context: Option<ToolContext>,
) -> anyhow::Result<ToolOutput> {
    for tool in &self.tools {
        if tool.definition().name == name {
            return tool.execute(input, context).await;
        }
    }
    Ok(ToolOutput {
        content: format!(“Tool '{}' not found”, name),
        is_error: true,
    })
}
```

#### 1.4 `agent.rs` 工具调用处适配 (`src/agent.rs:220-259`)

在 `agent.rs:234` 处，将 `event_tx` 和 `tool_use_id` 包装为 `ToolContext` 传入：

```rust
// 现有: let result = timeout(tool_timeout_duration, tool_registry.execute(&name, input_val)).await;
// 改为:
let context = ToolContext {
    event_tx: tx.clone(),
    tool_use_id: id.clone(),
};
let result = timeout(
    tool_timeout_duration,
    tool_registry.execute(&name, input_val, Some(context)),
).await;
```

---

### 第 2 层：`BashTool` 异步流化重构

#### 2.1 核心改动 (`src/tool/builtin/bash.rs:162-198`)

将 `cmd.output().await` 替换为 `cmd.spawn()` + 逐行流式读取：

```rust
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};

async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
    let command_str = input[“command”]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!(“Missing 'command' field”))?;
    let timeout_ms = input[“timeout_ms”].as_u64().unwrap_or(30000);

    let mut cmd = self.shell.build_command(command_str);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn()
        .map_err(|e| anyhow::anyhow!(“Failed to spawn command: {}”, e))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();

    // 使用 tokio::select! 同时读取 stdout 和 stderr
    let read_fut = async {
        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        let mut stdout_done = false;
        let mut stderr_done = false;

        while !stdout_done || !stderr_done {
            tokio::select! {
                line = stdout_reader.next_line(), if !stdout_done => {
                    match line {
                        Ok(Some(line)) => {
                            // 发送 LogDelta
                            if let Some(ctx) = &context {
                                let _ = ctx.event_tx.send(AgentEvent::LogDelta {
                                    id: ctx.tool_use_id.clone(),
                                    name: “bash”.to_string(),
                                    log: line.clone(),
                                    stream: “stdout”.to_string(),
                                }).await;
                            }
                            stdout_buf.push_str(&line);
                            stdout_buf.push('\n');
                        }
                        Ok(None) => stdout_done = true,
                        Err(e) => {
                            stderr_buf.push_str(&format!(“Error reading stdout: {}\n”, e));
                            stdout_done = true;
                        }
                    }
                }
                line = stderr_reader.next_line(), if !stderr_done => {
                    match line {
                        Ok(Some(line)) => {
                            if let Some(ctx) = &context {
                                let _ = ctx.event_tx.send(AgentEvent::LogDelta {
                                    id: ctx.tool_use_id.clone(),
                                    name: “bash”.to_string(),
                                    log: line.clone(),
                                    stream: “stderr”.to_string(),
                                }).await;
                            }
                            stderr_buf.push_str(&line);
                            stderr_buf.push('\n');
                        }
                        Ok(None) => stderr_done = true,
                        Err(e) => {
                            stderr_buf.push_str(&format!(“Error reading stderr: {}\n”, e));
                            stderr_done = true;
                        }
                    }
                }
            }
        }

        child.wait().await
    };

    match timeout(Duration::from_millis(timeout_ms), read_fut).await {
        Ok(Ok(status)) => {
            let exit_code = status.code().unwrap_or(-1);
            let content = format!(
                “exit_code: {}\nstdout:\n{}\nstderr:\n{}”,
                exit_code,
                truncate(&stdout_buf, 100_000),
                truncate(&stderr_buf, 10_000)
            );
            Ok(ToolOutput { content, is_error: !status.success() })
        }
        Ok(Err(e)) => Ok(ToolOutput {
            content: format!(“Failed to execute command: {}”, e),
            is_error: true,
        }),
        Err(_) => {
            // 超时：尝试 kill 子进程
            let _ = child.kill().await;
            let content = format!(
                “Command timed out after {}ms\nstdout so far:\n{}\nstderr so far:\n{}”,
                timeout_ms,
                truncate(&stdout_buf, 100_000),
                truncate(&stderr_buf, 10_000)
            );
            Ok(ToolOutput { content, is_error: true })
        }
    }
}
```

关键设计决策：
- **stdout 和 stderr 并行读取**：使用 `tokio::select!` 交错读取，避免一个流阻塞另一个流。
- **最终结果仍保留完整输出**：`ToolEnd` 事件仍包含完整的 stdout/stderr，`LogDelta` 只是过程中的”预览”。
- **超时时保留已有输出**：超时后在 `ToolOutput.content` 中包含已经收集到的部分输出。
- **子进程 kill**：超时后主动调用 `child.kill()` 清理。

#### 2.2 限流策略

在高频输出场景下（如 `cargo build` 的编译日志），逐行发送会产生大量 WebSocket 消息。采用**时间窗口聚合**策略：

```rust
use std::time::Instant;

const LOG_FLUSH_INTERVAL_MS: u128 = 200; // 每 200ms 最多发送一次

// 在读取循环中维护一个聚合缓冲区
let mut pending_log = String::new();
let mut last_flush = Instant::now();

// 每读取一行后：
pending_log.push_str(&line);
pending_log.push('\n');

if last_flush.elapsed().as_millis() >= LOG_FLUSH_INTERVAL_MS {
    if let Some(ctx) = &context {
        let _ = ctx.event_tx.send(AgentEvent::LogDelta {
            id: ctx.tool_use_id.clone(),
            name: “bash”.to_string(),
            log: std::mem::take(&mut pending_log),
            stream: “stdout”.to_string(),
        }).await;
    }
    last_flush = Instant::now();
}

// 循环结束后，flush 剩余内容
if !pending_log.is_empty() {
    // 发送最后一批
}
```

这将把最坏情况下的消息频率限制在 5 条/秒，同时仍保持实时性体验。

---

### 第 3 层：网关层协议转发

#### 3.1 `ProgressEvent` 新增 `log` 字段 (`src/gateway/protocol.rs:248-261`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = “camelCase”)]
pub struct ProgressEvent {
    #[serde(rename = “type”)]
    pub kind: String, // 新增 'tool_log' 类型
    pub session_id: Option<String>,
    pub iteration: Option<i32>,
    pub tool_name: Option<String>,
    pub tool_use_id: Option<String>,
    pub args: Option<Value>,
    pub result: Option<Value>,
    pub is_error: Option<bool>,
    pub thinking: Option<String>,
    pub token: Option<String>,
    pub output: Option<String>,
    // ---- 新增 ----
    /// 日志内容（仅 kind=”tool_log” 时有值）
    #[serde(skip_serializing_if = “Option::is_none”)]
    pub log: Option<String>,
    /// 日志来源流: “stdout” | “stderr”（仅 kind=”tool_log” 时有值）
    #[serde(skip_serializing_if = “Option::is_none”)]
    pub stream: Option<String>,
}
```

**设计理由**：复用现有 `ChatProgress` 消息类型，而非引入新的顶层 `MessageEnvelope` 变体。前端只需在处理 `chat.progress` 时新增对 `kind: “tool_log”` 的分支即可，**不需要修改 WebSocket 消息解析框架**。

#### 3.2 `bridge.rs` 新增转换分支 (`src/gateway/bridge.rs`)

在 `agent_event_to_gateway` 函数的 match 中新增：

```rust
AgentEvent::LogDelta { id, name, log, stream } => MessageEnvelope::ChatProgress(ProgressEvent {
    kind: “tool_log”.to_string(),
    session_id: Some(session_id.to_string()),
    tool_name: Some(name),
    tool_use_id: Some(id),
    log: Some(log),
    stream: Some(stream),
    ..Default::default()
}),
```

#### 3.3 通道背压说明

现有 `mpsc::channel(100)` 缓冲区（`chat.rs:468`）在限流后（每 200ms 最多 1 条 LogDelta）不会成为瓶颈。即使 5 个工具并行执行，每秒也只有 25 条 LogDelta，远低于 100 的缓冲上限。无需修改通道大小。

---

### 第 4 层：前端适配（前端团队负责）

#### 4.1 WebSocket 消息处理

在 `chat.progress` 的处理逻辑中新增：

```typescript
case 'tool_log':
  // 将日志追加到对应 tool_use_id 的日志缓冲区
  appendToolLog(event.toolUseId, event.log, event.stream);
  break;
```

#### 4.2 UI 展示

- **Log Streamer 区域**：在 Tool 执行气泡下方展示一个可折叠的日志区域，默认显示最后 5 行。
- **流区分**：stdout 用默认色，stderr 用红色/橙色。
- **活跃指示器**：收到 `tool_log` 时刷新心跳计时器。若超过 30 秒无任何 `tool_log` 且未收到 `tool_result`，标记为”疑似挂起”。
- **自动滚动**：日志区域自动滚到底部，用户手动向上滚动时暂停自动滚动。

---

### 第 5 层：其他 Tool 实现的适配

所有现有 Tool 实现的 `execute` 签名需增加 `_context: Option<ToolContext>` 参数，但内部不使用：

```rust
// 其他工具（如 ReadTool, WriteTool 等）只需改签名
async fn execute(&self, input: Value, _context: Option<ToolContext>) -> Result<ToolOutput> {
    // ... 原有逻辑不变 ...
}
```

---

## 实现步骤（按依赖顺序）

### Step 1: 事件与 trait 定义（无破坏性）
- [ ] `src/event.rs`: 新增 `LogDelta { id, name, log, stream }` 变体
- [ ] `src/tool.rs`: 新增 `ToolContext` 结构体
- [ ] `src/tool.rs`: 修改 `Tool::execute` 签名，增加 `context: Option<ToolContext>`
- [ ] `src/tool.rs`: 修改 `ToolRegistry::execute` 签名，增加 `context` 参数并透传

### Step 2: 全部 Tool 实现适配签名
- [ ] `src/tool/builtin/bash.rs`: 更新 `execute` 签名（暂不改实现）
- [ ] 其他所有 `impl Tool` 的文件：更新签名，增加 `_context` 参数
- [ ] 确保 `cargo check` 通过

### Step 3: Agent 层传递 ToolContext
- [ ] `src/agent.rs:220-259`: 构造 `ToolContext` 并传入 `tool_registry.execute()`
- [ ] 确保 `cargo check` 通过（此时 LogDelta 虽已定义但尚未发送）

### Step 4: BashTool 流式重构
- [ ] 将 `cmd.output().await` 替换为 `cmd.spawn()` + `Stdio::piped()`
- [ ] 实现 `tokio::select!` 并行读取 stdout/stderr
- [ ] 实现 200ms 时间窗口聚合限流
- [ ] 实现超时时的子进程 kill 与已有输出保留
- [ ] 更新单元测试 `test_shell_execution`

### Step 5: 网关层协议转发
- [ ] `src/gateway/protocol.rs`: `ProgressEvent` 新增 `log` 和 `stream` 字段
- [ ] `src/gateway/bridge.rs`: 新增 `LogDelta` -> `ChatProgress(kind=”tool_log”)` 转换
- [ ] 确保 `cargo check` 通过

### Step 6: 集成测试
- [ ] 编写测试：正常流式输出（多步 echo + sleep）
- [ ] 编写测试：脚本中途崩溃，验证 LogDelta 已发送 + ToolEnd 包含已有输出
- [ ] 编写测试：超时场景，验证子进程被 kill + 已有输出保留
- [ ] 编写测试：高频输出场景，验证限流生效（200ms 聚合）

### Step 7: 前端适配（前端团队）
- [ ] WebSocket 处理 `tool_log` 消息
- [ ] 实现 Log Streamer UI 组件
- [ ] 实现活跃指示器 + 超时告警

---

## 协议消息示例

### LogDelta (WebSocket 下行)

```json
{
  “id”: “req-abc123”,
  “type”: “chat.progress”,
  “payload”: {
    “type”: “tool_log”,
    “sessionId”: “session-001”,
    “toolName”: “bash”,
    “toolUseId”: “toolu_01XYZ”,
    “log”: “Running test suite...\nTest 1/10 passed\nTest 2/10 passed\n”,
    “stream”: “stdout”
  }
}
```

### stderr 示例

```json
{
  “id”: “req-abc123”,
  “type”: “chat.progress”,
  “payload”: {
    “type”: “tool_log”,
    “sessionId”: “session-001”,
    “toolName”: “bash”,
    “toolUseId”: “toolu_01XYZ”,
    “log”: “warning: unused variable `x`\n”,
    “stream”: “stderr”
  }
}
```

---

## 测试案例

1. **正常流式输出**：执行 `echo “Step 1”; sleep 2; echo “Step 2”; sleep 2; echo “Done”`
   - 预期：前端在脚本运行过程中依次看到 “Step 1”、”Step 2”、”Done” 的实时闪现。
   - 验证：`LogDelta` 事件的 `id` 与 `ToolStart` 的 `id` 一致。

2. **stdout + stderr 混合输出**：执行 `echo “info” && echo “warn” >&2 && echo “ok”`
   - 预期：前端收到 3 条 LogDelta，其中 stream 分别为 “stdout”、”stderr”、”stdout”。

3. **崩溃路径**：执行 `echo “before”; exit 1; echo “after”`
   - 预期：前端看到 “before” 的 LogDelta，随后收到 `tool_result`（`is_error: true`，exit_code: 1）。

4. **超时路径**：设置 `timeout_ms: 1000`，执行 `echo “start”; sleep 10; echo “end”`
   - 预期：前端看到 “start” 的 LogDelta，随后收到 `tool_result`（内容包含 “Command timed out” + “stdout so far: start”）。

5. **高频输出**：执行 `for i in $(seq 1 1000); do echo “line $i”; done`
   - 预期：由于 200ms 聚合，前端收到远少于 1000 条 LogDelta 消息。每条 LogDelta 的 `log` 字段包含多行聚合内容。

---

## 风险与缓解

| 风险 | 影响 | 缓解策略 |
|------|------|----------|
| 日志量过载冲击 WebSocket | 前端卡顿、网络带宽浪费 | BashTool 层 200ms 时间窗口聚合；通道缓冲区 100 提供背压 |
| `Tool` trait 签名变更影响面 | 所有 Tool 实现需修改 | 使用 `Option<ToolContext>` 保持向后兼容，其他工具仅改签名 |
| `child.kill()` 在 Windows 上的行为差异 | 子进程树可能无法完全终止 | Windows 上改用 `taskkill /F /T /PID` 或 `job object`（后续优化） |
| PowerShell 输出编码 | 非 UTF-8 字节导致行读取失败 | 已有 UTF-8 encoding 强制设置；BufReader 读取失败时降级到 lossy 处理 |

---

> [!IMPORTANT]
> 此文档已经过代码审查细化。确认后按 “实现步骤” 章节顺序落实代码修改。
