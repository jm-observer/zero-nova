# Agent 编排问题分析与优化建议

> **日期**: 2026-05-08  
> **模块**: `crates/nova-agent/src/orchestrator/`  
> **状态**: 待优化

---

## 一、当前编排流程概览

```
用户/LLM → OrchestrateTask → OrchestratorEngine::execute_plan
    ↓
    1. 解析 Plan JSON（stages, agents, depends_on）
    2. 按拓扑顺序执行 stages
    3. 每个 stage 内按 Parallel/Serial 模式执行 agents
    4. 所有 stage 完成后 → run_review（Review Agent 评估结果）
```

---

## 二、问题清单

### 问题 1：Review Agent 的 output_format 未传递

**文件**: `crates/nova-agent/src/orchestrator/mod.rs` → `run_review()`

```rust
let output = self.agent_tool.execute(
    json!({
        "prompt": prompt,
        "description": "Review orchestration outputs",
        "subagent_type": "Reviewer",
        "run_in_background": false,
        // ❌ 缺少 "output_format" 字段
    }),
    self.tool_context.clone(),
).await?;
```

**AgentTool schema 默认值**:
```rust
"output_format": {
    "type": "string",
    "enum": ["full", "summary"],
    "default": "full",  // ← 默认 full
}
```

**影响**: Review Agent 返回的是 **完整输出**（full），而不是摘要（summary）。Review 本身就是对多个 agent 结果的汇总评估，如果使用 full 模式，Review Agent 的输出会非常长，浪费 token 且增加后续解析负担。

---

### 问题 2：Review Agent 的 subagent_type 可能无法正确解析

**文件**: `crates/nova-agent/src/orchestrator/mod.rs` → `run_review()`

传递 `"subagent_type": "Reviewer"`，但 `AgentTool::resolve_agent_spec` 中：

```rust
fn resolve_agent_spec<'a>(&'a self, requested_type: Option<&str>) -> Result<(&'a AgentSpec, Vec<String>)> {
    let requested_type = requested_type.map(str::trim).filter(|value| !value.is_empty());
    if let Some(agent_type) = requested_type {
        if let Some(spec) = self.agent_types.get(agent_type) {
            return Ok((spec, Vec::new()));
        }
    }
    // 如果 "Reviewer" 不在 agent_types 中，会 fallback 到 primary agent
    let fallback = self.agent_types.get(&self.primary_agent_type)...
}
```

**影响**: 如果配置中没有名为 "Reviewer" 的 agent spec，Review Agent 会 fallback 到 primary agent（通常是 "nova"），导致 Review 阶段使用的是普通 Agent 的 prompt template 而不是专门的 Review prompt。

---

### 问题 3：并行阶段缺少超时控制

**文件**: `crates/nova-agent/src/orchestrator/scheduler.rs` → `execute_parallel()`

```rust
async fn execute_parallel<F, Fut>(...) -> Result<Vec<SubAgentResult>> {
    let mut join_set = JoinSet::new();
    // ...
    for agent in &stage.agents {
        // 所有 agent 同时启动，但没有单个 agent 的超时
        join_set.spawn(async move { (agent_id, runner(req, stage_id).await) });
    }
    // ...
    tokio::select! {
        _ = cancellation_token.cancelled() => { ... }  // 只有全局取消
        joined = join_set.join_next() => { ... }
    }
}
```

**影响**:
- 如果某个 agent 卡住（LLM 响应慢、工具调用死锁），整个并行阶段会一直等待
- 只有全局 `cancellation_token` 能终止，但单个 agent 超时无法被感知
- 并行阶段没有设置超时上限

---

### 问题 4：Serial 阶段失败后的 short-circuit 行为不一致

**文件**: `crates/nova-agent/src/orchestrator/scheduler.rs` → `execute_serial()`

```rust
if failed || cancelled {
    // 剩余 agents 被标记为 Cancelled，但不是 Failed
    append_cancelled_results(plan_id, &stage.stage_id, &stage.agents[index + 1..], &mut results);
    break;
}
```

**影响**:
- 第一个 agent 失败 → 后续 agents 被标记为 `Cancelled`
- 但 `stage_success` 的判断是 `all_success`，Cancelled 也算失败 ✅
- **问题在于**：前端/UI 可能无法区分 "agent 自己失败了" 和 "agent 被取消"

---

### 问题 5：Review 阶段 JSON 解析失败无重试

**文件**: `crates/nova-agent/src/orchestrator/reviewer.rs` → `parse_review_result()`

```rust
pub fn parse_review_result(raw: &str) -> Result<ReviewResult> {
    serde_json::from_str(raw).context("failed to parse review result JSON")
}
```

**影响**: 如果 LLM 返回的 JSON 格式有误（例如缺少引号、多了一个逗号），整个编排会直接失败，没有重试机制。

---

### 问题 6：Sub-agent 输出解析过于脆弱

**文件**: `crates/nova-agent/src/orchestrator/mod.rs` → `execute_stage` 闭包

```rust
let parsed: serde_json::Value = serde_json::from_str(&output.content)
    .map_err(|e| anyhow!("failed to parse sub-agent output: {}", e))?;
let content = parsed
    .get("output")
    .and_then(|v| v.as_str())
    .unwrap_or_default()  // ← 如果 "output" 字段不存在，返回空字符串
    .to_string();
```

**影响**:
- 如果 sub-agent 返回的 JSON 中没有 `output` 字段（例如 LLM 返回纯文本而非 JSON），会静默返回空字符串
- 空字符串会被当作成功结果，Review Agent 可能无法正确评估

---

### 问题 7：System Prompt 在每次 sub-agent 调用时重新生成

**文件**: `crates/nova-agent/src/tool/builtin/agent.rs` → `run_subagent()`

```rust
let mut system_prompt = spec.system_prompt_template.clone().unwrap_or_default();
if system_prompt.is_empty() {
    system_prompt = "You are a helpful assistant.".to_string();
}

let history = vec![Message::new(
    Role::System,
    vec![ContentBlock::Text { text: system_prompt }],
    chrono::Utc::now().timestamp_millis(),
)];
```

**影响**:
- 每个 sub-agent 调用都是独立的，没有共享上下文
- 如果编排需要 agent 之间传递信息（例如 agent-1 的结果作为 agent-2 的输入），必须通过 `prompt` 字段显式传递
- 对于需要多轮对话的编排场景（例如 agent 内部需要多次迭代），`max_iterations` 配置是唯一的控制手段

---

## 三、优化建议汇总

| 优先级 | 问题 | 建议方案 |
|--------|------|----------|
| **P0** | Review Agent output_format | 在 `run_review` 中显式传递 `"output_format": "summary"` |
| **P0** | Review Agent subagent_type | 确保配置中有 "Reviewer" agent spec，或 fallback 逻辑更明确 |
| **P1** | 并行阶段超时 | 为每个 agent 添加独立的 `tokio::time::timeout` |
| **P1** | Review JSON 解析重试 | 添加 1-2 次重试，每次重试前尝试清理/格式化 LLM 输出 |
| **P2** | Sub-agent 输出解析 | 增加 fallback：如果 JSON 解析失败，尝试将原始文本作为 output |
| **P2** | Serial 阶段状态区分 | 考虑新增 `SubAgentStatus::Skipped` 区分 "被取消" 和 "自己失败" |

---

## 四、相关文件索引

| 文件 | 职责 |
|------|------|
| `crates/nova-agent/src/orchestrator/mod.rs` | OrchestratorEngine 主逻辑 |
| `crates/nova-agent/src/orchestrator/planner.rs` | Plan 解析与验证 |
| `crates/nova-agent/src/orchestrator/scheduler.rs` | Stage 执行（Parallel/Serial） |
| `crates/nova-agent/src/orchestrator/reviewer.rs` | Review Agent prompt 构建与结果解析 |
| `crates/nova-agent/src/tool/builtin/agent.rs` | AgentTool 子代理执行 |
| `crates/nova-agent/src/tool/builtin/orchestrate_task.rs` | OrchestrateTask 入口工具 |
