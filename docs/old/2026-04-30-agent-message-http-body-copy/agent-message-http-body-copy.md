# Agent 消息 HTTP Body 复制设计

## 时间
- 创建时间：2026-04-30
- 最后更新：2026-04-30

## 项目现状
- 当前聊天消息渲染位于 `deskapp/src/ui/chat-view.ts`，消息模型定义位于 `deskapp/src/core/types.ts`。
- 现有 `Message.metadata` 为 `Record<string, unknown>`，可承载扩展字段，但没有标准化的“Provider HTTP 请求/响应 body”结构。
- 当前前端能展示消息内容、工具调用和工具结果，但没有针对单条 assistant 消息的“复制上游 Provider 请求/响应 body”操作。
- 会话消息在服务端持久化，前端通过 `getMessages` 拉取后渲染，具备把扩展元数据回显到历史消息的基础路径。

## 整体目标
在每条 assistant 消息旁新增两个按钮，分别复制与该消息 `message.id` 精确关联的 Provider HTTP 请求 body 与响应 body；复制内容为 pretty JSON；按钮对所有用户可见；数据持久化到会话消息 metadata，支持历史回看。

## Plan 拆分

| Plan | 标题 | 简述 | 依赖 | 顺序 | 状态 |
|---|---|---|---|---|---|
| Plan 1 | 元数据契约与消息结构扩展 | 定义可持久化的 metadata 字段与前后端类型映射，确保 `message.id` 精确关联 | 无 | 1 | 待开始 |
| Plan 2 | Provider HTTP Body 采集与持久化链路 | 在 Agent→Provider 调用路径采集 request/response body 并写入对应 assistant 消息 metadata | Plan 1 | 2 | 待开始 |
| Plan 3 | 前端按钮交互与复制行为 | 在 assistant 消息旁展示按钮，读取 metadata 并执行 pretty JSON 复制与异常提示 | Plan 1, Plan 2 | 3 | 待开始 |
| Plan 4 | 测试与回归验证 | 增加后端、前端与集成测试，覆盖正常/边界/错误路径 | Plan 1, Plan 2, Plan 3 | 4 | 待开始 |

执行顺序：Plan 1 → Plan 2 → Plan 3 → Plan 4。

## 风险与待定项
- 若一次 assistant 消息由多次 Provider 调用拼接生成，需要明确“最终绑定哪一对 request/response body”。本设计默认绑定“产出该 assistant 消息最终内容的主调用”。
- Provider SDK/适配层若存在流式增量协议，response body 的落盘时机需在流结束后统一组装，避免不完整 JSON。
- metadata 体积增加可能影响消息存储大小与拉取性能，需设置上限与降级策略（例如超过阈值时保留结构摘要并标记截断）。
