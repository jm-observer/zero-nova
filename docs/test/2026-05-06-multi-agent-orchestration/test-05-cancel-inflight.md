# 测试场景 5：取消正在执行的编排

## 目标
验证用户在编排执行过程中取消操作时，CancellationToken 能正确传播，所有在途子 Agent 被中止。

## 触发方式

```
/orchestrator 并行执行 3 个耗时任务：
1. 在 tmp/test-output/slow1.txt 中写入 1 到 100 的数字，每写一个数字前先执行 sleep 2
2. 在 tmp/test-output/slow2.txt 中写入 A 到 Z 的字母，每写一个字母前先执行 sleep 2
3. 在 tmp/test-output/slow3.txt 中写入当前时间戳 50 次，每次前先执行 sleep 2
```

## 操作步骤

1. 输入上述命令，等待编排计划生成并开始执行
2. 观察到 `sub_agent_spawn` 事件出现后（至少看到一些 `sub_agent_log`）
3. **立即点击前端的停止按钮** 或在 CLI 中按 **Ctrl+C**
4. 观察后续事件

## 预期编排计划结构

```json
{
  "planId": "plan-xxxxxxxx",
  "description": "并行执行 3 个耗时写入任务",
  "stages": [
    {
      "stageId": "s1",
      "mode": "parallel",
      "dependsOn": [],
      "agents": [
        {
          "agentId": "a1",
          "subagentType": "Coder",
          "description": "写入数字",
          "prompt": "..."
        },
        {
          "agentId": "a2",
          "subagentType": "Coder",
          "description": "写入字母",
          "prompt": "..."
        },
        {
          "agentId": "a3",
          "subagentType": "Coder",
          "description": "写入时间戳",
          "prompt": "..."
        }
      ]
    }
  ]
}
```

## 预期事件流

1. `orchestration_plan`
2. `sub_agent_spawn` x3
3. `sub_agent_log` x N（部分日志）
4. **用户取消**
5. `sub_agent_complete` x3 — status=Cancelled（或部分 Success + 部分 Cancelled）
6. `stage_complete` (s1) — allSuccess=false
7. `orchestration_complete` — overallSuccess=false

## 验证点

- [ ] 取消后所有在途子 Agent 是否在合理时间内停止（不超过几秒）
- [ ] 子 Agent 的 status 是否标记为 Cancelled
- [ ] 没有子 Agent 在取消后继续产生新的日志事件
- [ ] 编排最终状态为 overallSuccess=false
- [ ] 系统没有发生 panic 或未处理的错误
- [ ] 文件可能只被部分写入（这是预期行为）
- [ ] 前端是否正确显示"已取消"状态
