# 设计文档 - 修复 WebSocket 消息预览 Panic

- **时间**: 2026-04-28
- **项目现状**: 在 `channel-core` 的 WebSocket 处理器中，记录出站消息日志时，对 JSON 字符串进行截断预览的逻辑导致了 Panic（非字符边界切片）。
- **整体目标**: 修复该 Panic，确保日志预览逻辑在任何 UTF-8 字符串下都能安全运行。

## Plan 拆分

| Plan | 描述 | 状态 |
|------|------|------|
| [Plan 1](./fix-websocket-panic-plan-1.md) | 修复 `crates/channel-core/src/websocket.rs` 中的字符串切片逻辑 | 已完成 |
