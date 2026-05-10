# Plan 3: 历史 Tool 输出压缩

## 前置依赖

Plan 1: Prompt 体量诊断。

---

## 本次目标

对回灌到后续模型请求的超长 tool result 做结构化压缩，降低 history 体量，同时保留足够的可追溯信息。

可验证目标：

1. 单条 tool result 超过阈值时自动压缩。
2. 压缩后保留工具名、退出码、原始字符数、行数、前后片段和截断说明。
3. 不破坏 tool call 与 tool result 的协议关联。
4. 不影响用户终端看到的原始工具输出，只影响写入模型历史的内容。
5. 支持按工具或按调用显式禁用压缩。

---

## 涉及文件

| 文件 | 变更类型 | 说明 |
|---|---|---|
| `crates/nova-agent/src/agent.rs` | 修改 | 在 tool result 写入 history 前应用压缩 |
| `crates/nova-agent/src/message.rs` | 可能修改 | 如需要保存压缩元信息，可扩展 block 或 metadata |
| `crates/nova-agent/src/config.rs` | 修改 | 增加 tool result 压缩阈值和策略配置 |
| `crates/nova-agent/src/tool/*` | 可选修改 | 对特殊工具声明保留完整输出或专用摘要 |
| `crates/nova-agent/tests/integration/*` | 新增/修改 | 覆盖压缩、边界、协议关联 |

---

## 详细设计

### 1. 压缩边界

压缩只发生在“写入后续模型 history 的 tool result 内容”这一层。

不压缩的内容：

1. 实际工具执行结果对象。
2. UI/终端对用户展示的原始工具输出。
3. 日志中必要的执行状态。
4. 文件本身内容。

压缩的内容：

1. `Message` 中作为下一轮上下文发送给模型的 tool result 文本。
2. OpenAI-compatible 转换后 `role=tool` 或等价 tool result message 的 content。

这样可以最大限度降低行为风险：工具真实执行不变，只减少模型回看历史时的冗余。

### 2. 压缩格式

建议格式：

```text
[Tool output compacted]
Tool: Read
Exit code: 0
Original chars: 33475
Original lines: 912
Kept head chars: 2048
Kept tail chars: 2048
Reason: output exceeded 12000 chars

--- head ---
...

--- tail ---
...

[Full output omitted from model context. Re-run a narrower command or read a specific range if needed.]
```

设计理由：

1. 明确告诉模型这是压缩内容，避免误以为文件只有这些内容。
2. 保留头尾片段，通常足以判断文件类型、错误摘要、末尾失败信息。
3. 明确提示可重新读取更小范围，促进后续工具调用更精准。
4. 保留原始长度和行数，便于诊断。

### 3. 阈值策略

建议配置：

```toml
[tool_result_compaction]
enabled = true
max_chars = 12000
head_chars = 4000
tail_chars = 4000
max_lines_hint = true
```

约束：

1. `head_chars + tail_chars` 必须小于 `max_chars`。
2. 截断必须按 UTF-8 字符边界处理，禁止按字节硬切。
3. 如果内容长度小于等于 `max_chars`，不做任何改变。
4. 如果 `head_chars + tail_chars` 大于内容长度，直接保留原文。

### 4. 工具级策略

不同工具输出价值不同，建议支持工具级策略：

| 工具 | 默认策略 | 原因 |
|---|---|---|
| `Bash` | 压缩 | 命令输出可能极长，且可重跑 |
| `Read` | 压缩 | 文件内容可按范围重新读取 |
| `rg` 输出 | 压缩 | 搜索结果可缩小 pattern 或路径 |
| `Edit` | 不压缩或极少触发 | 结果通常很短 |
| `Agent` | 压缩 | 子代理结果可能很长 |

实现上可以先统一按长度压缩，后续再加入工具白名单/黑名单。

### 5. 协议关联

OpenAI tool call 要求 assistant tool call 与 tool result 通过 `tool_call_id` 对齐。压缩只能改 content，不能改：

1. tool call id。
2. tool name。
3. message 顺序。
4. tool result block 类型。

因此压缩函数建议签名只接收并返回文本：

```rust
fn compact_tool_output(tool_name: &str, output: &str, config: &ToolResultCompactionConfig) -> String
```

由调用方保持原有 block metadata。

### 6. 与 HistoryTrimmer 的关系

现有 `HistoryTrimmer` 是请求前整体裁剪，Plan 3 是 tool result 入史前局部压缩。

两者职责不同：

1. Tool result compaction：减少单条消息体量，保留历史结构。
2. HistoryTrimmer：当总上下文仍超预算时，删除较早消息。

执行顺序建议：先压缩 tool result，再运行 history trimmer。

---

## 测试案例

| 类型 | 用例 | 期望 |
|---|---|---|
| 正常路径 | 1K tool output | 原样保留 |
| 边界条件 | output 长度等于 `max_chars` | 原样保留 |
| 超长输出 | 30K tool output | 返回 compacted 文本，包含 head/tail/原始长度 |
| 多字节字符 | 中文和 emoji 混合输出 | 不 panic，不产生无效 UTF-8 |
| 协议一致 | tool_call_id 存在 | 压缩后 id、name、顺序不变 |
| 配置关闭 | enabled=false | 原样保留 |
| 特殊工具 | Edit 短输出 | 不被误压缩 |

---

## 验收标准

1. 类似 `tmp/response` 的 33.5K tool 输出能被压到配置阈值以下。
2. 后续请求仍包含合法 tool result 消息。
3. 模型可根据压缩提示重新读取必要范围。
4. 修复流程全部通过。
