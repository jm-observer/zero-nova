# Plan 3: Agent 主循环隐式 panic 假设收敛

## Plan 编号与标题
- Plan 3: Agent 主循环隐式 panic 假设收敛

## 前置依赖
- 建议依赖 Plan 2（上层错误边界已统一）

## 本次目标
- 消除 `crates/nova-agent/src/agent.rs` 关键路径 `unwrap` 及“依赖前置条件的隐式安全假设”。
- 保持现有流式事件边界与 tool-call 续写语义。

## 涉及文件
- `crates/nova-agent/src/agent.rs`

## 详细设计
1. 目标点清单（生产代码）
- `parsed_tool_calls.last().unwrap()`（`StopReason::MaxTokens` 分支）。
- `Arc::try_unwrap(ctx.history).unwrap_or_else(...)`（该点非 panic，但可读性上存在“成功/失败分支语义混杂”，建议保持并补注释）。

2. 处理策略
- `last().unwrap()` 改为显式匹配：
  - `if let Some((_, _, last_val)) = parsed_tool_calls.last() { ... } else { ... }`。
  - 在理论不应到达但可达场景下，采用保守策略：视作 `is_truncated = true` 并触发 continuation user message，禁止 panic。
- 对 `Arc::try_unwrap` 保持现状，但补充注释说明该分支仅为性能优化，失败分支 clone 是预期行为。

3. 一致性要求
- 不改变循环保护（LoopGuard）逻辑与 tool result 压缩行为。
- 不改变 usage 统计合并规则。

## 测试案例
- 正常路径：包含 tool_calls 的 `MaxTokens` 响应继续按原逻辑判断截断续写。
- 边界路径：`MaxTokens + parsed_tool_calls 为空` 时不 panic，进入 continuation 路径。
- 回归路径：agent 现有单测全部通过；新增针对空 tool_calls 边界的单测。