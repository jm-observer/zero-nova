# Plan 3: ProjectManager 返回信息增强

## Plan 编号与标题

Plan 3: ProjectManager 返回信息增强

## 前置依赖

Plan 2（同 turn 内 EnvironmentSnapshot 实时刷新）

## 本次目标

改进 `ProjectManager` 工具的返回信息，使其准确反映操作的实际效果。同时改进 `get` 操作返回更丰富的上下文信息，帮助 LLM 更好地理解当前状态。

## 涉及文件

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/nova-agent/src/tool/builtin/project_manager.rs` | 修改 | 改进 get/set 操作的返回格式 |

## 详细设计

### 当前问题

1. **set 操作**：返回 `"Project directory updated to: \\?\D:\git\streaming-speech"`
   - 没有说明 session ID 上下文
   - 没有描述变更影响哪些工具
   - LLM 和用户无法区分"数据库记录更新"和"实际 CWD 切换"

2. **get 操作**：返回 `"Current project directory: \\?\D:\git\zero-nova"`
   - 缺少是否有效的状态（目录是否存在）
   - 缺少是来自 session 持久化还是默认值

### 修改方案

#### set 操作返回格式

```rust
"set" => {
    // ... 验证路径存在 ...
    match self.project_dir_service.set_project_dir(session_id, path).await {
        Ok(new_path) => {
            // 检查目录是否实际存在
            let exists = new_path.exists();

            // 更新 shared_environment（Plan 2）
            if let Some(shared_env) = ctx.shared_environment.as_ref() {
                let mut env = shared_env.write().unwrap_or_else(|p| p.into_inner());
                env.project_dir = Some(new_path.to_string_lossy().to_string());
            }

            let mut msg = format!(
                "Project directory changed to: {}\n\
                 Directory exists: {}\n\
                 Affected tools: Bash (CWD), Read/Write/Edit (relative path base)",
                new_path.display(),
                if exists { "yes" } else { "NO - commands may fail" },
            );

            if !exists {
                msg.push_str("\nWarning: The specified directory does not exist on disk.");
            }

            Ok(ToolOutput {
                content: msg,
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

#### get 操作返回格式

```rust
"get" => {
    match self.project_dir_service.get_project_dir(session_id).await {
        Ok(Some(path)) => {
            let exists = path.exists();
            Ok(ToolOutput {
                content: format!(
                    "Current project directory: {}\nDirectory exists: {}",
                    path.display(),
                    if exists { "yes" } else { "no" },
                ),
                is_error: false,
            })
        }
        Ok(None) => Ok(ToolOutput {
            content: "Project directory: not set (using process working directory as fallback)".to_string(),
            is_error: false,
        }),
        Err(e) => Ok(ToolOutput {
            content: format!("Failed to get project directory: {}", e),
            is_error: true,
        }),
    }
}
```

### set 操作增加路径验证

在设置 project_dir 之前，验证目标路径是否为有效目录：

```rust
"set" => {
    let path_str = input["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'path' for set action"))?;
    let path = PathBuf::from(path_str);

    // 检查路径是否存在且为目录
    if !path.exists() {
        return Ok(ToolOutput {
            content: format!(
                "Failed to set project directory: path '{}' does not exist",
                path.display()
            ),
            is_error: true,
        });
    }
    if !path.is_dir() {
        return Ok(ToolOutput {
            content: format!(
                "Failed to set project directory: path '{}' is not a directory",
                path.display()
            ),
            is_error: true,
        });
    }

    // 继续执行 set 逻辑...
}
```

## 测试案例

### 1. set 成功时返回包含影响范围

```
输入: ProjectManager(set, "D:\\git\\streaming-speech")  // 存在的目录
验证: 返回内容包含:
  - 新路径
  - "Directory exists: yes"
  - "Affected tools: Bash (CWD), Read/Write/Edit"
```

### 2. set 不存在的路径时返回错误

```
输入: ProjectManager(set, "D:\\nonexistent")
验证: 返回 is_error=true，内容包含 "does not exist"
```

### 3. set 文件路径（非目录）时返回错误

```
输入: ProjectManager(set, "D:\\git\\streaming-speech\\README.md")
验证: 返回 is_error=true，内容包含 "is not a directory"
```

### 4. get 返回目录存在状态

```
设置: project_dir = Some("D:\\git\\streaming-speech")
输入: ProjectManager(get)
验证: 返回包含 "Directory exists: yes"
```

### 5. get 无 project_dir 时返回友好提示

```
设置: project_dir = None
输入: ProjectManager(get)
验证: 返回包含 "not set" 和 "process working directory" 提示
```
