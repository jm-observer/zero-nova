# Plan 1: Prompt 体量诊断

## 前置依赖

无。

---

## 本次目标

建立 prompt 请求体量的可观测能力，让后续精简有明确基线和验收指标。

可验证目标：

1. 能输出 system prompt 各 section 的字符数。
2. 能输出 tools schema 总字符数和单工具字符数。
3. 能输出 history 中每条消息的角色、内容长度、tool call 数量、tool result 长度。
4. 能标记超过阈值的高风险消息或 section。
5. 调试输出不影响正常请求内容。

---

## 涉及文件

| 文件 | 变更类型 | 说明 |
|---|---|---|
| `crates/nova-agent/src/prompt.rs` | 修改 | 为 `SystemPromptBuilder` 暴露 section 体量统计 |
| `crates/nova-agent/src/agent.rs` | 修改 | 在 prepare turn 或发送请求前生成 history 体量统计 |
| `crates/nova-agent/src/provider/openai_compat/conv.rs` | 修改 | 可选记录最终 OpenAI-compatible 请求摘要 |
| `crates/nova-agent/src/config.rs` | 修改 | 增加诊断开关和阈值配置 |
| `crates/nova-agent/tests/integration/*` | 新增/修改 | 覆盖统计输出和阈值标记 |

---

## 详细设计

### 1. 新增统计模型

建议新增轻量结构体，不引入 tokenizer 依赖：

```rust
pub struct PromptSizeReport {
    pub system_chars: usize,
    pub section_reports: Vec<PromptSectionSize>,
    pub tools_chars: usize,
    pub tool_reports: Vec<ToolSize>,
    pub history_chars: usize,
    pub message_reports: Vec<MessageSize>,
}
```

字段设计原则：

1. 使用字符数和字节数中至少一种。首版建议字符数即可，避免多字节 UTF-8 下切片误判。
2. 记录 section 名称，便于定位是 `Skill`、`DeveloperProjectPrompt` 还是 `ProjectContext` 过长。
3. 对 history 记录 message index，便于回查是哪一轮工具输出导致膨胀。
4. 不把完整内容写入日志，避免二次污染日志和隐私风险。

### 2. Section 统计

`SystemPromptBuilder` 当前内部保存 `sections: Vec<(SectionName, NamedSection)>`，可新增方法：

```rust
pub fn size_report(&self) -> Vec<PromptSectionSize>
```

该方法只读，不改变 build 输出。

统计内容：

1. section 名称。
2. section heading。
3. content 字符数。
4. priority。
5. 是否 required。
6. 是否超过配置阈值。

### 3. History 统计

在 `AgentRuntime::prepare_turn` 或发送请求前统计 `ctx.history`。

重点标记：

1. `Role::User` 中包含 `ToolResult` block 的消息。
2. 单条 content 超过阈值的消息。
3. 连续多条空 assistant 消息。
4. 单条 tool result 超过阈值。

当前 `tmp/response` 中最大问题是单条 tool 输出约 33.5K 字符，因此统计必须能直接暴露该项。

### 4. 配置项

建议新增：

```toml
[prompt_diagnostics]
enabled = false
large_section_chars = 8000
large_message_chars = 12000
large_tool_result_chars = 8000
```

默认关闭详细日志，只在 debug 命令、测试或显式配置开启时输出。

### 5. 输出形态

建议输出摘要日志：

```text
Prompt size: system=14908, tools=5111, history=35449
Large message: index=11 role=tool chars=33475
Large section: DeveloperProjectPrompt chars=5710
```

若已有 prompt preview 或 debug 命令，应优先把统计结果接入那里，避免常规日志噪音。

---

## 测试案例

| 类型 | 用例 | 期望 |
|---|---|---|
| 正常路径 | system prompt 含 3 个 section | 能返回每个 section 字符数 |
| 边界条件 | section 长度刚好等于阈值 | 不标记为超限或按明确规则标记 |
| 异常场景 | history 中存在 30K tool result | 报告中标记该消息为 large tool result |
| 多字节字符 | 中文内容超过阈值 | 字符数统计不 panic，不截断破坏 UTF-8 |
| 空内容 | 空 assistant 消息 | 统计为 0 并可标记为空消息 |
| 配置关闭 | diagnostics disabled | 不输出额外日志，不改变请求 |

---

## 验收标准

1. 能基于同类 `tmp/response` 请求定位最大上下文来源。
2. 统计逻辑不修改实际 prompt 和 history。
3. 单元测试覆盖 section、tools、history 三类统计。
4. `cargo clippy --workspace -- -D warnings`、`cargo fmt --all --check`、`cargo test --workspace` 通过。
