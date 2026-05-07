# Plan 2: Project 菜单同步链路修复

| 章节 | 说明 |
|------|------|
| Plan 编号与标题 | Plan 2: Project 菜单同步链路修复 |
| 前置依赖 | Plan 1 |
| 本次目标 | 确保项目路径切换后，聊天框上方 `Project` 显示、`@` 选择器根目录、手动刷新结果三者一致。 |

## 涉及文件

1. `deskapp/src/ui/chat-view.ts`
2. `deskapp/src/core/state.ts`（若需补充 runtime 状态辅助方法）
3. `deskapp/src/gateway-client.ts`（若需补充刷新响应字段处理）
4. `deskapp/src/i18n/zh.ts`
5. `deskapp/src/i18n/en.ts`
6. `deskapp/src/__tests__/chat-view-project-picker.test.ts`

## 详细设计

### 1. 刷新链路改为“请求序号 + 最新值提交”

1. 为 `refreshProjectMenuState()` 增加请求序号（reqId），仅接受最后一次请求结果。
2. 拉取 `session.runtime` 后统一调用 `applySessionProjectDir(sessionId, projectDir, source)`。
3. 在 `applySessionProjectDir` 中执行：
   - 更新 runtime 资源状态（如已拿到新快照）
   - 更新 `sessionProjectDirState`
   - 若路径变化，失效该会话的文件树缓存
   - 触发菜单重绘

### 2. runtime.updated 与手动刷新收敛

1. `handleSessionRuntimeUpdated` 与 `refreshProjectMenuState` 共用同一状态写入函数。
2. 当 runtime payload 不包含 `project_dir` 字段时，不覆盖当前值，避免误写为 `null`。
3. `project_dir = null` 作为显式清空语义保留。

### 3. 增加刷新反馈

1. 刷新成功且路径发生变化：提示“Project 已更新”。
2. 刷新成功但无变化：提示“Project 未变化”。
3. 刷新失败：提示“刷新失败，仍显示上次值”。

### 4. `@` 选择器与 Project 一致性

1. 每次 projectDir 变化时强制清空会话文件树缓存。
2. 下次触发 `@` 时必定基于新 projectDir 拉取目录树。

## 测试案例

1. 切换项目后收到 runtime.updated，菜单文案立即更新为新 basename。
2. 手动连点刷新 3 次，仅最后一次响应生效，不出现旧值回退。
3. 切换到无项目目录会话（`project_dir = null`），菜单显示“未设置”。
4. projectDir 变化后触发 `@`，目录列表来自新路径（验证缓存失效）。
5. 刷新失败时保留旧显示并出现失败提示。

