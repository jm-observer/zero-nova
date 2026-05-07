# 测试场景 1：简单并行

## 目标
验证多个无依赖的子 Agent 能够并行执行。

## 触发方式
在前端聊天框输入以下内容：

```
/orchestrator 在项目根目录的 tmp/test-output/ 下并行创建 3 个独立的文本文件：
1. tmp/test-output/hello.txt - 内容为 "Hello from Agent A"
2. tmp/test-output/world.txt - 内容为 "World from Agent B"
3. tmp/test-output/foo.txt - 内容为 "Foo from Agent C"
```

## 预期编排计划结构

```json
{
  "planId": "plan-xxxxxxxx",
  "description": "并行创建 3 个独立文本文件",
  "stages": [
    {
      "stageId": "s1",
      "mode": "parallel",
      "dependsOn": [],
      "agents": [
        {
          "agentId": "a1",
          "subagentType": "Coder",
          "description": "创建 hello.txt",
          "prompt": "在 tmp/test-output/hello.txt 中写入内容 'Hello from Agent A'。先确保目录 tmp/test-output/ 存在。",
          "contextFiles": ["tmp/test-output/"]
        },
        {
          "agentId": "a2",
          "subagentType": "Coder",
          "description": "创建 world.txt",
          "prompt": "在 tmp/test-output/world.txt 中写入内容 'World from Agent B'。先确保目录 tmp/test-output/ 存在。",
          "contextFiles": ["tmp/test-output/"]
        },
        {
          "agentId": "a3",
          "subagentType": "Coder",
          "description": "创建 foo.txt",
          "prompt": "在 tmp/test-output/foo.txt 中写入内容 'Foo from Agent C'。先确保目录 tmp/test-output/ 存在。",
          "contextFiles": ["tmp/test-output/"]
        }
      ]
    }
  ]
}
```

## 预期事件流

1. `orchestration_plan` — 发布编排计划，包含 1 个 Stage、3 个 Agent
2. `sub_agent_spawn` x3 — 3 个子 Agent 同时启动
3. `sub_agent_log` x N — 各子 Agent 的执行日志（交错出现，体现并行）
4. `sub_agent_complete` x3 — 3 个子 Agent 均 status=Success
5. `stage_complete` — stageId=s1, allSuccess=true
6. `orchestration_review_start` — Review Agent 开始评审
7. `orchestration_complete` — overallSuccess=true

## 验证点

- [ ] 3 个子 Agent 是否真正并行启动（观察 spawn 事件的时间戳是否接近）
- [ ] 3 个文件是否都正确创建且内容正确
- [ ] Stage 完成事件中 allSuccess=true
- [ ] Review Agent 是否正常触发并返回评审结果
- [ ] 前端 Agent 树是否显示 1 个 Stage 下 3 个并行节点
