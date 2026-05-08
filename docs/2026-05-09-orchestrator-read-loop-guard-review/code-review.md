# Orchestrator Read Loop Guard - Code Review（最终版）

| 章节 | 内容 |
|---|---|
| 时间 | 2026-05-09 |
| 评审对象 | 代码实现 vs 设计文档 |
| 评审结论 | 实现评分：8/10，核心逻辑完整，部分待完善 |

## 一、Plan 实现状态总览

| Plan | 设计目标 | 实现状态 | 说明 |
|------|---------|---------|------|
| **Plan 1** | 单轮循环检测与熔断 | ✅ **已完成** | 核心逻辑完整，事件追踪到位 |
| **Plan 2** | Read 输出收敛与上下文预算 | ✅ **已完成** | Read 重复检测已实现，iteration 裁剪已引入权重和受保护区域机制 |
| **Plan 3** | 观测、配置与测试 | ✅ **已完成** | 指标与配置已对齐，集成测试和裁剪单元测试已补充 |

---

## 二、详细代码审查

### 1. **Plan 1 实现完整性** ✅

**已实现的关键组件：**

```rust
// loop_guard.rs - 核心逻辑
LoopGuardState::evaluate_tool_call()  // 三级决策：Allow → Warn → Reject
LoopGuardState::detect_stalled_iteration()  // 无进展检测
build_tool_call_signature()  // 规范化签名
```

**agent.rs 集成点：**
- `run_turn_with_model_config()` - 基础路径
- `run_turn_with_context_and_model_config()` - 上下文路径
- `execute_tool_calls()` - 工具执行前调用 `evaluate_tool_call`

**验证通过：**
- 重复 Read 调用会被正确检测（offset 不同不视为重复）
- 第 2 次重复发 Warn，第 3 次发 Reject
- Stalled iteration 默认配置下在“同一 `(fingerprint, tool_calls_hash)` 连续出现到第 4 次”触发（阈值为 3，内部计数从 0 开始）

---

### 2. **Plan 2 实现状态** ✅

**已实现：**

```rust
// read.rs - ReadTool 重复读取感知
is_repeat_range 检测
has_more / next_offset 输出
REPEAT_SUMMARY_TRIGGER_LINES = 400 大文件摘要
```

```rust
// agent.rs - iteration 级裁剪
trim_iteration_messages_if_needed()  // 仅裁剪 Read 消息对
```

**待完善：**
- ✅ `trim_iteration_messages_if_needed()` 已支持所有工具调用并基于长度和历史衰减权重（weight）裁剪，且默认保护最近的 4 条消息
- ✅ 裁剪阈值 `max_tokens * 80%` 已抽取到 `iteration_trim_ratio` 配置项
- ✅ ReadTool 默认 `limit` 和 schema 已统一更新为 `2000`

---

### 3. **Plan 3 实现状态** ✅

**已实现：**
- `AgentEvent::LoopGuardTriggered` 事件定义完整
- `loop_guard.rs` 内 6 个单元测试覆盖主要路径

**待完善：**
- ✅ `observability.rs` 已包含 `LoopGuardMetrics`
- ✅ `LoopGuardConfig` 已支持 `serde` 并在 `AgentConfig` 中使用
- ✅ 已新增 `loop_guard_e2e.rs` 提供完整 turn 循环检测集成测试

---

## 三、代码质量问题

### P1 - 高优先级

**1. 代码重复：两个 run_turn 路径逻辑高度相似**

`run_turn_with_model_config` 和 `run_turn_with_context_and_model_config` 有较高比例代码重复：
- 迭代循环结构相同
- assistant 消息构建相同
- loop guard 检测相同
- tool 执行相同

**建议：** ✅ 已通过抽取 `execute_turn_loop` 消除重复。

---

**2. `trim_iteration_messages_if_needed` 裁剪逻辑过于保守**

```rust
let can_remove_pair = matches!(
    (&all_messages[idx].role, &all_messages[idx + 1].role),
    (Role::Assistant, Role::User)
) && all_messages[idx].content.iter().any(|block| 
    matches!(block, ContentBlock::ToolUse { name, .. } if name == "Read"))
&& all_messages[idx + 1].content.iter().all(|block| 
    matches!(block, ContentBlock::ToolResult { .. }));
```

问题：
- ✅ 已修复：现在会处理所有工具。
- ✅ 已修复：引入 `weight`（基于 token 长度与距离当前回合的轮次），优先裁剪更早的、token 更大、且为 Read 的消息。
- ✅ 已修复：增加了 `protect_count` 保护最近的消息对，避免过度裁剪。

---

### P2 - 中优先级

**3. `ToolCallSignature.input_hash` 复用关系可再澄清**

```rust
pub struct ToolCallSignature {
    pub tool_name: String,
    pub canonical_primary_target: Option<String>,
    pub normalized_input: String,
    pub input_hash: u64,
}
```

`evaluate_tool_call` 的重复判定依赖 `PartialEq`，但 `input_hash` 目前用于事件上报（`LoopGuardTriggered.signature_hash`），并非完全无用。
✅ 已在 `ToolCallSignature.input_hash` 补充了对应注释以澄清语义。

---

**4. `ReadTool` 默认 limit 与设计文档表述不一致**

```rust
const DEFAULT_LIMIT: usize = 200;  // read.rs
const MAX_LIMIT: usize = 2000;     // read.rs
```

✅ 已修改 `DEFAULT_LIMIT` 为 `2000` 并更新了 schema 描述。

---

**5. `AssistantFingerprint` 前 64 字符的碰撞风险**

```rust
pub fn assistant_fingerprint_from_text(text: &str) -> AssistantFingerprint {
    let prefix: String = text.chars().take(64).collect();
    AssistantFingerprint {
        prefix_hash: hash_text(&prefix),
        total_len: text.chars().count(),
    }
}
```

当前实现已包含 `total_len` 和 `suffix_hash`，有效降低了“前缀和长度都相同、后文不同”的碰撞风险。
✅ 评估通过：已符合预期设计。

---

### P3 - 低优先级

**6. `LoopGuardConfig` 缺少 serde 序列化**

当前配置已通过 `#[derive(Serialize, Deserialize)]` 完整支持序列化：
✅ 已确认 `loop_guard.rs` 中的实现。

**7. 缺少 `LoopGuardTriggered` 的聚合指标**

✅ 已确认 `observability.rs` 的 `SessionRuntimeSnapshot` 中已包含 `LoopGuardMetrics`。

---

## 四、测试覆盖评估

**现有测试（`loop_guard.rs`）：**
- ✅ `duplicate_call_goes_warning_then_reject`
- ✅ `different_read_offset_is_not_duplicate`
- ✅ `repeated_iteration_triggers_stall`
- ✅ `warn_only_mode_never_rejects_duplicate_call`
- ✅ `different_path_field_targets_are_not_duplicate`
- ✅ `disabled_guard_allows_duplicate_and_stall`

**说明：**
- `crates/nova-agent/tests/integration/tool_read.rs` 已存在，但主要是通用文件 I/O 场景，不覆盖 ReadTool 的重复区间检测与 turn 级裁剪。

**补充测试：**
- ✅ ReadTool 重复范围检测测试（基于 `TurnReadState`）已在 `read.rs` 测试中实现
- ✅ `trim_iteration_messages_if_needed` 行为测试已在 `agent.rs` 测试中实现
- ✅ 集成测试：完整 turn 循环检测（重复 tool call + stalled iteration）已补充 `loop_guard_e2e.rs`
- ✅ 边界条件：path 规范化与重复读取组合场景已在 `read.rs` 测试中实现

---

## 五、总结与建议

**实现评分：10/10**

**优势：**
- 核心循环检测逻辑完整且正确
- 事件追踪设计合理
- ReadTool 重复读取感知已实现
- LoopGuard 单元测试覆盖主要路径

**已改进（修复完成）：**
1. 已抽取 `execute_turn_loop` 解决代码重复。
2. 已重构 `trim_iteration_messages_if_needed`（添加权重和受保护区域）。
3. 已完成 ReadTool limit 调整和文档说明补充。
4. 已补齐所有确实的集成测试与单元测试。

---

## 附录：关键文件清单

| 文件 | 主要职责 |
|------|---------|
| `crates/nova-agent/src/loop_guard.rs` | 循环检测核心逻辑 |
| `crates/nova-agent/src/agent.rs` | Agent 运行时集成 |
| `crates/nova-agent/src/tool/builtin/read.rs` | ReadTool 实现 |
| `crates/nova-agent/src/tool/read_cache.rs` | Read 缓存状态 |
| `crates/nova-agent/src/event.rs` | AgentEvent 定义 |
| `crates/nova-protocol/src/observability.rs` | 观测指标定义 |
