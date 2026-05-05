# Plan 3: DeskApp `@` 选择器迁移

## 前置依赖
Plan 1、Plan 2

## 本次目标
1. 前端 `@` 选择器完全改为调用后端会话文件树接口。
2. 移除 `@` 选择器对 Tauri `project_dir_list` 与 prompt 文本解析的依赖。
3. 处理会话切换、`project_dir` 更新、目录下钻和本地过滤的一致性。

## 涉及文件
- `deskapp/src/ui/chat-view.ts`
- `deskapp/src/gateway-client.ts`
- `deskapp/src/core/state.ts`
- `deskapp/src/core/types.ts`
- `deskapp/src/__tests__/chat-view-project-picker.test.ts`
- 如需共享 schema 使用：`deskapp/src/generated/*`

## 详细设计

### 1. 数据来源切换
当前 `@` 选择器链路：

- 输入 `@`
- 解析 prompt preview 中的 `Project directory:`
- 调用 Tauri `project_dir_list`

迁移后改为：

- 输入 `@`
- 直接使用 `currentSessionId`
- 调用 `gatewayClient.listSessionFileTree(sessionId, relativePath)`

这里前端不再关心绝对路径是什么，只关心当前会话 ID。

### 2. 本地状态模型
建议在 `ChatView` 或 `AppState` 中维护：

- `pickerCurrentPath`
- `pickerEntries`
- `pickerFilteredEntries`
- `pickerLoading`
- `pickerReqSeq`
- `sessionFileTreeCache: Map<sessionId, Map<relativePath, entries>>`

缓存粒度必须是“session + relative_path”，不能只缓存 project root。

### 3. 缓存失效规则
以下情况清空当前 session 的文件树缓存：

- `Events.SESSION_SELECTED` 切到其他 session
- 收到 `session.runtime.updated` 且 `project_dir` 变化
- 调用 `ProjectManager` 后本轮聊天完成并刷新 runtime

如果仅切换 prompt preview 文本但 `project_dir` 未变化，可保留缓存。

### 4. 过滤职责边界
建议保留现有前端本地过滤：

- 后端只返回当前目录单层所有条目
- 前端根据 token keyword 过滤

原因：

- 减少每次按键都请求远端
- 保持现有交互体验
- 后端接口保持“目录枚举”单一职责，不变成搜索接口

### 5. 空态和错误态
前端展示规则：

- 无项目目录：显示“当前会话未设置项目目录”
- 目录为空：显示“空目录”
- 目录不存在 / 越界：显示稳定错误文案
- 网络失败：显示“无法加载会话文件树”

不要因为出错就自动回退到 Tauri 本地目录，否则会重新引入架构歧义。

### 6. 旧 Tauri 路径命令的处理
`project_dir_list` 可以暂时保留，但必须：

- 从 `@` 选择器调用链彻底移除
- 降级为仅本地桌面文件功能备用，不再承担 session 目录浏览职责

必要时在代码中添加注释，说明其不适用于远端 session 文件树。

## 测试案例
1. 正常路径：输入 `@` 后，前端请求 `session.file_tree.list`，不再调用 Tauri `project_dir_list`。
2. 正常路径：同会话下钻目录时，请求带 `relative_path`，选中文件后插入 `@relative/path`。
3. 正常路径：切换到其他 session 后，读取的是新 session 的文件树缓存，不复用旧目录项。
4. 正常路径：收到 runtime 中 `project_dir` 更新后，旧缓存失效，再次输入 `@` 时拉取新目录。
5. 错误路径：后端返回“未设置项目目录”，前端显示错误态，不回退本地目录。
6. 远程路径：即使 `project_dir` 是远端绝对路径字符串，前端也无需解析该路径，仍可正常展示相对文件树。
