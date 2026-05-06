# Plan 2: OpenAiCompatClient 替换实现

## Plan 编号与标题

Plan 2: OpenAiCompatClient 替换实现

## 前置依赖

Plan 1（依赖引入与类型映射层）

## 本次目标

1. 用 `async-openai::Client` + `Chat::create_stream()` 重写 `OpenAiCompatClient`
2. 重写 `OpenAiCompatStreamReceiver`，消费 `ChatCompletionResponseStream` 并转换为 `ProviderStreamEvent`
3. 保持 `LlmClient` 和 `StreamReceiver` trait 接口不变
4. 删除旧的手写 SSE 解析逻辑（OpenAI 路径部分）

## 涉及文件

| 文件 | 操作 |
|------|------|
| `crates/nova-agent/src/provider/openai_compat.rs` | **重写** — 改为目录模块入口（或直接重写内容） |
| `crates/nova-agent/src/provider/openai_compat/types.rs` | 暂保留（Plan 3 删除） |

## 详细设计

### 1. 模块结构调整

当前 `openai_compat.rs` 是一个文件模块，内部有 `pub mod types` 子模块指向 `openai_compat/types.rs`。重构后结构：

```
provider/
├── openai_compat.rs          # 重写：使用 async-openai
├── openai_compat/
│   ├── conv.rs               # Plan 1 新建的转换层
│   └── types.rs              # 暂保留，Plan 3 删除
```

由于 Rust 模块系统中 `openai_compat.rs` 和 `openai_compat/` 目录可以共存（`openai_compat.rs` 作为模块文件，子目录作为子模块），保持现有结构。

### 2. OpenAiCompatClient 重写

```rust
use async_openai::{Client as OpenAiSdkClient, config::OpenAIConfig};

pub struct OpenAiCompatClient {
    client: OpenAiSdkClient<OpenAIConfig>,
}

impl OpenAiCompatClient {
    pub fn new(api_key: String, base_url: String) -> Self {
        // async-openai 内部会拼接 /chat/completions 路径，
        // 所以 base_url 应为 "https://api.openai.com/v1" 这种形式
        let config = OpenAIConfig::new()
            .with_api_key(api_key)
            .with_api_base(base_url);
        Self {
            client: OpenAiSdkClient::with_config(config),
        }
    }
}
```

**关键点 — base_url 处理**：
- 当前代码中 `base_url` 形如 `https://api.openai.com/v1`，stream 方法手动拼接 `{base_url}/chat/completions`
- `async-openai` 的 `with_api_base` 也是使用相同的模式：在 base 后面拼接 `/chat/completions`
- 需要验证是否需要 trim 尾部的 `/`，具体行为取决于 async-openai 内部实现

### 3. LlmClient::stream 实现

```rust
#[async_trait]
impl LlmClient for OpenAiCompatClient {
    async fn stream(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        config: &ModelConfig,
    ) -> Result<Box<dyn StreamReceiver>> {
        let request = conv::build_request(messages, tools, config);

        debug!(
            "[OUTBOUND] LLM HTTP request via async-openai: model={}, msg_count={}",
            config.model,
            messages.len()
        );

        // 序列化请求体用于日志/诊断
        let request_body = serde_json::to_value(&request)
            .unwrap_or(serde_json::Value::Null);

        let stream = self.client
            .chat()
            .create_stream(request)
            .await
            .map_err(|e| anyhow!("Failed to create chat stream: {}", e))?;

        Ok(Box::new(OpenAiCompatStreamReceiver::new(stream, request_body)))
    }
}
```

### 4. OpenAiCompatStreamReceiver 重写

当前的 `OpenAiCompatStreamReceiver` 负责：
1. 从 `reqwest::Response` 读取字节
2. 喂给 `SseParser` 解析 SSE 帧
3. 反序列化为 `ChatCompletionChunk`
4. 转换为 `ProviderStreamEvent`

重写后：
1. 持有 `ChatCompletionResponseStream`（async-openai 返回的 `Pin<Box<dyn Stream<Item = Result<CreateChatCompletionStreamResponse>>>>>`）
2. 调用 `.next()` 获取已解析的 `CreateChatCompletionStreamResponse`
3. 转换为 `ProviderStreamEvent`

```rust
use async_openai::types::CreateChatCompletionStreamResponse;
use futures_util::StreamExt;
use tokio_stream::Stream;
use std::pin::Pin;

pub struct OpenAiCompatStreamReceiver {
    stream: Pin<Box<dyn Stream<Item = Result<CreateChatCompletionStreamResponse, async_openai::error::OpenAIError>> + Send>>,
    /// 按 index 存储正在组装的 tool calls
    pending_tool_calls: Vec<Option<PendingToolCall>>,
    pending_stop_reason: Option<StopReason>,
    /// 缓存待发射的事件
    event_queue: VecDeque<ProviderStreamEvent>,
    request_body: serde_json::Value,
    response_chunks: Vec<serde_json::Value>,
}
```

### 5. StreamReceiver::next_event 实现

核心逻辑与当前 `process_chunk` 方法类似，但输入类型改为 `CreateChatCompletionStreamResponse`：

```rust
#[async_trait]
impl StreamReceiver for OpenAiCompatStreamReceiver {
    async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>> {
        loop {
            // 1. 先消费缓冲队列
            if let Some(event) = self.event_queue.pop_front() {
                return Ok(Some(event));
            }

            // 2. 从 async-openai stream 获取下一个 chunk
            match self.stream.next().await {
                Some(Ok(response)) => {
                    // 记录原始响应用于诊断
                    if let Ok(json) = serde_json::to_value(&response) {
                        self.response_chunks.push(json);
                    }
                    self.process_response(response);
                    continue;
                }
                Some(Err(e)) => {
                    return Err(anyhow!("OpenAI stream error: {}", e));
                }
                None => {
                    // 流结束：flush pending tool calls
                    self.flush_pending_tool_calls();
                    if let Some(event) = self.event_queue.pop_front() {
                        return Ok(Some(event));
                    }
                    return Ok(None);
                }
            }
        }
    }

    fn request_body(&self) -> Option<serde_json::Value> {
        Some(self.request_body.clone())
    }

    fn response_body(&self) -> Option<serde_json::Value> {
        Some(serde_json::Value::Array(self.response_chunks.clone()))
    }
}
```

### 6. process_response 方法

将当前的 `process_chunk(chunk: ChatCompletionChunk)` 改为 `process_response(response: CreateChatCompletionStreamResponse)`：

```rust
fn process_response(&mut self, response: CreateChatCompletionStreamResponse) {
    // --- Usage 处理 ---
    if let Some(usage) = response.usage {
        self.event_queue.push_back(ProviderStreamEvent::MessageComplete {
            usage: conv::map_usage(&usage),
            stop_reason: self.pending_stop_reason.take(),
        });
        return;
    }

    let Some(choice) = response.choices.first() else { return };

    // --- finish_reason 处理 ---
    if let Some(reason) = &choice.finish_reason {
        self.pending_stop_reason = Some(conv::map_finish_reason(reason));
    }

    let delta = &choice.delta;

    // --- Text content ---
    if let Some(content) = &delta.content {
        if !content.is_empty() {
            self.event_queue.push_back(ProviderStreamEvent::TextDelta(content.clone()));
        }
    }

    // --- Tool calls 增量组装 ---
    if let Some(tool_calls) = &delta.tool_calls {
        for tc in tool_calls {
            let idx = tc.index as usize;
            while self.pending_tool_calls.len() <= idx {
                self.pending_tool_calls.push(None);
            }

            if let Some(id) = &tc.id {
                let name = tc.function.as_ref()
                    .and_then(|f| f.name.as_ref())
                    .cloned()
                    .unwrap_or_default();
                self.pending_tool_calls[idx] = Some(PendingToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments_buffer: String::new(),
                });
                self.event_queue.push_back(ProviderStreamEvent::ToolUseStart {
                    id: id.clone(),
                    name,
                });
            }

            if let Some(func) = &tc.function {
                if let Some(args) = &func.arguments {
                    if !args.is_empty() {
                        if let Some(Some(pending)) = self.pending_tool_calls.get_mut(idx) {
                            pending.arguments_buffer.push_str(args);
                        }
                        self.event_queue.push_back(
                            ProviderStreamEvent::ToolUseInputDelta(args.clone())
                        );
                    }
                }
            }
        }
    }
}
```

### 7. 与当前实现的关键差异

| 维度 | 当前实现 | 新实现 |
|------|---------|--------|
| HTTP 客户端 | 手动 `reqwest::Client` | `async-openai::Client` 内部管理 |
| SSE 解析 | 手写 `SseParser` | `async-openai` 内部 `eventsource-stream` |
| 请求构建 | 手动 `serde_json::json!` | `CreateChatCompletionRequest` builder |
| 响应类型 | 自定义 `ChatCompletionChunk` | `CreateChatCompletionStreamResponse` |
| 重试 | 无 | async-openai 内置指数退避（rate limit） |
| 错误处理 | 手动检查 JSON error 字段 | `OpenAIError` 枚举 |
| reasoning_content | 支持 | 不支持（已确认去掉） |
| `[DONE]` 处理 | `SseParser` 返回 `RawSseEvent::Done` | `Stream` 返回 `None` |

### 8. 流结束处理差异

当前实现中，`[DONE]` 信号会触发 `flush_pending_tool_calls`。在 async-openai 中，`[DONE]` 会导致 stream 返回 `None`，等价。但需要注意：

- async-openai 在收到 `[DONE]` 后 stream 自然结束
- 如果 `stream_options.include_usage = true`，usage chunk 会在 `[DONE]` **之前**发送
- 因此 `MessageComplete` 事件会在 stream `None` 之前被正确发射

## 测试案例

### 1. 基本文本流式响应

模拟 stream 返回 3 个 text delta chunk + 1 个 usage chunk：
- 预期：收到 3 个 `TextDelta` + 1 个 `MessageComplete`
- 验证 `Usage` 中 `input_tokens` 和 `output_tokens` 正确

### 2. 工具调用流式响应

模拟 stream 返回 tool_calls 增量：
- chunk 1: `tool_calls[0].id = "t1", function.name = "search"`
- chunk 2: `tool_calls[0].function.arguments = '{"q":'`
- chunk 3: `tool_calls[0].function.arguments = '"rust"}'`
- chunk 4: `finish_reason = "tool_calls"`, usage
- 预期：`ToolUseStart` → `ToolUseInputDelta` × 2 → `ToolUseEnd` → `MessageComplete`

### 3. 混合文本 + 工具调用

模拟先输出文本再发起工具调用：
- 预期：`TextDelta` 在前，`ToolUseStart` 在后

### 4. 流错误处理

模拟 stream 返回 `Err(OpenAIError)`：
- 预期：`next_event` 返回 `Err(anyhow!(...))`

### 5. request_body / response_body 可追溯

- 验证 `request_body()` 返回序列化后的 `CreateChatCompletionRequest`
- 验证 `response_body()` 返回所有 chunk 的 JSON 数组
