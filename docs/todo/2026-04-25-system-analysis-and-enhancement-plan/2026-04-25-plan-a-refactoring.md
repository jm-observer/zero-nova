# Design Doc: Plan A - Code Quality & Refactoring (代码质量与规范修复)

**Date**: 2026-04-25
**Status**: Draft
**Author**: Claude Code

## 1. Current State (现状)

当前的 `nova-core` 模块在实现 Agent 核心循环时存在以下技术债：

### 1.1 违反规范的 Panic 风险
在 `crates/nova-core/src/agent.rs` 的 `run_turn` 逻辑中，存在对 `Option` 或 `Result` 使用 `.unwrap()` 的情况（例如在处理 `MaxTokens` 自动续写时）。这违反了项目 `CLAUDE.md` 中关于禁止在非 `main.rs` 和测试中使用 `.unwrap()` 的强制要求，极易导致生产环境下的进程崩溃。

### 1.2 函数职责过度耦合 (God Function)
`AgentRuntime::run_turn` 函数承担了过多的职责，逻辑链路极长：
1.  **消息准备**: 构建与 LLM 交互的 Prompt。
2.  **流式响应处理**: 解析 LLM 返回的流式内容（Text, Thinking, Tool Use）。
3.  **工具调度**: 解析工具调用指令并调用 `ToolRegistry`。
4.  **并发控制**: 使用 `FuturesUnordered` 管理并行工具任务。
5.  **状态迭代**: 将工具结果回填至对话历史并决定是否继续循环。

这种高度耦合导致了以下问题：
- **难以测试**: 无法对单一环节（如仅对工具调度逻辑）进行单元测试。
- **维护困难**: 修改工具执行逻辑极易意外破坏 LLM 响应解析逻辑。
- **代码复杂度**: 函数行数过多，阅读理解成本高。

---

## 2. Goals (目标)

1.  **零 Panic 保证**: 消除所有非必要的 `.unwrap()` 和 `.expect()`，改为防御性错误处理。
2.  **职责单一化 (Single Responsibility)**: 将 `run_turn` 进行解耦，拆分为职责明确的私有组件。
3.  **提升可测试性**: 使核心逻辑（如工具调度、消息转换）能够通过单元测试进行验证。
4.  **符合规范**: 确保所有错误都通过 `anyhow::Result` 进行传播。

---

## 3. Detailed Design (实现细节)

### 3.1 错误处理重构
- **策略**: 所有的 `unwrap()` 将替换为 `ok_or_else` 或 `if let Some(...)` 模式。
- **传播**: 如果无法恢复的逻辑错误发生，应构造一个描述性的 `anyhow::Error` 并通过 `?` 向上层（`Gateway`）传播，最终由网关转为协议错误。

### 3.2 `AgentRuntime` 结构重构
将 `run_turn` 的逻辑拆分为以下逻辑单元（建议作为 `AgentRuntime` 的私有方法）：

#### 3.2.1 `prepare_next_turn`
- **职责**: 负责处理上下文裁剪、System Prompt 构建、以及基于当前状态决定是否需要进行 `MaxTokens` 续写。
- **输入**: `ConversationHistory`
- **输出**: `LLMRequest`

#### 3.2.2 `process_llm_stream`
- **职责**: 消费来自 LLM Client 的流式数据，并将碎片化的 `Delta` 聚合为结构化的 `AgentEvent` (如 `ThinkingDelta`, `TextDelta`, `ToolUseStart`)。
- **输入**: `Stream<Item = LLMResponseChunk>`
- **输出**: `AgentEvent`

#### 3.2.3 `coordinate_tool_execution`
- **职责**:
    - 解析 `ToolUse` 指令。
    - 从 `ToolRegistry` 获取工具实现。
    - 使用 `FuturesUnordered` 管理并行任务。
    - 负责结果的排序与回填。
- **输入**: `Vec<ToolCall>`
- **输出**: `Vec<ContentBlock::ToolResult>`

### 3.3 模块化后的伪代码结构
```rust
impl AgentRuntime {
    pub async fn run_turn(&mut self, ...) -> Result<...> {
        loop {
            // 1. 准备阶段
            let request = self.prepare_next_turn().await?;

            // 2. 交互阶段 (流式)
            let mut stream = self.llm_client.stream_chat(request).await?;
            let events = self.process_llm_stream(&mut stream).await?;

            // 3. 决策与行动阶段
            if let Some(tool_calls) = events.extract_tool_calls() {
                let results = self.coordinate_tool_execution(tool_calls).await?;
                self.update_history(results)?;
            } else {
                // 正常结束
                break;
            }
        }
    }
}
```

---

## 4. Test Plan (测试计划)

### 4.1 单元测试 (Unit Tests)
- **Tool Dispatch Test**: 构造模拟的 `ToolCall` 列表，验证 `coordinate_tool_execution` 是否能正确并行执行并按顺序返回结果。
- **Message Parsing Test**: 提供多种格式的 LLM 响应碎片（包含思考、文本、工具调用），验证 `process_llm_stream` 是否能正确解析。
- **Error Handling Test**: 模拟工具执行失败的情况，验证错误是否被正确包装为 `ToolResult` 而非导致进程崩溃。

### 4.2 集成测试 (Integration Tests)
- **Full Loop Test**: 运行一个完整的 `run_turn` 流程，确保从 LLM 请求到工具执行再到结果回传的闭环逻辑正确。

### 4.3 规范检查
- 使用 `cargo clippy` 确保没有遗漏的 `unwrap` 或未处理的 `Result`。
- 运行 `cargo test` 确保重构后核心功能未退化。
