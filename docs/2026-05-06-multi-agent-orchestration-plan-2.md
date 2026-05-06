# Plan 2: Orchestrator 核心逻辑

- **前置依赖**：Plan 1（协议与数据模型）
- **状态**：待实施

---

## 本次目标

1. 实现 `AgentTool` 的 `run_in_background`，支持真正的并行子 Agent 执行
2. 实现 `OrchestratorEngine`：接收拆分计划 JSON → 构建 DAG → 按阶段调度子 Agent → 收集结果
3. 实现 Review Agent 触发逻辑
4. 整合到现有 `AgentRuntime` / `ConversationService` 路径

**可验证标准：**
- 2 个并行子 Agent 的 `run_in_background=true` 调用实际并发执行，总时间 ≤ max(子任务时间) + 开销
- 串行依赖的子 Agent 在前驱完成后才启动
- Review Agent 收到所有子任务的 `output_summary`
- 某子 Agent 失败时，Orchestrator 收到错误，不崩溃，向用户报告

---

## 涉及文件

| 文件 | 操作 | 说明 |
|---|---|---|
| `crates/nova-agent/src/tool/builtin/agent.rs` | **修改** | 实现 `run_in_background`；新增编排相关字段处理 |
| `crates/nova-agent/src/orchestrator/mod.rs` | **新增** | `OrchestratorEngine` |
| `crates/nova-agent/src/orchestrator/planner.rs` | **新增** | JSON 计划解析 → DAG |
| `crates/nova-agent/src/orchestrator/scheduler.rs` | **新增** | Stage 调度：并行 JoinSet / 串行队列 |
| `crates/nova-agent/src/orchestrator/reviewer.rs` | **新增** | Review Agent 触发与结果汇总 |
| `crates/nova-agent/src/lib.rs` | **修改** | `pub mod orchestrator` |

---

## 详细设计

### 1. `AgentTool` 的 `run_in_background` 实现

**当前状态**：收到 `run_in_background=true` 时打印 warning 并同步执行。

**目标实现**：

```rust
// agent.rs 中 execute() 的关键改动
async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
    let run_in_background = input["run_in_background"].as_bool().unwrap_or(false);

    if run_in_background {
        // 1. 从 context 获取 event sender 和 cancellation token
        let event_tx = context.as_ref()
            .and_then(|c| c.event_sender.clone())
            .ok_or_else(|| anyhow::anyhow!("Background agent requires event_sender in context"))?;

        let agent_id = input["agent_id"].as_str()
            .ok_or_else(|| anyhow::anyhow!("Background agent requires agent_id"))?
            .to_string();
        let stage_id = input["stage_id"].as_str().unwrap_or("unknown").to_string();

        // 2. 发送 spawn 事件
        let _ = event_tx.send(AgentEvent::Progress {
            kind: "sub_agent_spawn".to_string(),
            args: Some(json!({
                "agent_id": agent_id,
                "stage_id": stage_id,
                "description": description,
                "subagent_type": subagent_type.unwrap_or("default"),
            })),
            ..Default::default()
        }).await;

        // 3. 构建子 Agent runtime（现有逻辑）
        let runtime = self.build_runtime(subagent_type, model_override, &context)?;
        let prompt = prompt.to_string();
        let output_format = input["output_format"].as_str().unwrap_or("full").to_string();

        // 4. tokio::spawn 异步执行，返回 handle
        let handle = tokio::spawn(async move {
            // 执行子 Agent
            let result = runtime.run_turn(vec![], &prompt, None, event_tx.clone()).await;

            // 结果处理
            match result {
                Ok(turn_result) => {
                    let output = if output_format == "summary" {
                        extract_summary(&turn_result)
                    } else {
                        turn_result.final_text()
                    };
                    let _ = event_tx.send(AgentEvent::Progress {
                        kind: "sub_agent_complete".to_string(),
                        args: Some(json!({
                            "agent_id": agent_id,
                            "stage_id": stage_id,
                            "status": "success",
                            "output_summary": output,
                        })),
                        ..Default::default()
                    }).await;
                    Ok(output)
                }
                Err(e) => {
                    let _ = event_tx.send(AgentEvent::Progress {
                        kind: "sub_agent_complete".to_string(),
                        args: Some(json!({
                            "agent_id": agent_id,
                            "stage_id": stage_id,
                            "status": "failed",
                            "error": e.to_string(),
                        })),
                        ..Default::default()
                    }).await;
                    Err(e)
                }
            }
        });

        // 5. 返回 handle ID（供 Orchestrator 等待）
        // ToolOutput 扩展：新增 background_handle 字段
        return Ok(ToolOutput::background(handle, agent_id));
    }

    // 原同步路径不变...
}
```

**`ToolOutput` 扩展**：

```rust
pub enum ToolOutput {
    Text(String),
    // 新增：后台执行句柄
    Background {
        handle: tokio::task::JoinHandle<Result<String>>,
        agent_id: String,
    },
}
```

### 2. `OrchestratorEngine`

```rust
// crates/nova-agent/src/orchestrator/mod.rs

pub struct OrchestratorEngine {
    agent_tool: Arc<AgentTool>,
    event_tx: mpsc::Sender<AgentEvent>,
}

impl OrchestratorEngine {
    /// 执行完整的编排计划
    pub async fn execute_plan(
        &self,
        plan: OrchestrationPlan,
        cancellation_token: CancellationToken,
    ) -> Result<Vec<SubAgentResult>> {
        let mut all_results: HashMap<String, SubAgentResult> = HashMap::new();
        let plan_id = plan.plan_id.clone();

        // 广播计划
        self.emit_plan(&plan).await;

        // 按 Stage 顺序执行（已排好拓扑序）
        for stage in &plan.stages {
            // 检查前驱 Stage 全部成功
            for dep_id in &stage.depends_on {
                // 此时 depends_on 的 stage 已在 all_results 中
                // 若某前驱失败，中止后续 stage
            }

            let stage_results = match stage.mode {
                StageMode::Parallel => {
                    self.execute_parallel_stage(stage, &cancellation_token).await?
                }
                StageMode::Serial => {
                    self.execute_serial_stage(stage, &cancellation_token).await?
                }
            };

            // 发送 stage_complete 事件
            let all_success = stage_results.iter().all(|r| r.status == SubAgentStatus::Success);
            self.emit(ProgressEvent::stage_complete(&stage.stage_id, &stage.mode, all_success)).await;

            // 如果并行 Stage 有失败，根据策略决定继续/中止
            if !all_success {
                // 目前策略：中止整个 Plan（后续可配置为"跳过失败继续"）
                return Err(anyhow::anyhow!("Stage {} had failures", stage.stage_id));
            }

            for r in stage_results {
                all_results.insert(r.agent_id.clone(), r);
            }
        }

        // 触发 Review
        self.emit_review_start(&plan_id).await;
        let review_result = self.run_review(&plan, &all_results, &cancellation_token).await?;

        self.emit_orchestration_complete(&plan_id, review_result.success).await;
        Ok(all_results.into_values().collect())
    }

    /// 并行 Stage：使用 JoinSet 等待所有子 Agent
    async fn execute_parallel_stage(
        &self,
        stage: &ExecutionStage,
        cancel: &CancellationToken,
    ) -> Result<Vec<SubAgentResult>> {
        let mut join_set: JoinSet<Result<SubAgentResult>> = JoinSet::new();

        for agent_req in &stage.agents {
            let tool = Arc::clone(&self.agent_tool);
            let input = build_agent_input(agent_req, &stage.stage_id, true); // background=true
            let ctx = self.build_tool_context();

            join_set.spawn(async move {
                tool.execute(input, Some(ctx)).await
                    .map(|output| SubAgentResult::from_output(&agent_req.agent_id, &stage.stage_id, output))
            });
        }

        let mut results = Vec::new();
        while let Some(res) = join_set.join_next().await {
            match res {
                Ok(Ok(r)) => results.push(r),
                Ok(Err(e)) => {
                    // 某个子 Agent 出错，取消其他
                    cancel.cancel();
                    return Err(e);
                }
                Err(join_err) => return Err(anyhow::anyhow!("JoinError: {}", join_err)),
            }
        }
        Ok(results)
    }

    /// 串行 Stage：按序执行
    async fn execute_serial_stage(
        &self,
        stage: &ExecutionStage,
        cancel: &CancellationToken,
    ) -> Result<Vec<SubAgentResult>> {
        let mut results = Vec::new();
        for agent_req in &stage.agents {
            if cancel.is_cancelled() {
                break;
            }
            let input = build_agent_input(agent_req, &stage.stage_id, false); // background=false
            let ctx = self.build_tool_context();
            let output = self.agent_tool.execute(input, Some(ctx)).await?;
            results.push(SubAgentResult::from_output(&agent_req.agent_id, &stage.stage_id, output));
        }
        Ok(results)
    }
}
```

### 3. JSON 计划解析（`planner.rs`）

Orchestrator LLM 输出以下 JSON，由 `planner.rs` 解析：

```json
{
  "plan_id": "plan-<uuid>",
  "description": "实现用户认证模块",
  "stages": [
    {
      "stage_id": "s1",
      "mode": "parallel",
      "depends_on": [],
      "agents": [
        {
          "agent_id": "a1",
          "subagent_type": "Coder",
          "description": "实现数据库模型",
          "prompt": "在 src/models/user.rs 中实现 User 结构体...",
          "context_files": ["src/models/"]
        },
        {
          "agent_id": "a2",
          "subagent_type": "Coder",
          "description": "实现 API 路由",
          "prompt": "在 src/routes/auth.rs 中实现登录/注册端点...",
          "context_files": ["src/routes/"]
        }
      ]
    },
    {
      "stage_id": "s2",
      "mode": "serial",
      "depends_on": ["s1"],
      "agents": [
        {
          "agent_id": "a3",
          "subagent_type": "Coder",
          "description": "集成测试",
          "prompt": "基于 a1、a2 完成的实现，编写集成测试...",
          "context_files": ["tests/"]
        }
      ]
    }
  ]
}
```

解析校验：
- `plan_id` 唯一
- Stage 的 `depends_on` 引用的 stage_id 必须存在（DAG 合法性检查）
- 无环检测（拓扑排序）
- `agent_id` 在整个 Plan 内唯一

```rust
// planner.rs
pub fn parse_and_validate(json: &str) -> Result<OrchestrationPlan> {
    let plan: OrchestrationPlan = serde_json::from_str(json)
        .map_err(|e| anyhow::anyhow!("Invalid orchestration plan JSON: {e}"))?;

    // DAG 合法性
    let stage_ids: HashSet<&str> = plan.stages.iter().map(|s| s.stage_id.as_str()).collect();
    for stage in &plan.stages {
        for dep in &stage.depends_on {
            if !stage_ids.contains(dep.as_str()) {
                anyhow::bail!("Stage {} depends on unknown stage {}", stage.stage_id, dep);
            }
        }
    }

    // 拓扑排序（检测环）
    topological_sort(&plan.stages)?;

    // agent_id 唯一性
    let mut seen_agents = HashSet::new();
    for stage in &plan.stages {
        for agent in &stage.agents {
            if !seen_agents.insert(agent.agent_id.clone()) {
                anyhow::bail!("Duplicate agent_id: {}", agent.agent_id);
            }
        }
    }

    Ok(plan)
}
```

### 4. Review Agent（`reviewer.rs`）

```rust
pub async fn run_review(
    engine: &OrchestratorEngine,
    plan: &OrchestrationPlan,
    results: &HashMap<String, SubAgentResult>,
    cancel: &CancellationToken,
) -> Result<ReviewResult> {
    // 构造 Review Agent 的输入
    let summaries: Vec<String> = results.values()
        .map(|r| format!("## Agent: {}\n状态: {:?}\n摘要:\n{}", r.agent_id, r.status, r.output))
        .collect();

    let review_prompt = format!(
        "你是一个 Review Agent。以下是编排任务 '{}' 中各子 Agent 的执行摘要：\n\n{}\n\n
请评估：
1. 各子任务的输出是否自洽、无冲突
2. 整体目标是否已达成
3. 是否有需要补充或修正的内容

以 JSON 格式回复：
{{\"success\": true/false, \"issues\": [...], \"summary\": \"...\", \"retry_agents\": [\"a1\", ...]}}",
        plan.description,
        summaries.join("\n\n---\n\n")
    );

    let runtime = engine.build_reviewer_runtime();
    let result = runtime.run_turn(vec![], &review_prompt, Some(cancel.clone()), engine.event_tx.clone()).await?;

    // 解析 JSON 结果
    parse_review_result(&result.final_text())
}
```

### 5. 与 `AgentRuntime` 的整合点

Orchestrator 本身**运行在一个普通的 `AgentRuntime` turn 中**，通过工具调用触发：

```
用户 → Orchestrator Agent turn
            │
            │ LLM 输出 OrchestrationPlan JSON
            │ （通过特殊 tool call：OrchestrateTask）
            │
            ▼
  OrchestratorEngine.execute_plan()
            │
            │ 内部 spawn 多个 AgentTool 调用
            │ 结果通过同一 session 的 event_tx 流式发送
            │
            ▼
  ReviewAgent.run()
            │
            ▼
  turn 完成，返回 TurnResult
```

新增工具：`OrchestrateTask`（仅在 Orchestrator Skill 激活时启用）：

```json
{
  "name": "OrchestrateTask",
  "description": "提交编排计划并执行多 Agent 调度",
  "input_schema": {
    "type": "object",
    "properties": {
      "plan_json": { "type": "string", "description": "符合 OrchestrationPlan schema 的 JSON 字符串" }
    },
    "required": ["plan_json"]
  }
}
```

---

## 测试案例

### T2-01：并行执行时间验证
- **输入**：2 个子 Agent，每个 sleep 500ms（用 mock LLM 模拟）
- **预期**：总时间 < 700ms（并发执行），而非 1000ms（串行）

### T2-02：串行依赖顺序
- **输入**：Stage s1 依赖 s0，s0 先于 s1 执行
- **预期**：s1 的 Agent 在 s0 全部完成后才收到第一条日志

### T2-03：并行 Stage 某 Agent 失败
- **输入**：Stage s1 有 a1、a2，a1 成功，a2 返回 Error
- **预期**：Plan 中止，`stage_complete` 事件 `all_success=false`，`orchestration_complete` 未触发，错误信息包含 a2 的错误

### T2-04：无效 DAG 检测
- **输入**：plan JSON 中 Stage s1 depends_on ["s99"]（s99 不存在）
- **预期**：`parse_and_validate()` 返回 `Err`，不执行任何 Agent

### T2-05：环形依赖检测
- **输入**：s1 depends_on ["s2"]，s2 depends_on ["s1"]
- **预期**：`parse_and_validate()` 返回 `Err: cyclic dependency`

### T2-06：Review Agent 收到全量摘要
- **输入**：Plan 包含 3 个子 Agent，均成功
- **预期**：Review Agent 的 prompt 包含 3 个摘要段落

### T2-07：CancellationToken 传播
- **输入**：用户点击停止时，3 个并行 Agent 正在执行
- **预期**：3 个 Agent 均在下一个 checkpoint 处停止，无泄露的 tokio task

### T2-08：output_format=summary 裁剪上下文
- **输入**：子 Agent 输出 10000 字符，`output_format=summary`
- **预期**：`SubAgentResult.output` 长度 ≤ 500 字符（摘要模式截取/压缩）
