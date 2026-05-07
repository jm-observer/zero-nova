# 测试场景 8：自动触发编排（无 /orchestrator 前缀）

## 目标
验证在 `skill_routing_enabled = true` 的配置下，Agent 能否根据任务复杂度自主判断并激活 Orchestrator Skill，而无需用户手动输入 `/orchestrator`。

## 前置配置确认

确保 `.nova/config.toml` 中：
```toml
[gateway]
skill_routing_enabled = true
```

确保 `agent-nova.md` 中有编排能力声明（如果没有，此测试可能无法通过）。

## 触发方式

直接在聊天框中输入（不带 /orchestrator 前缀）：

```
帮我完成以下任务：
1. 在 tmp/test-output/utils.rs 中写一个字符串工具模块，包含 trim_and_lowercase 和 capitalize_first 两个函数
2. 在 tmp/test-output/validators.rs 中写一个校验模块，包含 is_valid_email 和 is_valid_url 两个函数
3. 在 tmp/test-output/test_all.rs 中为以上所有函数编写单元测试
这三个任务中，前两个可以并行，第三个依赖前两个
```

## 预期行为

### 路径 A：Agent 自动识别为编排任务
1. Agent 分析任务复杂度，判断需要编排
2. 自动调用 Skill 工具激活 Orchestrator
3. 后续流程与手动 `/orchestrator` 相同

### 路径 B：Agent 未自动触发
如果 Agent 选择串行逐步完成而非编排，说明：
- `agent-nova.md` 中编排能力声明不够强
- 或本地模型对 Skill 触发的理解能力不足
- 这本身也是一个有价值的测试结果

## 预期编排计划结构（路径 A）

```json
{
  "planId": "plan-xxxxxxxx",
  "description": "并行创建工具模块和校验模块，串行编写测试",
  "stages": [
    {
      "stageId": "s1",
      "mode": "parallel",
      "dependsOn": [],
      "agents": [
        {
          "agentId": "a1",
          "subagentType": "Coder",
          "description": "字符串工具模块",
          "prompt": "...",
          "contextFiles": ["tmp/test-output/utils.rs"]
        },
        {
          "agentId": "a2",
          "subagentType": "Coder",
          "description": "校验模块",
          "prompt": "...",
          "contextFiles": ["tmp/test-output/validators.rs"]
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
          "description": "单元测试",
          "prompt": "...",
          "contextFiles": ["tmp/test-output/"]
        }
      ]
    }
  ]
}
```

## 验证点

- [ ] Agent 是否自主判断并激活 Orchestrator Skill（观察是否出现 Skill 激活事件）
- [ ] 如果自动触发，编排计划是否合理（并行 + 串行结构）
- [ ] 如果未触发，记录 Agent 的实际行为作为基线参考
- [ ] skill_routing_enabled=true 配置是否生效
- [ ] 对比手动 `/orchestrator` 和自动触发的结果差异
