# Plan 4: 聊天输入区 Project 下拉菜单

## 前置依赖
Plan 1

## 本次目标
1. 在聊天输入框附近显式展示当前会话 Project 目录。
2. 通过下拉菜单提供目录相关的轻量快捷动作。
3. 保持与现有 `@` 路径选择器、`project_dir_list` 和会话 runtime 数据一致。

## 涉及文件
- `deskapp/src/ui/chat-view.ts`
- `deskapp/src/styles/main/chat.css`
- `deskapp/src/core/state.ts`
- `deskapp/src/core/types.ts`
- `deskapp/src/__tests__/chat-view-project-picker.test.ts`
- `deskapp/src/__tests__/gateway-client-contract.test.ts`
- `deskapp/src-tauri/src/commands/file.rs`

## 详细设计

### 1. 交互位置与外观
在聊天输入区新增一个紧邻输入框的 `Project` 触发器：

- 折叠态显示：`Project: <basename>`
- hover 或展开面板显示完整绝对路径
- 若 `project_dir` 为空：显示 `Project: Not Set`

不把完整绝对路径直接塞进输入框主区域，避免挤压多行输入体验。

### 2. 菜单内容
下拉菜单至少包含：

- 第一项：当前绝对路径（只读、不可点击）
- 第二项：`复制路径`
- 第三项：`在文件管理器中打开`
- 第四项：`刷新`

若当前目录为空：

- 第一项显示 `未设置 Project 目录`
- 其余动作禁用

### 3. 数据来源
数据只来自当前会话的 `SessionRuntimeSnapshot.projectDir`。

刷新策略：

1. 当前会话切换时读取缓存
2. 缓存缺失时调用 `getSessionRuntime(sessionId)`
3. 收到 `session.runtime.updated` 时即时刷新按钮与菜单

这样菜单与实际工具执行根目录共享同一 source of truth。

### 4. 与 `@` 选择器协同
Project 菜单不替换 `@` 选择器，但增加两个协同点：

- 展开菜单时可提示“`@` 选择器将从该目录开始”
- 用户点击 `刷新` 后，如当前 `@` 选择器已打开，重新加载根目录列表

### 5. Tauri 侧能力复用
若 `file_reveal` 已支持目录路径，可直接复用打开系统文件管理器能力。
若不支持目录路径或行为不稳定，则在 `src-tauri` 增加专用命令，例如：

- `project_dir_open`

该命令只接收当前 runtime 返回的绝对路径，不允许前端任意拼装越界路径。

### 6. 长路径展示
Windows 路径可能很长，触发器显示规则：

- 主按钮只展示 basename
- tooltip / 面板内展示完整路径
- 面板路径文本允许换行或中间截断，但复制动作必须复制完整值

## 测试案例
1. 正常路径：当前 session runtime 带 `projectDir`，输入区显示 basename，菜单显示完整路径。
2. 正常路径：点击复制路径后，复制的是完整绝对路径。
3. 正常路径：点击打开目录后，调用对应 Tauri 命令。
4. 边界路径：`projectDir` 为空时，菜单展示“未设置”并禁用动作。
5. 回归路径：现有 `@` 项目路径选择器过滤、键盘选择测试继续通过。
6. 联动路径：收到 `session.runtime.updated` 后，菜单文案同步更新，无需刷新页面。
