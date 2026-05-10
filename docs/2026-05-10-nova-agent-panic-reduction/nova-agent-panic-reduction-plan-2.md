# Plan 2: ConversationService 关键路径 panic 点治理

## Plan 编号与标题
- Plan 2: ConversationService 关键路径 panic 点治理

## 前置依赖
- 建议依赖 Plan 1（统一底层序列化错误策略）

## 本次目标
- 去除 `crates/nova-agent/src/app/conversation_service.rs` 生产路径中的 `unwrap`。
- 明确锁读取失败（中毒）与快照序列化失败时的错误传播边界。

## 涉及文件
- `crates/nova-agent/src/app/conversation_service.rs`

## 详细设计
1. 目标点清单（生产代码）
- `resolve_run_models()`：`session.control.read().unwrap()`。
- `execute_agent_turn()` 快照构建：`serde_json::to_value(...).unwrap()`（prompt/tools/skills 三处）。

2. 处理策略
- 锁读取：改为
  - `session.control.read().map_err(|e| anyhow!(...)).context("failed to read session control lock")?`
  - 错误信息需包含 `session_id` 与调用阶段（如 `resolve_run_models`）。
- 快照序列化：将 `map(|x| serde_json::to_value(x).unwrap())` 改为可失败收集（如 `collect::<Result<Vec<_>, _>>()`）并 `.context("...")?`。
- 对 `prompt_preview` 的 `Option` 序列化采用 `transpose()`，避免局部 panic。

3. 行为约束
- 失败时应更新 run 状态为 `failed`（保持现有异常路径语义）。
- 不新增 `println!`，日志继续使用 `log` 宏。

## 测试案例
- 正常路径：现有 turn 执行流程行为不变（消息落库、usage 更新、run success）。
- 异常路径：模拟快照序列化失败时，turn 返回错误且 run 状态进入 failed。
- 并发路径：并发读写控制状态场景下不引入死锁，且锁错误可观测。