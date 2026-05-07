# 测试场景 2：串行依赖

## 目标
验证有依赖关系的 Stage 按正确顺序串行执行，后续 Stage 能感知前序 Stage 的输出。

## 触发方式

```
/orchestrator 分两步完成任务：
第一步：在 tmp/test-output/config.json 中写入一个 JSON 配置文件，内容为 {"app_name": "nova-test", "version": "1.0.0", "port": 8080}
第二步：读取 tmp/test-output/config.json 的内容，然后在 tmp/test-output/config-summary.txt 中写一段文字总结这个配置文件的内容
```

## 预期编排计划结构

```json
{
  "planId": "plan-xxxxxxxx",
  "description": "串行创建配置文件并生成摘要",
  "stages": [
    {
      "stageId": "s1",
      "mode": "serial",
      "dependsOn": [],
      "agents": [
        {
          "agentId": "a1",
          "subagentType": "Coder",
          "description": "创建配置文件",
          "prompt": "在 tmp/test-output/config.json 中创建 JSON 配置文件，内容为：{\"app_name\": \"nova-test\", \"version\": \"1.0.0\", \"port\": 8080}。先确保目录存在。",
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
          "agentId": "a2",
          "subagentType": "Coder",
          "description": "生成配置摘要",
          "prompt": "读取 tmp/test-output/config.json 文件内容，然后在 tmp/test-output/config-summary.txt 中写一段文字总结该配置：应用名称、版本号和端口。",
          "contextFiles": ["tmp/test-output/"]
        }
      ]
    }
  ]
}
```

## 预期事件流

1. `orchestration_plan` — 2 个 Stage，s2 依赖 s1
2. `sub_agent_spawn` (a1) — Stage 1 启动
3. `sub_agent_log` x N — a1 执行日志
4. `sub_agent_complete` (a1) — status=Success
5. `stage_complete` (s1) — allSuccess=true
6. `sub_agent_spawn` (a2) — Stage 2 启动（必须在 s1 完成后）
7. `sub_agent_log` x N — a2 执行日志
8. `sub_agent_complete` (a2) — status=Success
9. `stage_complete` (s2) — allSuccess=true
10. `orchestration_review_start`
11. `orchestration_complete` — overallSuccess=true

## 验证点

- [ ] Stage 2 是否在 Stage 1 完成后才启动（观察 spawn 时间戳）
- [ ] config.json 文件内容是否正确
- [ ] config-summary.txt 是否存在且引用了 config.json 的实际内容
- [ ] 依赖链 s2 -> s1 是否正确执行
- [ ] 前端展示是否呈现两个串行 Stage
