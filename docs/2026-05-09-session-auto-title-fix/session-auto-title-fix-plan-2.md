# Plan 2: 会话标题更新事件链路补全

## 前置依赖
- Plan 1

## 本次目标
- 在标题成功更新时，稳定发射 `session.summary.updated`。
- 确保事件内容与前端 `AppState.handleSessionSummaryUpdated` 兼容。
- 避免重复事件和无效事件。

## 涉及文件
- `crates/nova-agent/src/app/types.rs`
- `crates/nova-agent/src/app/application.rs`
- `crates/nova-agent/src/conversation/service.rs`
- `crates/nova-gateway-core/src/bridge.rs`
- `deskapp/src/core/state.ts`（仅在字段对齐需要时）
- `deskapp/src/gateway-client.ts`（仅在字段对齐需要时）

## 详细设计
- 发射点选择：
  - 标题写入成功且标题值发生变化后，在后端单点发射 `AppEvent::SessionSummaryUpdated`。
  - 推荐放在 `run_title_generation` 成功分支，避免在 `create/list` 路径重复推送。
- 事件载荷：
  - `session_id`
  - `title`
  - `updated_at`
  - `message_count`
  - `agent_id`
- 去重策略：
  - 若新标题与旧标题一致，不发事件。
  - 若状态不是首次成功（例如重试后仍相同），不发事件。
- 网关映射：
  - 保持 `AppEvent::SessionSummaryUpdated` -> `MessageEnvelope::SessionSummaryUpdated`。
  - 确认消息路由为广播给会话连接，不依赖请求响应通道。

## 测试案例
- 正常路径：
  - 标题从默认值更新为新标题后，客户端可收到 `session.summary.updated`。
- 边界条件：
  - 标题重复写入相同值时，不应产生重复事件。
- 异常路径：
  - 标题更新事件发送失败（channel closed）不应影响主聊天流程，且需 `warn` 日志记录一次。
