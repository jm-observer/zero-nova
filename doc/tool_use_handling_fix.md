# Tool Use Handling Fix – 解决 LLM `tool_use` 未触发的根本原因

## 1️⃣ 现象
在实际运行 `nova-cli` 时，日志只出现了一系列 `content_block_start`、`content_block_delta`、`content_block_stop`、`message_delta (stop_reason="tool_use")`，随后程序直接结束，没有任何 `ToolStart` / `ToolEnd` 事件，也没有看到 `web_fetch` 的执行结果。

## 2️⃣ 根本原因
- **`AnthropicStreamReceiver::next_event` 只对 `ContentBlockDelta` 中的 `text` 字段作解析**，对 `input_json_delta`（即工具调用的 JSON 片段）直接忽略。
- 当 LLM 发送 `tool_use` 块时，这些块被当作普通文字处理，导致 `run_turn` 根本收不到 `ToolUseStart` / `ToolUseInputDelta`，于是 `tool_calls` 为空，程序直接 `break` 结束。

## 3️⃣ 解决方案概览
> 目标：让 `AnthropicStreamReceiver` 能够识别并拆解 `tool_use` 块，生成对应的 `ProviderStreamEvent::ToolUseStart` 与 `ToolUseInputDelta`，从而走完整个工具调用链路。

### 关键改动
1. **在 `AnthropicStreamReceiver` 中维护临时缓存**
   ```rust
   struct AnthropicStreamReceiver {
       response: reqwest::Response,
       parser: SseParser,
       pending_tool_events: VecDeque<ProviderStreamEvent>,
       current_block_buf: String,
   }
   ```
2. **在 `next_event` 里**：
   - 当解析到 `ContentBlockDelta` 且 `type == "input_json_delta"` 时，把 `partial_json` 片段拼接到 `current_block_buf`，不立刻返回事件。
   - 当收到 `ContentBlockStop`（块结束）时，解析累计的完整 JSON，判断是否为 `tool_use`。如果是，则先 `push_back(ProviderStreamEvent::ToolUseStart { id, name })`，再 `push_back(ProviderStreamEvent::ToolUseInputDelta(input_json))`。
   - 之后的循环会依次返回这两条事件，正好匹配 `run_turn` 的处理逻辑。
3. **保留原有文字处理**：其他 `ContentBlockDelta` 中的 `text` 仍旧返回 `TextDelta`，不影响现有功能。

### 示例代码（核心）
```rust
impl StreamReceiver for AnthropicStreamReceiver {
    async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>> {
        // 先处理已经排好的工具事件
        if let Some(ev) = self.pending_tool_events.pop_front() {
            return Ok(Some(ev));
        }
        // 读取 SSE、喂给 parser …
        // 当 parser 产出一个 StreamEvent 时：
        let provider_event = match event {
            // 文字块
            crate::provider::types::StreamEvent::ContentBlockDelta { delta, .. } => {
                if let Some(t) = delta.get("type") {
                    if t == "input_json_delta" {
                        // 累积 JSON 片段
                        if let Some(p) = delta.get("partial_json").and_then(|v| v.as_str()) {
                            self.current_block_buf.push_str(p);
                            return Ok(None);
                        }
                    }
                }
                // 仍然处理普通文字
                if let Some(txt) = delta.get("text").and_then(|t| t.as_str()) {
                    ProviderStreamEvent::TextDelta(txt.to_string())
                } else {
                    continue;
                }
            }
            crate::provider::types::StreamEvent::ContentBlockStop { .. } => {
                // 解析累计的 JSON
                let json_str = &self.current_block_buf;
                self.current_block_buf.clear();
                let v: serde_json::Value = serde_json::from_str(json_str)
                    .map_err(|e| anyhow!("Failed to parse tool_use JSON: {}", e))?;
                if v.get("type") == Some(&serde_json::json!("tool_use")) {
                    let id = v["id"].as_str().unwrap().to_string();
                    let name = v["name"].as_str().unwrap().to_string();
                    let input = v["input"].to_string();
                    self.pending_tool_events.push_back(
                        ProviderStreamEvent::ToolUseStart { id: id.clone(), name: name.clone() }
                    );
                    self.pending_tool_events.push_back(
                        ProviderStreamEvent::ToolUseInputDelta(input)
                    );
                    return Ok(None);
                }
                ProviderStreamEvent::TextDelta(String::new()) // ignore non‑tool blocks
            }
            // 其余保持不变 …
            crate::provider::types::StreamEvent::MessageDelta { delta, .. } => {
                if let Some(txt) = delta.get("text").and_then(|t| t.as_str()) {
                    ProviderStreamEvent::TextDelta(txt.to_string())
                } else {
                    continue;
                }
            }
            crate::provider::types::StreamEvent::MessageComplete { usage } => {
                ProviderStreamEvent::MessageComplete { usage }
            }
            crate::provider::types::StreamEvent::Error { error } => {
                return Err(anyhow!("Anthropic API Error: {}", error));
            }
            _ => continue,
        };
        Ok(Some(provider_event))
    }
}
```

## 4️⃣ 验证方法
1. **编译运行**
   ```bash
   cargo run --bin nova-cli --run "帮我抓取 https://example.com" --verbose
   ```
   - 期待看到 `[tool: web_fetch]` 开始日志，随后显示抓取结果或错误。
2. **检查日志**
   - 必须出现 `ToolStart`、`ToolEnd`（或错误）事件；如果仍然缺失，请确认 `input_json_delta` 已被累计（`%E6%…` 片段完整拼接后形成合法 JSON）。
3. **网络可达性**
   - 确认机器可以访问外网，且 `API_KEY`（Anthropic）有效。
   - 如需要调试，可在 `web_fetch` 中加入 `log::debug!` 打印请求 URL。

## 5️⃣ 小结
- 之前的实现只能捕获文字 `text`，导致 `tool_use` 块被丢失。
- 通过累计 `input_json_delta` 并在块结束时一次性解析为 `ToolUseStart` + `ToolUseInputDelta`，即可让 `AgentRuntime` 正确收到工具调用事件。
- 该改动仅涉及 `src/provider/anthropic.rs`，不影响其它功能，保持了现有文字流处理逻辑。

---
*此文档用于记录本次 bug 修复方案以及后续维护参考。*
