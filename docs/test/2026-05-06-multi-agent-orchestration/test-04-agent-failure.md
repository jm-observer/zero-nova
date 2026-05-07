# 测试场景 4：子 Agent 失败处理

## 目标
验证当某个子 Agent 执行失败时，系统能正确标记失败、传播错误，并影响后续依赖 Stage 的执行。

## 触发方式

```
/orchestrator 并行执行以下两个任务：
1. 在 tmp/test-output/success.txt 中写入 "I am successful"
2. 执行命令 `cat /nonexistent/path/impossible.txt` 并将结果写入 tmp/test-output/result.txt

两个任务完成后，在 tmp/test-output/final-report.txt 中汇总上面两个任务的结果
```

## 预期编排计划结构

```json
{
  "planId": "plan-xxxxxxxx",
  "description": "并行执行一成功一失败的任务，再串行汇总",
  "stages": [
    {
      "stageId": "s1",
      "mode": "parallel",
      "dependsOn": [],
      "agents": [
        {
          "agentId": "a1",
          "subagentType": "Coder",
          "description": "写入成功文件",
          "prompt": "在 tmp/test-output/success.txt 中写入内容 'I am successful'。确保目录存在。",
          "contextFiles": ["tmp/test-output/"]
        },
        {
          "agentId": "a2",
          "subagentType": "Coder",
          "description": "读取不存在的文件",
          "prompt": "执行命令 cat /nonexistent/path/impossible.txt，将命令的输出写入 tmp/test-output/result.txt。不要做任何错误处理或降级，如果命令失败就报告失败。",
          "contextFiles": ["tmp/test-output/"]
        }
      ]
    },
    {
      "stageId": "s2",
      "mode": "serial",
      "dependsOn": ["s1"],
      "agents": [
        {
          "agentId": "a3",
          "subagentType": "Coder",
          "description": "汇总报告",
          "prompt": "读取 tmp/test-output/success.txt 和 tmp/test-output/result.txt 的内容，在 tmp/test-output/final-report.txt 中写入汇总。",
          "contextFiles": ["tmp/test-output/"]
        }
      ]
    }
  ]
}
```

## 预期事件流

1. `orchestration_plan`
2. `sub_agent_spawn` x2 — a1, a2 并行启动
3. `sub_agent_complete` (a1) — status=Success
4. `sub_agent_complete` (a2) — status=Failed, error 字段非空
5. `stage_complete` (s1) — allSuccess=false
6. 后续行为取决于实现：
   - 可能跳过 s2（因为依赖的 s1 有失败）
   - 或继续执行 s2 但 Review Agent 感知到部分失败
7. `orchestration_complete` — overallSuccess=false

## 验证点

- [ ] 成功的 Agent (a1) 结果是否正确保留（success.txt 存在）
- [ ] 失败的 Agent (a2) 是否正确报告 status=Failed
- [ ] error 字段是否包含有意义的错误信息
- [ ] Stage 完成事件中 allSuccess=false
- [ ] 依赖 s1 的 s2 是否被正确处理（跳过或带条件执行）
- [ ] Review Agent 是否感知到部分失败并在评审中提及
- [ ] 最终 orchestration_complete 的 overallSuccess=false
- [ ] 前端是否用视觉区分（如红色标记）展示失败 Agent

## 注意事项

本地 LLM 可能会尝试"修复"失败（比如跳过 cat 命令直接创建空文件），因此 prompt 中明确要求不做降级处理。如果 LLM 仍然绕过，可以换一个更明确会失败的任务，例如要求执行一个不存在的二进制文件。
