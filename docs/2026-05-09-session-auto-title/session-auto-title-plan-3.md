# Plan 3: 网关协议与前端同步链路

## 前置依赖
- Plan 1
- Plan 2

## 本次目标
- 让后台异步生成出的标题可以及时推送到前端
- 清理前端当前"首条消息即标题"的本地策略，改为以后端为准

## 涉及文件
- `crates/nova-agent/src/app/types.rs`
- `crates/nova-agent/src/app/application.rs`
- `crates/nova-gateway-core/src/bridge.rs`
- `crates/nova-protocol/src/session.rs`
- `deskapp/src/gateway-client.ts`
- `deskapp/src/services/chat-service.ts`
- `deskapp/src/core/state.ts`
- `deskapp/src/ui/sidebar-view.ts`
- `deskapp/src/ui/chat-view.ts`

## 详细设计
### 1. 新增会话标题更新事件
- **事件常量**：`SESSION_SUMMARY_UPDATED`（建议定义为 `const SESSION_SUMMARY_UPDATED = "session.summary.updated";`）
- **事件 payload 定义**：
  ```typescript
  interface SessionSummaryUpdatedEvent {
    sessionId: string;
    title: string;
    updatedAt: number;  // 时间戳（毫秒）
    messageCount: number;
    agentId: string;
    version?: string;   // 可选：事件版本，用于未来兼容
  }
  ```
- **字段必填性**：
  - `sessionId`：必填
  - `title`：必填
  - `updatedAt`：必填
  - `messageCount`：必填
  - `agentId`：必填
  - `version`：可选，用于未来扩展
- **原因**：
  - `SessionRuntimeUpdated` 语义偏运行态，不适合混入摘要字段。
  - 单独事件可以直接驱动 sidebar 和 chat header，无需前端每次再发 `sessions.list`。
- **版本兼容策略**：
  - 后端发送事件时附带 `version` 字段（默认 `"1.0"`）
  - 前端收到事件后检查 `version`，若不支持则忽略或降级处理
  - 未来扩展时可通过 `version` 区分不同 payload 结构

### 2. 后端发射时机
- 标题生成成功并写库后立即发事件。
- 若标题未变化则不发，避免重复刷新。
- 事件源建议放在 application 层，由 `ConversationService` 返回或通过内部回调上送，避免 repository 直接感知网关。

### 3. 前端状态更新
- `GatewayClient` 增加 `onSessionUpdated` 或更明确的 `onSessionSummaryUpdated`。
- `AppState` 增加 upsert 逻辑：
  - 根据 `sessionId` 找到 session。
  - 只更新返回字段，不破坏当前消息列表。
- `SidebarView` 与 `ChatView` 继续从 state 读取标题，无需单独维护副本。

### 4. 清理现有前端标题策略
- `ChatService.sendMessage` 在"当前无 session"时创建会话，不再执行：
  - `const title = text.length > 20 ? ...`
- 改为创建占位标题 session，例如直接不传 `title`，由后端统一填默认值。
- 手动点击"新建会话"也不需要把 UI 层字符串视为最终标题，只用于默认展示。

### 5. 兼容与回退
- 若旧网关或旧前端暂未支持新事件：
  - 前端仍可在 `turn_complete` 后补一次 `sessions.list` 作为兜底。
  - 但正式实现应以推送事件为主，避免把列表刷新绑到每次 turn 完成。

## 测试案例
- 正常路径：
  - 后台生成标题后，侧边栏 session 项立即更新。
  - 当前会话页头标题同步更新。
- 边界条件：
  - 标题事件到达时用户已切到其他 session，只更新 state，不误改当前消息区。
  - 收到同一标题的重复事件时，不触发多余重渲染。
- 异常路径：
  - 标题生成失败时前端保持默认标题，不出现错误弹窗。
  - 前端未收到事件时，下一次主动 `sessions.list` 仍能拿到已更新标题。
