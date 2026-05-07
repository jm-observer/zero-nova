# 测试场景 6：Review Agent 评审验证

## 目标
验证所有 Stage 执行完毕后 Review Agent 能正确触发，汇总各子 Agent 输出，并给出有意义的评审结论。

## 触发方式

```
/orchestrator 并行完成以下两个独立任务，要求各子 Agent 输出详细的工作摘要：
1. 在 tmp/test-output/analysis.md 中写一份关于 Rust 所有权系统的简短分析（3-5 段）
2. 在 tmp/test-output/comparison.md 中写一份 Rust 与 Go 错误处理机制的对比（3-5 段）
```

## 预期编排计划结构

```json
{
  "planId": "plan-xxxxxxxx",
  "description": "并行撰写两篇技术分析文章",
  "stages": [
    {
      "stageId": "s1",
      "mode": "parallel",
      "dependsOn": [],
      "agents": [
        {
          "agentId": "a1",
          "subagentType": "Coder",
          "description": "Rust 所有权分析",
          "prompt": "在 tmp/test-output/analysis.md 中撰写关于 Rust 所有权系统（ownership, borrowing, lifetimes）的简短分析，3-5 段。完成后输出工作摘要。",
          "contextFiles": ["tmp/test-output/"]
        },
        {
          "agentId": "a2",
          "subagentType": "Coder",
          "description": "错误处理对比",
          "prompt": "在 tmp/test-output/comparison.md 中撰写 Rust（Result/Option）与 Go（error 接口）错误处理机制的对比分析，3-5 段。完成后输出工作摘要。",
          "contextFiles": ["tmp/test-output/"]
        }
      ]
    }
  ]
}
```

## 预期事件流

1. `orchestration_plan`
2. `sub_agent_spawn` x2
3. `sub_agent_log` x N
4. `sub_agent_complete` x2 — 两个都 Success，outputSummary 非空
5. `stage_complete` (s1) — allSuccess=true
6. **`orchestration_review_start`** — Review Agent 被触发
7. Review Agent 接收到两个子 Agent 的 outputSummary
8. Review Agent 输出 ReviewResult JSON：
   ```json
   {
     "success": true,
     "issues": [],
     "summary": "两篇文章均已完成，内容自洽...",
     "retryAgents": []
   }
   ```
9. `orchestration_complete` — overallSuccess=true, summary 包含评审内容

## 验证点

- [ ] `orchestration_review_start` 事件是否在所有 Stage 完成后出现
- [ ] Review Agent 是否收到了每个子 Agent 的摘要信息
- [ ] Review Agent 的评审结论是否引用了各子 Agent 的实际产出
- [ ] ReviewResult 中 success=true，issues 为空，retryAgents 为空
- [ ] 最终 summary 是否有实质内容（不是模板化的空话）
- [ ] 两个 md 文件是否都存在且内容合理
- [ ] 前端是否有专门的 Review Agent 展示区域
