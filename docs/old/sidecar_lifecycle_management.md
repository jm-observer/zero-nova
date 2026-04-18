# Nova Gateway Sidecar 生命周期管理设计文档

## 1. 背景

`nova_gateway` 作为一个独立的二进制程序，常被父进程（如 Tauri 应用）作为 Sidecar 启动。为了保证资源的有效回收，子进程必须能够在父进程非正常退出（崩溃、被强制结束等）时实现自动关闭，防止系统出现过多的“僵尸”进程或残留后台任务。

## 2. 核心问题

在 Windows 平台及普通终端环境下，依赖 `stdin` 的 EOF（文件结束符）来检测父进程退出存在局限性：
- **控制台共享**：在手动测试时，子进程往往继承终端控制台，即使父进程（Shell）退出，控制台句柄依然被子进程保留，导致 `stdin.read()` 永远阻塞而接收不到 EOF。
- **句柄管理**：在某些复杂的进程启动链中，`stdin` 的继承行为可能不如预期灵活。

## 3. 设计原则

- **鲁棒性**：提供多种互补的监控策略（PID 监控 + Stdin 监控）。
- **轻量级**：监控任务应占用极低的 CPU 和 IO 资源。
- **无侵入性**：父进程仅需按需提供 PID，不影响 Gateway 的核心业务逻辑。

## 4. 详细设计方案

### 4.1 监控策略 A：PID 活跃度监控 (推荐)

这是针对 Sidecar 场景最稳健的方案。

- **实现机制**：父进程在启动 `nova_gateway` 时，通过命令行参数 `--parent-pid <PID>` 传入自身的进程 ID。
- **监控逻辑**：
  1. `nova_gateway` 启动后解析参数，获取 `parent_pid`。
  2. 启动一个异步任务，每隔一定时间（如 2 秒）检查该 PID 是否仍在系统中活跃。
  3. **检查手段**：
     - 在 Windows 上，可以通过尝试打开进程句柄（`OpenProcess`）或使用底层系统调用检查。
     - 在生产环境下，推荐使用 `sysinfo` 库以保证跨平台一致性。
  4. 一旦检测到父进程不存在，Gateway 执行优雅关机并退出。

### 4.2 监控策略 B：Stdin EOF 监控 (基准方案)

作为兜底方案，利用 `tokio::select!` 监听 `stdin`。

- **实现机制**：子进程启动后，利用异步任务读取 `stdin`。
- **退出条件**：读取到 0 字节（EOF）。
- **改进点**：增加详细的追踪日志，帮助开发者识别当前 `stdin` 的状态。

### 4.3 整体架构

使用 `tokio::select!` 宏协调多个任务：

```rust
tokio::select! {
    // 业务服务器
    res = start_server(...) => { ... }
    
    // 策略 A: PID 监控
    _ = monitor_parent_pid(pid) => {
        log::warn!("Detected parent process exit via PID monitoring.");
        std::process::exit(0);
    }
    
    // 策略 B: Stdin 监控
    _ = monitor_stdin_eof() => {
        log::warn!("Detected parent process exit via Stdin EOF.");
        std::process::exit(0);
    }
    
    // 信号监听 (如 Ctrl+C)
    _ = tokio::signal::ctrl_c() => { ... }
}
```

## 5. 改进后的命令行接口 (CLI)

新增 `--parent-pid` 选项：

| 参数 | 缩写 | 说明 | 默认值 |
| :--- | :--- | :--- | :--- |
| `--parent-pid` | 无 | 父进程的 PID，用于生命周期绑定 | 可选 |

## 6. 依赖项评估

为了高效实现进程监控，建议引入以下依赖：
- **`sysinfo`**：轻量级地获取系统进程状态。

## 7. 方案演进方向 (Future Work)

- **Job Objects (Windows 专用)**：未来可以考虑在父进程侧将子进程放入 Windows Job Object，并设置 `KillOnJobClose`，这是 Windows 平台上最完美但也最重型的方案。
- **Heartbeat (双向检测)**：若需要更精确的状态同步，可考虑建立双向维持（Heartbeat）协议。
