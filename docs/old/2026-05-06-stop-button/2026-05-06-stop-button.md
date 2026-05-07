# 停止按钮功能设计

**时间**: 2026-05-06

---

## 项目现状

### 前端

- `deskapp/src/ui/chat-view.ts` 中发送按钮（`#send-btn`）始终显示"发送"图标，无流式状态感知
- `deskapp/src/gateway-client.ts` 已实现 `stopTask(sessionId)` 方法（发送 `chat.stop` 消息），但**无任何 UI 入口调用它**
- `ChatView` 已有 `streamingMessageEl` / `streamingContent` 字段追踪流式输出状态，以及对 `chat:complete` / `chat:error` 事件的监听，但未与按钮状态挂钩
- i18n 文件中已有 `'chat.stop': '停止生成'` 字符串，说明该功能曾被规划但未完成

### 后端

- 协议层已定义 `chat.stop` / `chat.stop.response` 消息
- `handle_chat_stop` → `app.stop_turn()` → `session.take_cancellation_token().cancel()`
- Agent 循环在**迭代顶部、流事件循环、工具执行三个检查点**检测取消信号，检测后返回已收集的部分结果
- 整套取消机制**完整可用**，前端只需调用即可

---

## 整体目标

在 LLM 响应期间，将发送按钮替换为停止按钮，用户点击后中止当前 Agent Turn，后端返回已生成的部分内容并结束流式输出。

具体要求：

1. **状态切换**：消息发出后按钮变为停止图标，`chat:complete` / `chat:error` / 停止响应后恢复发送图标
2. **停止操作**：点击停止按钮调用 `gateway.stopTask(sessionId)`，同时在 UI 上立即给出反馈（禁用按钮防止重复点击）
3. **多会话正确性**：切换会话时，按钮状态反映**当前会话**的实际流式状态
4. **容错**：连接断开、`chat.stop.response` 未到达等异常情况下，按钮能自动恢复

---

## Plan 拆分

| Plan | 标题 | 说明 | 依赖 |
|------|------|------|------|
| Plan 1 | 前端状态管理 | 在 `ChatView` 中建立流式状态机，管理按钮切换逻辑，集成 `stopTask` 调用，处理多会话场景 | 无 |
| Plan 2 | UI 与样式 | 停止按钮 SVG 图标、CSS 过渡动画、`streaming` 状态样式类、可选的脉冲指示器 | Plan 1 |

---

## 风险与待定项

| 类型 | 描述 |
|------|------|
| **时序竞态** | `chat.stop.response` 与最后一个 `chat.progress` / `chat.complete` 的到达顺序不确定，需前端容忍两种顺序 |
| **部分结果存储** | 后端取消后返回已有 `turn_messages`，会话历史中会留下一条截断的助手消息，需确认产品行为是否可接受（目前后端已支持） |
| **多 Tab 同步** | 若同一会话在多个窗口/Tab 打开，停止操作只能通知当前 Tab；属于已知限制，暂不处理 |
