# Phase 1: 基础完善 - 详细设计

> 日期：2026-04-25
> 范围：修复关键缺失、补齐已知 TODO、提升稳定性

---

## 背景

Phase 1 聚焦于修复代码库中已识别的关键缺失功能和改进基础稳定性。不涉及新功能开发，而是确保现有功能完整可靠。

---

## 任务清单

### 1.1 文件预览 Tauri 命令缺失

**问题描述：**
`deskapp/src/ui/modals.ts:116-125` 在 `openFilePreview()` 方法中调用了三个 Tauri 命令，但 `deskapp/src-tauri/src/lib.rs:77-91` 中只注册了部分：

```
已注册：  file_exists, file_read, file_open, file_reveal, file_save_as
缺失：  file_stat, file_read_text
```

**前端调用链路：**
```
modals.ts:openFilePreview(filePath)
  ├─ invoke('file_stat', { filePath })                  // ❌ 未注册
  ├─ invoke('file_read', { filePath })                  // ✅ 已注册
  ├─ invoke('file_read_text', { filePath })             // ❌ 未注册
  ├─ invoke('file_open', { filePath })                  // ✅ 已注册
  └─ invoke('file_reveal', { filePath })                // ✅ 已注册
```

**设计决策：**
- `file_stat` 返回 `{size: number, isDir: boolean, modified: string}`
- `file_read_text` 复用 `file_read` 的文本读取逻辑，返回 `{content: string}` 更简洁
- 保持现有 `base64_encode` 无依赖实现

**实现方案：**

在 `deskapp/src-tauri/src/commands/file.rs` 中添加：

```rust
// 新增 FileInfo 结构体
#[derive(Serialize)]
pub struct FileInfo {
    pub size: u64,
    pub is_dir: bool,
    pub modified: String,
}

#[tauri::command]
pub async fn file_stat(file_path: String) -> Result<FileInfo, String> {
    // 返回文件元信息
}

#[tauri::command]
pub async fn file_read_text(file_path: String) -> Result<String, String> {
    // 简单文本读取
}
```

更新 `deskapp/src-tauri/src/lib.rs` 的 `invoke_handler`：
```rust
.invoke_handler(tauri::generate_handler![
    // ... existing commands ...
    commands::file::file_stat,      // 新增
    commands::file::file_read_text,  // 新增
    // ... rest ...
])
```

**测试验证：**
- 前端打开 png 图片预览 → file_read 正常工作
- 前端打开 txt/md 文本预览 → file_read_text 正常工作
- 前端打开 jpg 图片预览 → file_read 返回 dataUrl
- 文件不存在场景 → 错误提示正确

---

### 1.2 file_reveal Linux 实现

**问题描述：**
`deskapp/src-tauri/src/commands/file.rs:177-181` 中 `file_reveal` 对 Linux 平台是 `todo`：

```rust
#[cfg(target_os = "linux")]
{
    // todo
    // open::that(_parent.to_str().unwrap_or("")).map_err(|e| e.to_string())?;
}
```

**设计决策：**
- Linux 使用 `xdg-open` 命令作为 `open::that` 等效实现
- 无需引入额外依赖，利用现有 `std::process::Command`
- 如果在 PATH 中找到 `xdg-open` 则使用它，否则 fallback 到 `nautilus --select` 或 `dolphin`

**实现方案：**
```rust
#[cfg(target_os = "linux")]
{
    let parent_str = parent.to_str().unwrap_or("");
    if parent_str.is_empty() {
        return Err("无法获取父目录路径".to_string());
    }
    std::process::Command::new("xdg-open")
        .arg(parent_str)
        .spawn()
        .map_err(|e| {
            // 备用方案：尝试 nautilus --select
            std::process::Command::new("nautilus")
                .args(["--select", &file_path])
                .spawn()
                .map_err(|_| format!("Linux 文件显示失败: {}", e))
        })?;
}
```

---

### 1.3 Gateway 日志轮转实现

**问题描述：**
`deskapp/src-tauri/src/commands/gateway.rs:255` 中日志每次启动都截断：

```rust
.truncate(true)  // 每次启动覆盖
```

**设计决策：**
- 实现基于大小的日志轮转（每 10MB 滚动一次）
- 保留最近 N 个日志文件（默认 5 个）
- 压缩历史日志为 GZIP 格式
- 使用 `flate2` crate（已有依赖）

**实现方案：**

创建 `deskapp/src-tauri/src/commands/gateway_log.rs`：

```rust
pub struct LogWriter {
    current_file: Mutex<File>,
    current_size: Mutex<u64>,
    config: LogRotationConfig,
    log_dir: PathBuf,
}

impl LogWriter {
    // 写入一行，自动检查是否需要轮转
    pub fn write_line(&self, line: &str) -> Result<(), String>;

    // 执行日志轮转（压缩当前日志，移动旧日志）
    fn rotate_next(&self);
}
```

**更新 gateway.rs 中的日志逻辑：**
```rust
// 替换原有静态文件句柄
let log_writer = Arc::new(LogWriter::new(log_dir, config)?);

// 后台线程使用 new_writer
std::thread::spawn(move || {
    for line in reader.lines() {
        // ...
        let _ = log_writer.write_line(line);
    }
});
```

---

### 1.4 工具命名一致性修复

**问题描述：**
`TaskCreate` 和 `TaskUpdate` 的 `input_schema()` 中使用驼峰命名 `activeForm`，但在 `Task` 内部结构体中使用的是 `active_form` (snake_case)。这意味着工具注册表中的 schema 与内部的 `Task` 结构存在命名不一致。

**实现方案：**
统一使用 `active_form` (snake_case)：

```rust
// TaskCreateTool.input_schema()
pub fn input_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "subject": ...,
            "description": ...,
            "active_form": { "type": "string", ... },  // 改为 snake_case
            "metadata": ...,
        },
        "required": ["subject", "description"]
    })
}

// TaskUpdateTool.input_schema() - 同步更新
// TaskUpdateTool.execute() - 同步更新 input["activeForm"] → input["active_form"]
```

---

## 实施验证

### 编译检查
```bash
cargo clippy --workspace -- -D warnings  # 通过
cargo fmt --all --check                  # 通过
cargo test --workspace                   # 58 tests pass
```

### 影响范围

| 文件 | 变更类型 | 说明 |
|------|----------|------|
| `deskapp/src-tauri/commands/file.rs` | 新增 | file_stat, file_read_text |
| `deskapp/src-tauri/lib.rs` | 修改 | 注册新命令 |
| `deskapp/src-tauri/Cargo.toml` | 修改 | 添加 chrono 依赖 |
| `deskapp/src-tauri/commands/file.rs` | 修改 | 实现 Linux file_reveal |
| `crates/nova-core/src/tool/builtin/task.rs` | 修改 | activeForm → active_form |
| `deskapp/src-tauri/commands/gateway_log.rs` | 新增 | 日志轮转模块 |
| `deskapp/src-tauri/commands/gateway.rs` | 修改 | 集成 LogWriter |

---

## 风险评估

1. **Linux file_reveal 兼容性** - `xdg-open` 是标准工具，覆盖主流桌面环境
2. **日志轮转性能** - GZIP 压缩使用 `flate2`，阻塞不应明显
3. **命名一致性** - 向后兼容：LLM prompt 可同时识别两种命名
