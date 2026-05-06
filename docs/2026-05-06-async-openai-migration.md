# OpenAI 兼容层迁移至 async-openai

## 时间

- 创建日期：2026-05-06

## 项目现状

当前 `nova-agent` crate 的 OpenAI 兼容层（`provider/openai_compat.rs`）使用 `reqwest` 手动构建 HTTP 请求、手动解析 SSE 流。主要组件：

| 文件 | 职责 |
|------|------|
| `provider/openai_compat.rs` | OpenAI 兼容 HTTP 客户端，请求构建、SSE 流消费 |
| `provider/openai_compat/types.rs` | 自定义流式响应类型（ChatCompletionChunk 等） |
| `provider/sse.rs` | 手写 SSE 帧解析器（同时被 Anthropic client 共用） |
| `provider/types.rs` | 共享类型（Usage、ToolDefinition、StopReason 等） |

存在的问题：
1. 手写 SSE 解析器维护成本高，且与官方 spec 同步存在滞后
2. 自定义请求/响应类型覆盖不全（如缺少 `completion_tokens_details`）
3. 非标准字段（`reasoning_content`、`enable_thinking`）增加了维护负担

## 整体目标

将 OpenAI 兼容层的 HTTP 请求和流式解析替换为 `async-openai` 库，具体包括：

1. 使用 `async-openai::Client` + `Chat::create_stream()` 替代手写 reqwest + SSE 逻辑
2. 使用 `async-openai` 的标准类型（`CreateChatCompletionRequest`、`CreateChatCompletionStreamResponse` 等）替代自定义类型
3. 通过 `async-openai` 的 `CompletionUsage` 类型获取 token 统计
4. 移除非标准字段支持（`reasoning_content`、`enable_thinking`、`include_reasoning`）
5. 保持 `LlmClient` trait 抽象不变，对上层 `AgentRuntime` 透明

### 不在范围内

- Anthropic client（`provider/anthropic.rs`）不受影响
- SSE 解析器（`provider/sse.rs`）保留，因 Anthropic client 仍在使用
- `LlmClient` / `StreamReceiver` / `ProviderStreamEvent` 等 trait 和枚举定义不变
- 本地 tokenizer 预估（不在本次范围）

## Plan 拆分

| Plan | 标题 | 简述 | 依赖 |
|------|------|------|------|
| Plan 1 | 依赖引入与类型映射层 | 在 workspace 和 nova-agent 中添加 async-openai 依赖；编写内部类型与 async-openai 类型之间的转换函数 | 无 |
| Plan 2 | OpenAiCompatClient 替换实现 | 用 async-openai Client + Chat::create_stream 重写 OpenAiCompatClient，删除旧的手写请求/SSE 逻辑 | Plan 1 |
| Plan 3 | 调用点适配与清理 | 更新所有 OpenAiCompatClient::new 调用点的构造方式；删除不再需要的 openai_compat/types.rs；运行完整检查 | Plan 2 |

## 风险与待定项

1. **async-openai 的 base_url 格式差异**：当前代码 `base_url` 通常形如 `https://api.openai.com/v1`，而 `async-openai` 的 `with_api_base` 预期的格式可能不含 `/v1` 后缀（库内部会拼接路径）。需要在实现时验证并做必要的 trim 处理。
2. **async-openai 的 reqwest 版本冲突**：需确认 async-openai 0.36.1 使用的 reqwest 版本与 workspace 中的 0.12 兼容。
3. **SSE 解析器保留**：`sse.rs` 仍被 Anthropic client 引用，不能删除。但 OpenAI 路径不再使用它。
4. **`ChatCompletionStreamResponseDelta` 无 `reasoning_content` 字段**：async-openai 严格遵循官方 spec，不含该字段。已确认暂时去掉此功能。
5. **Usage 类型映射**：async-openai 的 `CompletionUsage.prompt_tokens` 是 `u32`，当前内部 `Usage.input_tokens` 是 `u64`，需要安全转换。
