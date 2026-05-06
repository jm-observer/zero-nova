# Plan 2: 同 turn 内 EnvironmentSnapshot 实时刷新

## Plan 编号与标题

Plan 2: 同 turn 内 EnvironmentSnapshot 实时刷新

## 前置依赖

Plan 1（BashTool 读取 project_dir 作为 CWD）

## 本次目标

在同一个 turn 内，当 `ProjectManager` 工具更新了 `project_dir` 后，后续迭代中的工具调用（包括 Bash、Read、Write、Edit）能感知到最新的 `project_dir`，而不是继续使用 turn 开头采集的旧快照。

## 涉及文件

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/nova-agent/src/agent.rs` | 修改 | turn 循环中用可变 environment 替代不可变 clone |
| `crates/nova-agent/src/tool.rs` | 修改 | `ToolContext` 增加可选的 project_dir 回写通道，或 `ToolOutput` 增加 side-effect 描述 |
| `crates/nova-agent/src/tool/builtin/project_manager.rs` | 修改 | 通过通道通知 agent 循环 project_dir 已变更 |

## 详细设计

### 问题分析

两个 turn 执行路径（`run_turn` 和 `run_turn_with_context`）中的核心循环：

```rust
// agent.rs:384-393 (run_turn) / agent.rs:679-688 (run_turn_with_context)
for iteration in 0..max_iterations {
    // ... LLM 流式调用 ...
    let tool_result_blocks = self
        .execute_tool_calls(
            parsed_tool_calls,
            session_id,
            environment.clone(),  // ← 每次迭代都 clone 同一个初始 snapshot
            &event_tx,
            &cancellation_token,
        )
        .await?;
}
```

`environment` 参数在整个循环中**不可变**。即使 `ProjectManager` 在某次迭代中更新了数据库里的 `project_dir`，下一次迭代仍然使用旧的 snapshot。

### 方案：通过共享 `Arc<RwLock<Option<EnvironmentSnapshot>>>` 实现实时更新

引入一个共享可变容器，让 `ProjectManager` 工具在修改 project_dir 后同步更新 snapshot：

#### 步骤 1：在 `ToolContext` 中增加 snapshot 回写引用

```rust
// tool.rs
#[derive(Clone)]
pub struct ToolContext {
    // ... 现有字段 ...
    pub environment: Option<EnvironmentSnapshot>,
    /// 共享的可变环境快照，允许工具（如 ProjectManager）实时更新 project_dir。
    /// 所有同一迭代/turn 中的工具共享此引用。
    pub shared_environment: Option<Arc<RwLock<EnvironmentSnapshot>>>,
}
```

#### 步骤 2：在 agent turn 循环中使用共享 snapshot

```rust
// agent.rs - run_turn / run_turn_with_context
// 在循环开始前创建共享 snapshot
let shared_env: Option<Arc<RwLock<EnvironmentSnapshot>>> = environment.clone().map(|env| {
    Arc::new(RwLock::new(env))
});

for iteration in 0..max_iterations {
    // ... LLM 流式调用 ...

    // 每次迭代前从 shared_env 读取最新 snapshot
    let current_env = shared_env.as_ref().map(|se| se.read().unwrap().clone());

    let tool_result_blocks = self
        .execute_tool_calls(
            parsed_tool_calls,
            session_id,
            current_env,           // ← 每次迭代读取最新值
            shared_env.clone(),    // ← 传递共享引用
            &event_tx,
            &cancellation_token,
        )
        .await?;
}
```

#### 步骤 3：`execute_tool_calls` 签名调整

```rust
async fn execute_tool_calls(
    &self,
    parsed_tool_calls: Vec<(String, String, serde_json::Value)>,
    session_id: &str,
    environment: Option<EnvironmentSnapshot>,
    shared_environment: Option<Arc<RwLock<EnvironmentSnapshot>>>,
    event_tx: &mpsc::Sender<AgentEvent>,
    cancellation_token: &Option<CancellationToken>,
) -> Result<Vec<ContentBlock>> {
    // ... 在构造 ToolContext 时传入 shared_environment ...
    Some(ToolContext {
        // ...
        environment,
        shared_environment,
    }),
}
```

#### 步骤 4：`ProjectManagerTool` 更新共享 snapshot

```rust
// project_manager.rs - set 分支
"set" => {
    let path_str = input["path"].as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'path' for set action"))?;
    let path = PathBuf::from(path_str);

    match self.project_dir_service.set_project_dir(session_id, path).await {
        Ok(new_path) => {
            // 更新共享 snapshot 中的 project_dir
            if let Some(shared_env) = ctx.shared_environment.as_ref() {
                let mut env = shared_env.write().unwrap_or_else(|p| p.into_inner());
                env.project_dir = Some(new_path.to_string_lossy().to_string());
            }

            Ok(ToolOutput {
                content: format!("Project directory updated to: {}", new_path.display()),
                is_error: false,
            })
        }
        Err(e) => Ok(ToolOutput {
            content: format!("Failed to set project directory: {}", e),
            is_error: true,
        }),
    }
}
```

### 并发工具调用的时序考虑

LLM 可能在同一个 iteration 内返回多个工具调用（例如同时调用 ProjectManager 和 Bash），这些工具通过 `FuturesUnordered` 并发执行。

**场景分析**：
- 如果 LLM 在同一条 assistant message 中同时调用 `ProjectManager(set)` 和 `Bash(ls)`，它们并发执行，Bash 可能在 ProjectManager 完成之前就已经开始。
- 这在逻辑上是合理的：同一条 message 内的多个工具调用，LLM 并不期望它们有顺序依赖。
- **真正的场景是跨 iteration 的**：LLM 先发一条 message 调用 ProjectManager，收到结果后再发下一条 message 调用 Bash。此时 `shared_environment` 已经被更新。

因此 `shared_environment` 的 `RwLock` 机制足以保证跨 iteration 的正确性，同一 iteration 内的并发行为不构成问题。

### 为什么不每次 iteration 重新从数据库 collect EnvironmentSnapshot？

`EnvironmentSnapshot::collect()` 会执行 3 个 `git` 子进程命令（`rev-parse`、`status`、`log`），代价较高。如果每次 iteration 都重新 collect，一个包含 10 次迭代的 turn 会多执行 30 次 git 命令。通过 `shared_environment` 的局部更新方案，仅在 `project_dir` 实际变更时修改该字段，其余 git 信息保持不变（它们在 turn 内不太可能变化）。

## 测试案例

### 1. 同 turn 内 ProjectManager set 后 Bash 使用新目录

```
模拟 turn 执行：
  Iteration 1: LLM 调用 ProjectManager(set, "D:\\other_project")
  Iteration 2: LLM 调用 Bash("pwd")
验证: Bash 输出包含 "other_project"
```

### 2. 同 turn 内 ProjectManager set 后 Read 使用新 project_dir

```
模拟 turn 执行：
  Iteration 1: LLM 调用 ProjectManager(set, "D:\\other_project")
  Iteration 2: LLM 调用 Read(file_path="README.md")  // 相对路径
验证: Read 在 D:\other_project\README.md 中查找文件
```

### 3. 并发调用 ProjectManager 和 Bash（同一 iteration）

```
模拟 turn 执行：
  Iteration 1: LLM 同时调用 ProjectManager(set) 和 Bash("pwd")
验证: Bash 可能使用旧目录或新目录（取决于执行顺序），不 panic，不死锁
```

### 4. 未调用 ProjectManager 时 shared_environment 保持不变

```
模拟 turn 执行：
  Iteration 1-5: 仅调用 Bash 命令
验证: 所有 Bash 使用初始 project_dir，shared_environment 未被修改
```
