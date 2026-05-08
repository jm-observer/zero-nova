# Orchestrator Read Loop Guard - Design Review

| 章节 | 内容 |
|---|---|
| 时间 | 2026-05-09 |
| 评审对象 | orchestrator-read-loop-guard.md |
| 评审结论 | 设计文档质量：良好，建议推进实施 |

## 设计假设验证

| 设计假设 | 验证结果 | 说明 |
|---------|---------|------|
| `ReadTool` 是全局共享的内置工具实例 | ✅ 正确 | `ReadTool` 作为内置工具注册，不能将 turn-scoped 状态直接塞入其结构体 |
| 重复检测需要区分分页读取 | ✅ 正确 | `ToolCallSignature` 包含 `offset` 和 `limit`，不同 offset 的 Read 不会被误判 |
| assistant 文本指纹需要轻量 | ✅ 正确 | `AssistantFingerprint` 只取前 64 字符 hash + 长度，避免阻塞 Tokio worker |
| 被拒绝的调用需要操作指引 | ✅ 正确 | `Reject` 的 message 明确告诉模型 "increase `offset`" |
| loop guard 逻辑应独立于 `agent.rs` | ✅ 正确 | `loop_guard.rs` 已独立，`agent.rs` 只做编排调用 |

## Plan 1 实现状态

**已完成部分：**
- ✅ `LoopGuardState` - turn-scoped 调用轨迹
- ✅ `ToolCallSignature` - 规范化签名（支持 `Read`/`Write`/`Bash`/`Agent`）
- ✅ `evaluate_tool_call` - 重复调用检测（Allow → Warn → Reject 三级）
- ✅ `detect_stalled_iteration` - 无进展 iteration 检测
- ✅ 单元测试覆盖主要路径（6 个 test case）

**阈值设计：**
- `max_consecutive_duplicate_tool_calls: 2` - 第 3 次重复时 Reject
- `max_stalled_iterations: 3` - 连续 3 次相同 (fingerprint, tool_calls_hash) 触发 stall
- `DUPLICATE_WARNING_THRESHOLD: 1` - 第 2 次重复时 Warn

## 发现的问题

### P1 - 高优先级

#### 1. Plan 2 缺少具体实现入口

**问题：** 文档提到 "在 `run_turn_with_context_and_model_config` / `run_turn_with_model_config` 的 iteration 末尾增加增量裁剪钩子"，但当前代码中 `HistoryTrimmer` 只在 `prepare_turn()` 处理。

**影响：** 单 turn 内部的消息膨胀无法被有效抑制。

**建议：** 明确 Plan 2 的实现位置，在 iteration 循环中增加 `trim_iteration_history()` 钩子。

#### 2. `ReadTool` 输出协议缺少 `has_more` / `next_offset`

**问题：** 文档提到 "ReadTool 的输入 schema 和输出协议需要更强的'可继续分页'信号"，但当前 `ReadTool` 返回的是原始文本，没有分页元信息。

**影响：** 模型无法判断是否还有更多内容，可能导致盲目重试或重复读取。

**建议：** 在 `ReadTool` 输出中增加分页元信息：
```rust
pub struct ReadResult {
    pub content: String,
    pub has_more: bool,
    pub next_offset: Option<usize>,
    pub total_lines: usize,
    pub read_range: (usize, usize), // start, end line
}
```

### P2 - 中优先级

#### 3. `ToolCallSignature.input_hash` 字段冗余

**问题：** `input_hash` 在 `evaluate_tool_call` 中没有使用（比较用的是 `PartialEq`），只在 `tool_calls_hash` 中被使用。

**建议：** 明确用途或考虑移除，减少不必要的计算。

#### 4. 缺少结构化观测

**问题：** Plan 3 提到要暴露 `loop_guard_triggered`、`duplicate_read_hit`、`iteration_trimmed` 等指标，但当前实现中没有。

**建议：** 使用 `nova-protocol/src/observability.rs` 或类似机制记录结构化观测信息。

#### 5. `LoopGuardConfig` 缺少配置来源

**问题：** 当前配置是硬编码的默认值，无法动态调整。

**建议：** 考虑从 `gateway` 或运行时配置中读取，以便根据实际场景调整阈值。

### P3 - 低优先级

#### 6. `AssistantFingerprint` 的局限性

**问题：** 只取前 64 字符可能导致误判（前 64 字符相同但后续内容不同）。

**说明：** 对于检测"无进展 iteration"来说，这个精度是足够的。建议在日志中记录 `total_len` 以便调试。

## 风险评估

| 风险 | 严重程度 | 缓解措施 |
|------|---------|---------|
| 过严的重复检测误伤分页读取 | 中 | `ToolCallSignature` 包含 offset/limit，已区分 |
| 只在 Read 侧截断，context 仍膨胀 | 中 | Plan 2 的 iteration 级增量裁剪 |
| assistant 指纹检测 CPU 开销 | 低 | 前 64 字符 hash + 长度，非常轻量 |
| 拒绝执行后模型继续盲目重试 | 中 | Reject message 包含明确操作指引 |
| ReadTool 全局状态污染 | 低 | `LoopGuardState` 在 `AgentRuntime` 内创建 |

## 实施建议

### 推荐实施顺序

1. **Plan 2 实现** - Read 输出收敛 + iteration 级增量裁剪
2. **ReadTool 分页元信息** - 补充 `has_more`、`next_offset` 等字段
3. **结构化观测** - 补充 `loop_guard_triggered`、`duplicate_read_hit` 等指标
4. **配置化** - 将阈值从硬编码改为可配置

### 验证建议

1. 补充集成测试覆盖正常探索、分页读取、重复读取、误判回归和 turn 熔断行为
2. 使用真实 orchestrator 会话进行端到端验证
3. 监控 `loop_guard_triggered` 指标，确认阈值设置合理
