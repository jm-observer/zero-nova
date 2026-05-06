# Plan 2: Provider HTTP Body 采集与持久化链路

## 前置依赖
- Plan 1: 元数据契约与消息结构扩展。

## 本次目标
在 Agent 到 LLM Provider 的真实请求链路中采集 request/response body，并按 `message.id` 精确写入对应 assistant 消息 metadata，且随会话消息持久化。

## 涉及文件
- `crates/nova-agent/src/provider/*`（OpenAI/Anthropic/兼容层请求封装）
- `crates/nova-agent/src/conversation/*`（消息生成与落库流程）
- `crates/nova-agent/src/message.rs`（消息模型）
- 消息存储仓储层（sqlite/repository 对应文件）

## 详细设计
1. 采集位置：
- 请求 body：在 Provider HTTP 请求发送前，拿到最终序列化 payload。
- 响应 body：在 Provider 响应完整接收后，拿到最终 JSON payload（流式场景在 stream complete 后聚合）。

2. 关联机制：
- 以“即将落库的 assistant 消息 `message.id`”为主键，把 `providerHttpTrace.boundMessageId` 写成同值。
- 若一次生成流程包含中间调用，仅保存最终产出该 assistant 消息的那一对 body。

3. 持久化策略：
- 在 assistant 消息写库前，把 `providerHttpTrace` 注入 metadata，一次性提交。
- 历史读取时通过原消息查询路径直接返回，无额外 join 查询。

4. 体积控制：
- 定义具名常量 `MAX_PROVIDER_HTTP_BODY_BYTES`。
- 超限时进行可解析截断：优先保留顶层结构和关键字段，设置 `truncated=true`。
- 截断策略需保证前端仍可 pretty-print，不写入非法 JSON。

5. 失败处理：
- 若采集失败，不影响主回复流程；消息正常落库但不附带 trace。
- 记录一次结构化日志（避免多层重复打点）。

## 测试案例
1. 正常路径：生成 assistant 消息后，metadata 内 request/response body 与 Provider 实际 payload 一致。
2. 边界路径：流式响应多 chunk，最终落库为完整 responseBody。
3. 边界路径：body 超限，`truncated=true` 且 JSON 仍可解析。
4. 错误路径：Provider 失败或解析失败，assistant 错误消息不携带无效 trace，主流程不中断。
5. 一致性路径：`boundMessageId == message.id` 恒成立。
