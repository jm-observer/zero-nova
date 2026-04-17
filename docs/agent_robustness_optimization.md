# AgentRuntime 逻辑鲁棒性优化设计文档

> 版本: v1.1 | 日期: 2026-04-17 | 状态: 草案

## 1. 背景

在对现有 `AgentRuntime`（基于 `v1.0` 增强版本）进行 Review 后，发现虽然已经实现了超时、取消、续写等核心功能，但在极端场景（如模型输出非法 JSON）和续写精度（如模型在 Tool Call 中途断掉）方面仍有优化空间。

本文档旨在提出针对性的改进方案，进一步提升 Agent 的鲁棒性和交互精度。

## 2. 核心改进点

### 2.1 Tool Input 解析鲁棒性增强

**现状**：
目前代码中（`agent.rs` 第 152/196 行）对 `input_json` 使用了 `unwrap_or_else(|_| serde_json::json!({}))`。
- **风险**：如果 LLM 返回了非法 JSON（如格式破碎），Agent 会静默地将空对象 `{}` 传给工具。
- **后果**：工具可能会因为缺少必需参数而报错（显式错误）或产生不可预知的行为（隐式错误）。

**改进方案**：
1.  **主动纠错逻辑**：如果 `input_json` 解析失败，不直接使用空对象，而是生成一个包含解析错误说明的 `ContentBlock::ToolResult` 发回给 LLM。
2.  **日志记录**：使用 `log::warn!` 记录解析失败，便于调试。

### 2.2 跨截断的 Tool Call 自动续写

**现状**：
目前的 `MaxTokens` 续写仅在 `tool_calls.is_empty()` 时触发。
- **风险**：如果 LLM 在生成一个大型 Tool Call JSON 的中途因为 `MaxTokens` 停止了，`tool_calls` 列表将不为空（包含了一个残缺的 call），目前的逻辑会跳过续写。
- **后果**：Agent 会尝试执行一个残缺的 Tool Call，导致解析失败。

**改进方案**：
1.  **残缺检查**：即使 `tool_calls` 不为空，如果 `last_stop_reason` 是 `MaxTokens`，则检查最后一个 Tool Call 的 JSON 字符串是否闭合。
2.  **回滚并重定向**：如果最后一个 Tool Call 被截断，将其从当前迭代的 `assistant` 消息中剔除（或者保留但标记为截断），向 LLM 发送 "Please continue your last tool call" 或是简单的 "Continue"。

### 2.3 `all_messages` 对话上下文管理

**现状**：
`run_turn` 中 `all_messages` 随着迭代不断增长。
- **建议**：在多轮迭代中，虽然需要全上下文才能让模型回忆起之前的工具结果，但应确保 `turn_messages` 返回的是这一轮次产生的**增量**消息，以便前端高效渲染。目前这一块实现基本正确，但逻辑可以更清晰。

## 3. 详细设计

### 3.1 Tool 解析错误反馈机制

```rust
// 伪代码参考
let input_val: serde_json::Value = match serde_json::from_str(&input_json) {
    Ok(v) => v,
    Err(e) => {
        log::warn!("Failed to parse tool input JSON: {}. Content: {}", e, input_json);
        // 这里不直接退出，而是构造一个占位符，或者在下文处理中标记为错误
        serde_json::json!({ "__error": format!("Invalid JSON: {}", e) })
    }
};
```

如果检测到 `__error`，`tool_registry.execute` 应能识别并返回一个友好的错误提示给 LLM，提示它重新生成正确的 JSON。

### 3.2 Tool 截断识别逻辑

```rust
fn is_json_potentially_truncated(json: &str) -> bool {
    let trimmed = json.trim();
    if trimmed.is_empty() { return true; }
    // 简单的启发式检查：是否以 } 结尾
    !trimmed.ends_with('}')
}

// 在 run_turn 循环逻辑中：
if last_stop_reason == Some(StopReason::MaxTokens) {
    let is_truncated = if tool_calls.is_empty() {
        true 
    } else {
        // 检查最后一个 tool call 是否结束
        let (_, _, last_json) = tool_calls.last().unwrap();
        is_json_potentially_truncated(last_json)
    };

    if is_truncated {
        // 执行续写逻辑
        // ...
    }
}
```

## 4. 涉及文件

- `src/agent.rs`: 核心逻辑修改。
- `docs/agent_robustness_optimization.md`: (本文件)

## 5. 验证计划

1.  **Mock LLM 测试**：模拟返回非法 JSON，验证 Agent 是否能向 LLM 报告解析错误。
2.  **截断模拟测试**：通过限制 `max_tokens` 强行在 Tool Call JSON 中间截断，验证 Agent 是否会自动发出续写请求。
