# Plan 1: BashTool 读取 project_dir 作为 CWD

## Plan 编号与标题

Plan 1: BashTool 读取 project_dir 作为 CWD

## 前置依赖

无

## 本次目标

修改 `BashTool` 的执行逻辑，在 `ToolContext.environment.project_dir` 存在时将其设为命令的 `current_dir`，使 Bash 命令在 session 绑定的项目目录下执行。

## 涉及文件

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/nova-agent/src/tool/builtin/bash.rs` | 修改 | 在 `execute()` 中从 context 获取 project_dir 作为 CWD |

## 详细设计

### 当前行为

`BashTool::execute()` 中的 CWD 逻辑（`bash.rs:219-222`）：

```rust
let mut cmd = self.shell.build_command(command_str);
if let Some(ws) = &self.workspace {
    cmd.current_dir(ws);
}
```

- `self.workspace` 在 `new()` 中固定为 `None`（`bash.rs:143`）
- 结果：命令始终在 gateway 进程的 CWD（即启动目录）执行
- `ToolContext.environment.project_dir` 完全未被读取

### 修改方案

在 `execute()` 中增加 CWD 解析优先级：

1. **`self.workspace`**（构造时指定，兼容现有 API）
2. **`context.environment.project_dir`**（session 级别，来自 `ToolContext`）
3. **不设置**（使用进程默认 CWD）

#### 代码修改（前台执行路径）

```rust
// bash.rs execute() 方法中，替换原有的 workspace 判断逻辑
let mut cmd = self.shell.build_command(command_str);

// CWD 优先级: workspace (构造时指定) > environment.project_dir (session 级别)
let effective_cwd = self.workspace.clone().or_else(|| {
    context
        .as_ref()
        .and_then(|ctx| ctx.environment.as_ref())
        .and_then(|env| env.project_dir.as_deref())
        .map(PathBuf::from)
});

if let Some(cwd) = &effective_cwd {
    cmd.current_dir(cwd);
}
```

#### 代码修改（后台执行路径）

后台执行分支（`run_in_background = true`，`bash.rs:190-216`）同样需要修改：

```rust
if run_in_background {
    let shell = self.shell.clone();
    let command_str_owned = command_str.to_string();
    let ctx = context.clone();

    // 在 spawn 前计算 effective_cwd
    let effective_cwd = self.workspace.clone().or_else(|| {
        ctx.as_ref()
            .and_then(|c| c.environment.as_ref())
            .and_then(|env| env.project_dir.as_deref())
            .map(PathBuf::from)
    });

    tokio::spawn(async move {
        let mut cmd = shell.build_command(&command_str_owned);
        if let Some(cwd) = effective_cwd {
            cmd.current_dir(cwd);
        }
        // ... 其余不变
    });
    // ...
}
```

### 删除 `with_workspace` 方法

`BashTool::with_workspace()` 未在生产代码中使用。经审查：

- `register_builtin_tools()` 调用 `BashTool::new(config)` — 不使用 `with_workspace`
- 无其他调用点

现在 `execute()` 已从 `ToolContext.environment.project_dir` 获取 CWD，`with_workspace` 不再需要。**保留 `workspace` 字段但标记为 deprecated**，或直接移除。建议：直接移除 `workspace` 字段和 `with_workspace()` 方法，简化代码。

## 测试案例

### 1. 正常路径：project_dir 存在时 Bash 使用该目录

```
设置: ToolContext.environment.project_dir = Some("D:\\git\\streaming-speech")
输入: Bash(command="pwd" / "Get-Location")
验证: 输出包含 "streaming-speech"
```

### 2. 无 project_dir 时使用进程默认 CWD

```
设置: ToolContext.environment = None
输入: Bash(command="pwd")
验证: 输出为进程启动目录（不报错，向后兼容）
```

### 3. workspace 优先于 project_dir（如保留 workspace 字段）

```
设置: workspace = Some("/tmp/ws"), project_dir = Some("/other")
输入: Bash(command="pwd")
验证: 输出为 /tmp/ws
```

### 4. project_dir 路径不存在时的行为

```
设置: project_dir = Some("D:\\nonexistent")
输入: Bash(command="echo hello")
验证: 命令执行失败，返回有意义的错误信息（OS 层面 spawn 会失败）
```
