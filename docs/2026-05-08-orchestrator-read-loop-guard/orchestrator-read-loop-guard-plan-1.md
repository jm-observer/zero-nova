# Plan 1: 单轮循环检测与熔断

| 章节 | 内容 |
|---|---|
| Plan 编号与标题 | Plan 1: 单轮循环检测与熔断 |
| 前置依赖 | 无 |
| 本次目标 | 在单个 turn 内识别“连续重复的相同工具调用”和“多轮无进展模式”，在不影响正常探索的前提下阻断最常见的重复 `Read` 循环。 |
| 涉及文件 | `crates/nova-agent/src/agent.rs`、`crates/nova-agent/src/tool.rs`、`crates/nova-agent/src/event.rs`、新增 `crates/nova-agent/src/loop_guard.rs` |

## 详细设计

### 1. 设计原则

1. 检测范围限定在 **单个 turn** 内，不跨 session 持久化，避免把用户多轮正常回访误判为循环。
2. 首版优先使用 **规则检测**，不引入相似度模型或复杂 NLP。
3. 检测逻辑不直接绑死在 `Read`，而是对所有 tool call 通用；但 `Read` 会是重点命中对象。
4. 为了满足项目的文件长度与单一职责约束，loop guard 实现必须落在独立模块中，不能继续堆进 `agent.rs`。

### 2. 核心数据结构

建议引入 turn-scoped 的 `LoopGuardState`，可挂在 `run_turn_*` 的局部变量或独立模块中。

建议字段：

- `recent_calls: VecDeque<ToolCallSignature>`
- `consecutive_duplicate_count: usize`
- `stalled_iteration_count: usize`
- `last_assistant_fingerprint: Option<AssistantFingerprint>`

其中 `ToolCallSignature` 至少包括：

- `tool_name`
- `canonical_primary_target`
- `normalized_input`
- `input_hash`

`canonical_primary_target` 对 `Read` / `Edit` / `Write` 可是规范化文件路径；对 `Bash` 可为空；对 `Agent` 可是子 agent id 或 prompt hash。

在 `agent.rs` 中只保留简短调用，例如：

- 初始化 `LoopGuardState`
- 生成 assistant 指纹
- 调用 `loop_guard::check(...)`
- 根据返回结果决定继续执行、注入 warning 或构造 reject 结果

签名规范化、计数更新和 reject 文案拼装都应封装在独立模块中。

### 3. 规范化规则

针对 `Read`：

- 规范化 `file_path`
- 保留 `offset`、`limit`
- 去掉无关字段顺序差异

针对 `Bash`：

- 仅做字符串 trim，不尝试语义解析

针对 `Agent`：

- 使用 `subagent_type` / `agent_selection`
- 对 `prompt` 做哈希，不直接持有全文

### 4. 命中策略

首版建议采用两级策略：

1. **相同 tool call 连续重复**
   - 若与上一次签名完全一致，则 `consecutive_duplicate_count += 1`
   - 否则清零

2. **多轮无进展**
   - 当前轮 assistant 指纹与上一轮一致；
   - 且 tool call 签名也未变化；
   - 连续达到阈值后，判定为 stalled turn。

assistant 指纹首版严格采用轻量启发式：

- 文本前 64 个字符的 hash
- 文本总长度

不使用 Levenshtein、diff 或其他高 CPU 开销算法。

### 5. 处理策略

建议新增三种处理结果：

- `Allow`
- `AllowWithWarning { message }`
- `Reject { message, reason_code }`

执行顺序：

1. LLM 产出 tool call
2. runtime 生成签名
3. loop guard 返回决策
4. `Allow` 继续执行
5. `AllowWithWarning` 将 warning 注入 tool result 或 system log
6. `Reject` 直接构造受控 tool result，不触发真实工具执行

`Reject` 返回文案必须具备指导性。建议采用如下语义：

```text
System Guard: You have repeated the exact same read request multiple times.
Please stop reading the same offset and either increase your `offset` parameter
to inspect a new range, or continue the analysis based on what you have already learned.
```

目标是让模型知道“为什么被拦截”以及“下一步可执行动作”。

### 6. 事件与日志

建议增加结构化日志字段：

- `tool`
- `reason`
- `duplicate_count`
- `stalled_iteration_count`
- `signature_hash`

如已有事件体系允许，可新增 `AgentEvent::LoopGuardTriggered`；若暂不改协议，至少写入 `SystemLog`。

## 测试案例

1. 连续两次完全相同的 `Read(file, offset=1, limit=200)`：第一次允许，第二次产生 warning。
2. 连续三次完全相同的 `Read`：达到阈值后拒绝执行。
3. 同一文件不同 `offset` 的分页读取：不判定为重复。
4. assistant 文本重复但 tool call 不同：不触发 stalled turn。
5. assistant 文本和 tool call 都重复，连续多轮无变化：触发 turn 熔断。
6. 非 `Read` 工具如 `Bash` 连续重复：同样能命中通用保护。
7. `Reject` 文案断言：返回内容中明确包含“停止重复读取”和“提高 offset 或继续分析”的指导语。
