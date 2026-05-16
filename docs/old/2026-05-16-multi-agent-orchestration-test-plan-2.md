# Plan 2：OrchestratorEngine 测试

- **Plan 编号**：2
- **前置依赖**：Plan 1（SubAgentExecutor trait）
- **本次目标**：为 OrchestratorEngine 编写完整的单元测试，覆盖所有代码路径

## 涉及文件

| 文件 | 变更类型 |
|------|---------|
| `crates/nova-agent/src/orchestrator/mod.rs` | 新增 `#[cfg(test)] mod tests` |

## 测试基础设施

### MockExecutor

```rust
struct MockExecutor {
    /// agent_id → 预设响应。Ok(output_text) 表示成功，Err(msg) 表示失败。
    responses: HashMap<String, Result<String, String>>,
    /// 模拟 catalog 中的 agent ID 集合。
    catalog: HashSet<String>,
    /// 默认 agent ID。
    default_id: String,
}
```

`execute_agent` 实现：
1. 从 input JSON 解析出 `agent_id`（如果有）或 `description`
2. 查 `responses` 表，匹配到则返回对应结果
3. 成功时返回 `ToolOutput { content: json!({"output": text}).to_string() }`
4. 失败时返回 `Err(anyhow!(msg))`
5. 未匹配时返回默认成功响应（空 output）

### 事件收集器

```rust
fn collect_events(rx: &mut mpsc::Receiver<AgentEvent>) -> Vec<String> {
    // 从 channel 读取所有已发送的事件，解析出 kind 字段
}
```

### Plan JSON 构建器

```rust
fn build_plan_json(stages: Vec<StageSpec>) -> String {
    // 便捷方法，根据 StageSpec 列表生成合法的 plan JSON
}
```

## 详细测试用例

### A. 辅助方法（6 个测试）

这些测试不需要 async runtime，直接测试 Engine 的同步方法。

#### A1. `validate_agent_selection_known_agent`

- **Setup**：catalog = {"nova", "developer"}，default = "nova"
- **Input**：selection = "developer"
- **Assert**：返回 ("developer", None)

#### A2. `validate_agent_selection_unknown_agent`

- **Setup**：catalog = {"nova"}，default = "nova"
- **Input**：selection = "unknown-agent"
- **Assert**：返回 ("nova", Some(warning))，warning 包含 "not found in catalog"

#### A3. `review_agent_id_with_reviewer`

- **Setup**：catalog = {"nova", "reviewer"}
- **Assert**：`review_agent_id()` 返回 "reviewer"

#### A4. `review_agent_id_without_reviewer`

- **Setup**：catalog = {"nova"}，default = "nova"
- **Assert**：`review_agent_id()` 返回 "nova"

#### A5. `parse_plan_valid`

- **Input**：合法 2-stage plan JSON
- **Assert**：返回 Ok，plan_id 和 stage 数量正确

#### A6. `parse_plan_invalid`

- **Input**：`"not valid json"`
- **Assert**：返回 Err

### B. execute_plan 核心路径（7 个测试）

所有测试使用 `#[tokio::test]`，构造 `MockExecutor` + event channel。

#### B1. `single_stage_parallel_all_success`

- **Plan**：1 个 parallel stage，agent a1 + a2
- **Mock**：a1 → Ok("output-a1")，a2 → Ok("output-a2")，Reviewer → Ok(ReviewResult JSON with success=true)
- **Assert**：
  - `outcome.results` 含 a1、a2，均为 Success
  - `outcome.review` 为 Some，success=true
  - 事件序列：`orchestration_plan` → `sub_agent_spawn`×2 → `sub_agent_complete`×2 → `stage_complete` → `orchestration_review_start` → `orchestration_complete`
  - `orchestration_complete` 的 overall_success=true

#### B2. `single_stage_serial_all_success`

- **Plan**：1 个 serial stage，agent a1 + a2
- **Mock**：同 B1
- **Assert**：与 B1 类似，验证 serial 模式下事件同样正确

#### B3. `two_stage_serial_dependency`

- **Plan**：s1(parallel, a1) → s2(serial, a2, dependsOn=["s1"])
- **Mock**：a1 → Ok，a2 → Ok，Reviewer → Ok(success=true)
- **Assert**：
  - 事件中 s1 的 spawn/complete 在 s2 的 spawn 之前
  - 两个 stage 都有 stage_complete 事件
  - overall_success=true

#### B4. `dependency_failure_blocks_downstream`

- **Plan**：s1(parallel, a1) → s2(serial, a2, dependsOn=["s1"])
- **Mock**：a1 → Err("boom")
- **Assert**：
  - s1 的 stage_complete allSuccess=false
  - s2 未执行（无 s2 的 spawn 事件）
  - orchestration_complete 包含 "blocked by dependency"
  - overall_success=false
  - review 为 None

#### B5. `partial_stage_failure_stops_orchestration`

- **Plan**：s1(parallel, a1+a2) → s2(serial, a3, dependsOn=["s1"])
- **Mock**：a1 → Ok，a2 → Err("fail")
- **Assert**：
  - s1 的 stage_complete allSuccess=false
  - s2 不执行
  - orchestration 结束，没有 review

#### B6. `cancellation_before_review`

- **Plan**：1 个 parallel stage，a1
- **Mock**：a1 → Ok
- **Setup**：在 execute_plan 开始后、review 前取消 token
- **实现方式**：MockExecutor 在返回成功结果时同时触发 cancellation_token.cancel()
- **Assert**：
  - orchestration_complete 包含 "cancelled before review"
  - overall_success=false
  - review 为 None

#### B7. `empty_plan_no_stages`

- **Plan**：plan_id="test"，stages=[]
- **Assert**：
  - results 为空
  - orchestration_complete 包含 "No stages"
  - overall_success=true
  - review 为 None

### C. 事件发射（2 个测试）

#### C1. `event_sequence_single_stage`

- **Plan**：1 个 parallel stage，agent a1
- **Assert**：精确匹配事件 kind 序列：
  1. `orchestration_plan`
  2. `sub_agent_spawn`
  3. `sub_agent_complete`
  4. `stage_complete`
  5. `orchestration_review_start`
  6. `orchestration_complete`

#### C2. `event_args_contain_correct_ids`

- **Plan**：plan_id="p1"，stage_id="s1"，agent_id="a1"
- **Assert**：每个事件的 args JSON 中 plan_id / stage_id / agent_id 与 plan 定义一致

### D. rewire_log_forwarding（3 个测试）

#### D1. `log_delta_forwarded_as_orchestration_progress`

- **Setup**：构造 ToolContext（含 event_tx），调用 rewire_log_forwarding 获得新 ctx
- **Action**：向新 ctx.event_tx 发送 `AgentEvent::LogDelta { log: "hello", stream: "stdout", ... }`
- **Assert**：
  - orchestrator_tx 收到包含 `sub_agent_log` 的 SystemLog 事件
  - original_tx 也收到原始 LogDelta 事件

#### D2. `non_log_events_passthrough_only`

- **Setup**：同 D1
- **Action**：发送 `AgentEvent::TextDelta { ... }`
- **Assert**：
  - original_tx 收到 TextDelta
  - orchestrator_tx 不收到任何事件

#### D3. `none_context_returns_none`

- **Action**：`rewire_log_forwarding(None, tx, ...)`
- **Assert**：返回 None

## 测试数据

### ReviewResult JSON 模板

```json
{
  "success": true,
  "issues": [],
  "retryAgents": [],
  "summary": "All agents completed successfully."
}
```

MockExecutor 在收到 Reviewer 类型的请求时返回此模板（可按测试需要调整 success/issues）。

## 约束

- 所有测试使用 `#[tokio::test]`
- 不依赖文件系统（除非测试 rewire_log_forwarding 需要临时 ToolContext）
- 事件断言通过解析 `AgentEvent::SystemLog` 字符串中的 `kind=` 字段
- MockExecutor 通过 `agent_id` 字段路由响应；Reviewer 请求通过 `agent_selection` 或 `subagent_type` 包含 "Reviewer" 来识别
