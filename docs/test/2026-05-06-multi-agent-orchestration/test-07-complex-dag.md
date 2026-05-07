# 测试场景 7：复杂 DAG（多 Stage 混合依赖）

## 目标
验证 3 个以上 Stage 的复杂 DAG 调度：多层依赖、并行与串行混合、拓扑排序正确性。

## 触发方式

```
/orchestrator 请按以下计划完成任务：

阶段 1（并行）：
- 在 tmp/test-output/module_a.rs 中创建模块 A，定义 pub struct Config { pub name: String }
- 在 tmp/test-output/module_b.rs 中创建模块 B，定义 pub struct Database { pub url: String }

阶段 2（并行，依赖阶段 1）：
- 在 tmp/test-output/service_a.rs 中创建服务 A，引用模块 A 的 Config，定义 pub fn init_config() -> Config
- 在 tmp/test-output/service_b.rs 中创建服务 B，引用模块 B 的 Database，定义 pub fn connect_db() -> Database

阶段 3（串行，依赖阶段 2）：
- 在 tmp/test-output/main.rs 中创建主入口，引用服务 A 和 B，调用 init_config() 和 connect_db()
```

## 预期编排计划结构

```json
{
  "planId": "plan-xxxxxxxx",
  "description": "三层 DAG：基础模块 → 服务层 → 入口",
  "stages": [
    {
      "stageId": "s1",
      "mode": "parallel",
      "dependsOn": [],
      "agents": [
        {
          "agentId": "a1",
          "subagentType": "Coder",
          "description": "创建模块 A",
          "prompt": "在 tmp/test-output/module_a.rs 中定义：pub struct Config { pub name: String }。确保目录存在。",
          "contextFiles": ["tmp/test-output/module_a.rs"]
        },
        {
          "agentId": "a2",
          "subagentType": "Coder",
          "description": "创建模块 B",
          "prompt": "在 tmp/test-output/module_b.rs 中定义：pub struct Database { pub url: String }。确保目录存在。",
          "contextFiles": ["tmp/test-output/module_b.rs"]
        }
      ]
    },
    {
      "stageId": "s2",
      "mode": "parallel",
      "dependsOn": ["s1"],
      "agents": [
        {
          "agentId": "a3",
          "subagentType": "Coder",
          "description": "创建服务 A",
          "prompt": "在 tmp/test-output/service_a.rs 中创建服务 A。假设 module_a.rs 已存在并定义了 Config 结构体。定义：mod module_a; use module_a::Config; pub fn init_config() -> Config { Config { name: \"nova\".to_string() } }",
          "contextFiles": ["tmp/test-output/service_a.rs", "tmp/test-output/module_a.rs"]
        },
        {
          "agentId": "a4",
          "subagentType": "Coder",
          "description": "创建服务 B",
          "prompt": "在 tmp/test-output/service_b.rs 中创建服务 B。假设 module_b.rs 已存在并定义了 Database 结构体。定义：mod module_b; use module_b::Database; pub fn connect_db() -> Database { Database { url: \"localhost:5432\".to_string() } }",
          "contextFiles": ["tmp/test-output/service_b.rs", "tmp/test-output/module_b.rs"]
        }
      ]
    },
    {
      "stageId": "s3",
      "mode": "serial",
      "dependsOn": ["s2"],
      "agents": [
        {
          "agentId": "a5",
          "subagentType": "Coder",
          "description": "创建主入口",
          "prompt": "在 tmp/test-output/main.rs 中创建主入口。引用 service_a 和 service_b 模块，在 fn main() 中调用 service_a::init_config() 和 service_b::connect_db()，打印结果。",
          "contextFiles": ["tmp/test-output/"]
        }
      ]
    }
  ]
}
```

## 预期事件流

1. `orchestration_plan` — 3 个 Stage，依赖链 s1 → s2 → s3
2. **Stage 1**：`sub_agent_spawn` x2 → `sub_agent_complete` x2 → `stage_complete`
3. **Stage 2**：`sub_agent_spawn` x2 → `sub_agent_complete` x2 → `stage_complete`
4. **Stage 3**：`sub_agent_spawn` x1 → `sub_agent_complete` x1 → `stage_complete`
5. `orchestration_review_start`
6. `orchestration_complete`

## 验证点

- [ ] 拓扑排序是否正确：s1 → s2 → s3 严格按序
- [ ] s1 的两个 Agent 是否并行
- [ ] s2 的两个 Agent 是否并行
- [ ] s2 是否在 s1 全部完成后才启动
- [ ] s3 是否在 s2 全部完成后才启动
- [ ] 所有 5 个文件是否正确创建
- [ ] 文件间的引用关系是否逻辑自洽
- [ ] 总共 5 个 Agent 全部 Success
- [ ] 前端 Agent 树是否呈现 3 层结构
