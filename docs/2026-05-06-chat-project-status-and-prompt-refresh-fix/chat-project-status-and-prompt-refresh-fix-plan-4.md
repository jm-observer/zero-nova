# Plan 4: Prompt 重载展示一致性修复与回归测试

| 章节 | 说明 |
|------|------|
| Plan 编号与标题 | Plan 4: Prompt 重载展示一致性修复与回归测试 |
| 前置依赖 | Plan 1 |
| 本次目标 | 让“重新加载提示词”形成可验证闭环，保证前端展示内容和后端最新版本一致。 |

## 涉及文件

1. `deskapp/src/ui/agent-console-view.ts`
2. `deskapp/src/gateway-client.ts`
3. `deskapp/src/core/state.ts`（必要时增加 prompt 版本状态）
4. `deskapp/src/ui/agent-console-renderers.ts`
5. `deskapp/src/i18n/zh.ts`
6. `deskapp/src/i18n/en.ts`
7. `deskapp/src/__tests__/agent-console-prompt-reload.test.ts`（新增）

## 详细设计

### 1. 重载闭环状态机

新增前端最小状态机：

1. `idle`：未重载
2. `reloading`：请求中
3. `awaiting_sync`：重载成功但展示版本未对齐
4. `synced`：展示版本已对齐 reload 返回版本
5. `failed`：重载失败

### 2. 版本对齐逻辑

1. `reloadSessionSystemPrompt(sessionId)` 返回 `versionAfter` 后，保存为 `expectedPromptVersion`。
2. 随后拉取 `session.prompt.preview` 与 `session.runtime`：
   - 若 `systemPromptState.version === expectedPromptVersion`，进入 `synced` 并刷新预览。
   - 若不一致，进入 `awaiting_sync` 并在短间隔内做有限重试（如 2~3 次，具名常量控制）。
3. 重试仍不一致则给出“重载已提交但前端尚未对齐”的提示，并保留最近一次预览。

### 3. 展示层增强

1. Prompt 区域增加简要状态提示：`已对齐版本 xxx` / `等待新版本同步` / `重载失败`。
2. 版本信息展示使用短版 + title 全量版本，保持可读性。
3. 不展示完整 prompt diff，避免引入额外复杂度。

### 4. 异常与回退策略

1. reload 请求失败：维持当前展示，不清空历史内容。
2. preview 拉取失败：保留旧预览并提示加载失败。
3. runtime/version 缺失：进入降级模式，仅按 preview 时间刷新提示。

## 测试案例

1. 重载成功且版本立即一致：状态 `reloading -> synced`，预览内容更新。
2. 重载成功但版本延迟一致：状态 `reloading -> awaiting_sync -> synced`。
3. 重载成功但始终不一致：状态最终停在 `awaiting_sync`，有明确提示。
4. 重载失败：状态 `reloading -> failed`，旧预览仍可见。
5. 并发点击重载按钮：后一次请求覆盖前一次预期版本，最终以最后一次为准。

