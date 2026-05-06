# Plan 3: 调度执行语义修复

- **前置依赖**：Plan 1、Plan 2
- **状态**：待开始

---

## 本次目标

把编排执行从“能跑部分路径”修到“成功/失败/取消均有一致结果模型和事件模型”，并让 review 输入建立在真实阶段结果上。

**可验证标准：**
- 编排事件通过正式结构化链路发出
- 并行/串行 stage 都能产出完整 `SubAgentResult`
- 子 Agent 失败时仍会发送 `sub_agent_complete` 与 `stage_complete`
- 取消信号能传播到阶段执行和 review 入口

---

## 涉及文件

| 文件 | 操作 | 说明 |
|---|---|---|
| `crates/nova-agent/src/event.rs` | 修改 | 必要时新增结构化编排事件分支 |
| `crates/nova-agent/src/app/types.rs` | 修改 | 若 AppEvent 需要映射，补齐类型 |
| `crates/nova-gateway-core/src/bridge.rs` | 修改 | 将编排事件映射为 `ChatProgress(ProgressEvent)` |
| `crates/nova-agent/src/orchestrator/mod.rs` | 修改 | 统一 emit、review、结果汇总逻辑 |
| `crates/nova-agent/src/orchestrator/scheduler.rs` | 修改 | 失败聚合、取消行为、阶段返回结构 |
| `crates/nova-agent/src/tool/builtin/agent.rs` | 修改 | 子 Agent 生命周期事件走结构化通道 |
| `crates/nova-agent/src/orchestrator/reviewer.rs` | 修改 | 基于真实结果构造 review prompt |

---

## 详细设计

### 1. 结构化发射编排事件

当前问题的根因不是“事件名不对”，而是事件承载层错了。修复方案：

1. 在 agent/app/gateway 三层补一类明确的编排进度事件
2. 统一映射到 `MessageEnvelope::ChatProgress(ProgressEvent { kind, args, ... })`
3. 禁止再通过 `SystemLog("[orchestration] ...")` 或 `LogDelta(stream = "orchestration")` 嵌 JSON

### 2. 调度器返回完整结果

`execute_parallel` / `execute_serial` 不再直接把子任务错误用 `?` 提前抛出，而是转换成：

```rust
SubAgentResult {
    agent_id,
    stage_id,
    status: SubAgentStatus::Failed,
    output: String::new(),
    error: Some(...),
}
```

上层再基于整批 `stage_results` 计算：

- `all_success`
- 是否终止后续 stage
- 是否进入 review

### 3. review 输入只依赖阶段结果

`reviewer.rs` 当前只拼 `output`，且默认都是成功。修复后应：

- 显式带上 `status`
- 失败项带 `error`
- 取消项单独标识

这样 review 才能得出“整体失败但已获得部分结果”的结论。

### 4. `run_in_background` 的定位

当前编排器内部实际使用 `run_in_background = false`，而 `AgentTool` 的后台路径又自己走了一套日志事件。这两条路径会继续制造分叉。

修复决策：

- **短期**：编排主路径统一走同步 `AgentTool.execute()` + 调度器并发，不依赖 `run_in_background`
- **中期**：若保留后台模式，复用同一套结构化事件发射逻辑，不再额外定义日志协议

这样能先把可用性修好，再考虑后台句柄抽象。

---

## 测试案例

### T3-01：并行 stage 成功
- 输入：2 个成功子 Agent
- 预期：收到 2 个 `sub_agent_complete(success)` 和 1 个 `stage_complete(allSuccess=true)`

### T3-02：并行 stage 局部失败
- 输入：1 成功、1 失败
- 预期：两个 agent 均有完成事件，stage 事件为 `allSuccess=false`，上层能得到失败结果集合

### T3-03：串行 stage 首任务失败
- 输入：串行 stage 第 1 个子 Agent 失败
- 预期：后续 agent 不再启动，但当前失败结果被保留并上报

### T3-04：取消传播
- 输入：执行中触发 `CancellationToken`
- 预期：调度器返回已完成部分结果，未完成 agent 标记为 `cancelled` 或安全终止

### T3-05：review 输入完整性
- 输入：混合成功/失败结果
- 预期：review prompt 中同时包含成功输出和失败原因

