# 会话克隆（分支）功能设计文档

- 时间: 2026-04-20
- 项目现状: 会话历史只能线性增长，无法从中间点进行分支。
- 本次目标: 在聊天界面上支持右键点击消息进行“克隆”，创建一个新会话并包含当前会话到该消息为止的所有历史。

## 详细设计

### 1. 通讯协议扩展 (Gateway Protocol)
在 `MessageEnvelope` 中增加以下类型：
- `sessions.copy`: 客户端发起克隆请求。
  - `sessionId`: 源会话 ID。
  - `index`: 截断消息的索引（包含该索引的消息）。
- `sessions.copy.response`: 服务端返回克隆成功后的新会话对象。

### 2. 后端实现 (Rust)
- **SessionStore**: 增加 `copy_session(source_id, truncate_index)` 方法。
  - 通过 Uuid 生成新会话 ID。
  - 复制历史消息数组的 `0..=truncate_index` 部分。
  - 自动生成新名称，如 `Source Name (Copy)`。
- **Handlers**: 实现 `handle_session_copy` 并在 `router.rs` 中分发。

### 3. 前端实现 (TypeScript)
- **GatewayClient**: 封装 `copySession` 方法。
- **ChatService**: 监听 `SESSION_COPY` 事件，调用 Client 并更新 AppState 切换到新会话。
- **ChatView**:
  - 渲染消息时，在 HTML 中注入 `data-index` 属性。
  - 监听 `messagesContainer` 的 `contextmenu` 事件。
  - 实现自定义右键菜单，提供“克隆此会话”选项。
  - 菜单点击时通过 EventBus 发送 `SESSION_COPY` 事件。
- **Styles**: 在 `main.css` 中增加 `.context-menu` 相关样式。

## 测试案例
1. **正常路径**: 右键点击第 3 条消息，选择克隆。新会话应包含前 3 条消息，且当前视图自动切换到新会话。
2. **边缘情况**: 克隆最后一条消息。效果应等同于复制整个会话。
3. **显示验证**: 验证右键菜单样式是否符合整体美观（深色模式、圆角、阴影）。
4. **Agent 一致性**: 验证克隆后的会话是否保持与原会话相同的 Agent 配置。
