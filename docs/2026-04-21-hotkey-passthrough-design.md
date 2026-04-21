# 2026-04-21 快捷键透传改进设计

## 时间
2026-04-21

## 项目现状
当前应用在前端 `ChatView` 中监听了 `Ctrl+Win` 快捷键，并通过 Tauri 后端命令 `trigger_recording_hotkey` 使用 Enigo 库模拟按键。这种方式会导致原始按键被应用"吞掉"，且模拟按键在某些情况下不够稳定，导致外部录音软件无法正常接收该全局热键。

## 本次目标
实现快捷键的完全透传，确保当应用处于焦点时，按下 `Ctrl+Win` 仍能被系统级的其他软件（如录音工具）捕获。
- 移除前端的快捷键监听逻辑。
- 移除后端不再需要的模拟按键命令。
- 确保应用不拦截或消费该组合键。

## 详细设计
1. **前端修改**：
   - 在 `deskapp/src/ui/chat-view.ts` 中，移除 `bindEvents` 方法里监听 `Ctrl+Win` 并调用 `invoke` 的逻辑（已完成）。
2. **后端逻辑清理**：
   - 在 `deskapp/src-tauri/src/commands/system.rs` 中，移除 `trigger_recording_hotkey` 函数（已完成）。
   - 在 `deskapp/src-tauri/src/lib.rs` 中，从 `invoke_handler` 中移除该命令（已完成）。
3. **底层 WebView2 配置 (New)**：
   - 在 `deskapp/src-tauri/src/lib.rs` 的窗口初始化逻辑中，通过 native API 禁用 `AreBrowserAcceleratorKeysEnabled`。这将防止 WebView2 拦截 `Ctrl`、`Win` 等修饰键组合，确保它们能穿透到系统。

## 测试案例
1. **正常路径**：
   - 启动应用并聚焦。
   - 在输入框或任何地方按下 `Ctrl+Win`。
   - 验证外部录音软件能否成功启动录音。
2. **边界条件**：
   - 在输入框有焦点且正在输入时按下热键，验证输入框不会发生异常（如光标跳动等）。

## 风险与待定项
- 禁用浏览器加速键可能会导致 `F5`（刷新）、`Ctrl+P`（打印）等浏览器默认功能失效，但在桌面版 OpenFlux 中，这些功能本来就不是必需的。
- 该方案依赖 Windows 特有的 WebView2 接口，跨平台实现需条件编译。
