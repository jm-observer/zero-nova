# Plan 3：Scheduler / Planner / Reviewer 测试补全

- **Plan 编号**：3
- **前置依赖**：无（与 Plan 1/2 可并行）
- **本次目标**：补全 scheduler、planner、reviewer 三个模块的测试覆盖

## 涉及文件

| 文件 | 变更类型 |
|------|---------|
| `crates/nova-agent/src/orchestrator/scheduler.rs` | 扩充 `#[cfg(test)] mod tests` |
| `crates/nova-agent/src/orchestrator/planner.rs` | 扩充 `#[cfg(test)] mod tests` |
| `crates/nova-agent/src/orchestrator/reviewer.rs` | 扩充 `#[cfg(test)] mod tests` |

---

## Scheduler 补全（7 个测试）

现有 3 个测试覆盖：parallel 失败保留、serial 失败取消剩余、pre-cancelled parallel。
复用现有的 `build_stage` helper 和 `init_test_logger`。

### S1. `parallel_all_success`

- **Setup**：2 个 agent 的 parallel stage
- **Mock**：两个 agent 都返回 Ok(Success)
- **Assert**：results.len()=2，两个都是 Success

### S2. `serial_all_success`

- **Setup**：2 个 agent 的 serial stage
- **Mock**：两个都返回 Ok(Success)
- **Assert**：results.len()=2，两个都是 Success
- **额外验证**：通过 AtomicUsize 计数器验证调用顺序（a1 先于 a2）

### S3. `serial_error_path`

- **Setup**：2 个 agent 的 serial stage
- **Mock**：a1 返回 Err("error msg")（注意：不是 Ok(Failed)，而是 Err）
- **Assert**：
  - results[0].status = Failed，error 包含 "error msg"
  - results[1].status = Cancelled

### S4. `serial_cancellation_mid_execution`

- **Setup**：2 个 agent 的 serial stage
- **Mock**：a1 返回 Ok(Success)，但 execute_agent 闭包在 a1 完成后 cancel token
- **Assert**：
  - results[0].status = Success
  - results[1].status = Cancelled

### S5. `empty_stage_zero_agents`

- **Setup**：parallel stage，agents = []
- **Assert**：results 为空 vec，不 panic

### S6. `single_agent_parallel`

- **Setup**：parallel stage，1 个 agent
- **Mock**：返回 Ok(Success)
- **Assert**：results.len()=1，status=Success

### S7. `single_agent_serial`

- **Setup**：serial stage，1 个 agent
- **Mock**：返回 Ok(Success)
- **Assert**：results.len()=1，status=Success

---

## Planner 补全（8 个测试）

现有 3 个测试覆盖：unknown dependency、cycle、default subagent type。

### P1. `valid_multi_stage_topological_sort`

- **Input**：3 个 stage 的链式依赖 s1→s2→s3
- **Assert**：
  - parse_and_validate 返回 Ok
  - stages 顺序为 [s1, s2, s3]

### P2. `duplicate_stage_ids_rejected`

- **Input**：两个 stage 都用 stage_id="s1"
- **Assert**：返回 Err，错误信息包含 "duplicate" 或 stage id

### P3. `duplicate_agent_ids_rejected`

- **Input**：s1 含 agent a1，s2 也含 agent a1
- **Assert**：返回 Err，错误信息包含 "duplicate" 或 agent id

### P4. `empty_plan_id_rejected`

- **Input**：plan_id="" 或 plan_id="   "
- **Assert**：返回 Err

### P5. `diamond_dependency_sorts_correctly`

- **Input**：4 个 stage 形成菱形依赖
  - s1 无依赖
  - s2 dependsOn=[s1]
  - s3 dependsOn=[s1]
  - s4 dependsOn=[s2, s3]
- **Assert**：
  - parse_and_validate 返回 Ok
  - s1 排在 s2 和 s3 前面
  - s4 排在 s2 和 s3 后面

### P6. `invalid_json_rejected`

- **Input**：`"{{not json}}"`
- **Assert**：返回 Err

### P7. `agent_selection_field_deserialized`

- **Input**：agent JSON 含 `"agentSelection": "developer"`
- **Assert**：解析后 agent.agent_selection = Some("developer")

### P8. `stage_mode_as_str`

- **Assert**：`StageMode::Parallel.as_str()` = "parallel"，`StageMode::Serial.as_str()` = "serial"

---

## Reviewer 补全（8 个测试）

现有 1 个测试覆盖：review prompt 包含 failure details。

### R1. `review_prompt_success_agents`

- **Input**：1 个 Success agent，output="good result"
- **Assert**：prompt 包含 "Status: success" 和 "good result"

### R2. `review_prompt_cancelled_agents`

- **Input**：1 个 Cancelled agent
- **Assert**：prompt 包含 "Status: cancelled"

### R3. `review_prompt_multiple_agents`

- **Input**：3 个 agent（1 Success，1 Failed，1 Cancelled）
- **Assert**：prompt 包含 3 段 agent 信息，用 "---" 分隔

### R4. `review_prompt_empty_output`

- **Input**：1 个 Success agent，output=""
- **Assert**：prompt 包含 `<empty>` 占位符

### R5. `parse_review_result_valid`

- **Input**：
  ```json
  {"success": true, "issues": [], "retryAgents": [], "summary": "All good."}
  ```
- **Assert**：
  - result.success = true
  - result.issues 为空
  - result.retry_agents 为空
  - result.summary = "All good."

### R6. `parse_review_result_invalid`

- **Input**：`"not json at all"`
- **Assert**：返回 Err

### R7. `parse_review_result_with_issues`

- **Input**：
  ```json
  {
    "success": false,
    "issues": ["missing file", "wrong format"],
    "retryAgents": ["a2"],
    "summary": "Partial failure."
  }
  ```
- **Assert**：
  - result.success = false
  - result.issues.len() = 2
  - result.retry_agents = ["a2"]

### R8. `review_prompt_empty_results`

- **Input**：空 HashMap
- **Assert**：不 panic，返回合理的 prompt 字符串

---

## 测试数据复用

### build_stage helper 扩展

在 scheduler tests 中扩展现有 `build_stage`，增加变体：

```rust
fn build_empty_stage(mode: StageMode) -> ExecutionStage { ... }
fn build_single_agent_stage(mode: StageMode) -> ExecutionStage { ... }
```

### SubAgentResult 工厂

```rust
fn success_result(plan_id: &str, stage_id: &str, agent_id: &str, output: &str) -> SubAgentResult { ... }
fn failed_result_with_error(plan_id: &str, stage_id: &str, agent_id: &str, error: &str) -> SubAgentResult { ... }
fn cancelled_result(plan_id: &str, stage_id: &str, agent_id: &str) -> SubAgentResult { ... }
```

### Plan JSON 模板

```rust
fn plan_json_chain(stage_count: usize) -> String { ... }
fn plan_json_diamond() -> String { ... }
```

## 约束

- 所有新测试放在对应模块的 `#[cfg(test)] mod tests` 中
- 复用现有的 `init_test_logger()` 模式
- 不引入新的外部依赖
