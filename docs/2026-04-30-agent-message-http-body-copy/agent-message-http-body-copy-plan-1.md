# Plan 1: 元数据契约与消息结构扩展

## 前置依赖
无。

## 本次目标
定义并落地“assistant 消息关联 Provider HTTP body”的统一数据契约，保证可持久化、可回放、可前端类型安全消费。

## 涉及文件
- `schemas/` 下聊天消息相关 schema（按现有消息 schema 位置补充 metadata 字段定义）
- `crates/nova-protocol/src/chat.rs`（或消息 DTO 对应文件）
- `deskapp/src/generated/schema-types.ts`
- `deskapp/src/core/types.ts`

## 详细设计
1. 新增 metadata 结构（挂载在 assistant 消息上）：
- `providerHttpTrace.requestBody`
- `providerHttpTrace.responseBody`
- `providerHttpTrace.format`（固定 `json`）
- `providerHttpTrace.boundMessageId`（冗余校验，必须等于当前消息 `id`）
- `providerHttpTrace.capturedAt`（毫秒时间戳）
- `providerHttpTrace.truncated`（布尔，标记是否因体积超限被截断）

2. 约束规则：
- 仅 assistant 消息允许该字段。
- `requestBody`、`responseBody` 均存储为 JSON 值（对象/数组/标量均可），不存字符串化 JSON。
- `boundMessageId` 必填，用于前端与服务端双向一致性校验。

3. Schema 与类型对齐：
- 先更新 schema，再同步 Rust DTO 与前端生成类型，符合“前后端结构体对齐”规则。
- 前端 `Message.metadata` 增加可选强类型读取辅助（不破坏现有 `Record<string, unknown>` 兼容路径）。

4. 兼容策略：
- 历史消息无该字段时，前端按钮仍显示但置为不可用态并提示“无可复制 body”。

## 测试案例
1. 正常路径：assistant 消息 metadata 含完整 `providerHttpTrace`，schema 校验通过。
2. 边界路径：仅有 requestBody 或仅有 responseBody，校验不通过（按必填约束）。
3. 错误路径：`boundMessageId` 与消息 `id` 不一致，服务端拒绝持久化或写入告警并丢弃该 trace。
4. 兼容路径：无 `providerHttpTrace` 的旧消息仍可被反序列化并正常展示。
