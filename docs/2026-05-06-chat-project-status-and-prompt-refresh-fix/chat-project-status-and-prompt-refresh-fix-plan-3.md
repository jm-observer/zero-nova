# Plan 3: 标题栏连接状态稳定性修复

| 章节 | 说明 |
|------|------|
| Plan 编号与标题 | Plan 3: 标题栏连接状态稳定性修复 |
| 前置依赖 | Plan 1 |
| 本次目标 | 修复“标题栏一直连接中”问题，确保连接态、Provider 健康态、运行中提示互不干扰。 |

## 涉及文件

1. `deskapp/src/core/event-bus.ts`
2. `deskapp/src/main.ts`
3. `deskapp/src/ui/titlebar.ts`
4. `deskapp/src/ui/chat-view.ts`
5. `deskapp/src/__tests__/titlebar-status.test.ts`（新增）

## 详细设计

### 1. 明确状态优先级

1. 第一优先级：连接态（`connected` / `connecting` / `reconnecting` / `disconnected` / `failed`）。
2. 第二优先级：Provider 健康聚合态（仅在连接态为 `connected` 时参与）。
3. 第三优先级：运行文案（仅作为附加文本，不改变连接灯颜色逻辑）。

### 2. 事件拆分落地

1. `main.ts` 只通过连接态事件更新标题栏连接状态。
2. `chat-view.ts` 迭代进度改发运行文案事件，不再写 `gateway:status`。
3. `titlebar.ts` 增加运行文案处理器，和连接态处理器分离。

### 3. Provider 状态聚合规则保持，但加保护

1. 当连接态不是 `connected`，直接显示连接态文案，不读取 provider map。
2. Provider map 为空时显示“网关已连接，Provider 状态未知”。
3. Provider 状态更新只影响 `connected` 场景，防止断连时错误显示“就绪”。

### 4. 初始化与恢复逻辑

1. 首次渲染默认值可为 `connecting`，但一旦收到连接事件必须立即覆盖。
2. reconnect 流程中，标题栏应稳定显示 `reconnecting`，直到收到 `connected`。

## 测试案例

1. 连接建立后状态从 `connecting` -> `connected`，标题栏不再停留连接中。
2. 聊天中发出运行文案事件，不改变连接态颜色/主状态。
3. 断网触发 `reconnecting` / `disconnected`，Provider 健康状态不覆盖连接态。
4. 恢复连接后，Provider 聚合态继续生效（healthy/degraded/error）。

