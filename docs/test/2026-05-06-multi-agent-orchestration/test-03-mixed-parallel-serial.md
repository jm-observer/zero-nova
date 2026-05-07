# 测试场景 3：混合并行+串行调度

## 目标
验证并行和串行混合的 DAG 调度：Stage 1 并行执行多个独立子任务，Stage 2 串行执行依赖 Stage 1 输出的任务。

## 触发方式

```
/orchestrator 请完成以下任务：
1. 并行创建两个 Rust 源文件：
   - tmp/test-output/greeter.rs：定义一个 pub fn greet(name: &str) -> String 函数，返回 "Hello, {name}!"
   - tmp/test-output/farewell.rs：定义一个 pub fn farewell(name: &str) -> String 函数，返回 "Goodbye, {name}!"
2. 上面两个文件完成后，创建 tmp/test-output/lib.rs，内容为 mod greeter; mod farewell; 并添加一个 pub fn demo() 函数，调用 greeter::greet 和 farewell::farewell
```

## 预期编排计划结构

```json
{
  "planId": "plan-xxxxxxxx",
  "description": "并行创建两个模块，然后串行创建入口文件",
  "stages": [
    {
      "stageId": "s1",
      "mode": "parallel",
      "dependsOn": [],
      "agents": [
        {
          "agentId": "a1",
          "subagentType": "Coder",
          "description": "创建 greeter 模块",
          "prompt": "在 tmp/test-output/greeter.rs 中定义函数：pub fn greet(name: &str) -> String { format!(\"Hello, {}!\", name) }。确保目录存在。",
          "contextFiles": ["tmp/test-output/greeter.rs"]
        },
        {
          "agentId": "a2",
          "subagentType": "Coder",
          "description": "创建 farewell 模块",
          "prompt": "在 tmp/test-output/farewell.rs 中定义函数：pub fn farewell(name: &str) -> String { format!(\"Goodbye, {}!\", name) }。确保目录存在。",
          "contextFiles": ["tmp/test-output/farewell.rs"]
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
          "description": "创建入口文件",
          "prompt": "在 tmp/test-output/lib.rs 中创建入口文件。内容包含：mod greeter; mod farewell; 以及 pub fn demo() -> String 函数，该函数调用 greeter::greet(\"World\") 和 farewell::farewell(\"World\") 并拼接返回。",
          "contextFiles": ["tmp/test-output/"]
        }
      ]
    }
  ]
}
```

## 预期事件流

1. `orchestration_plan` — 2 个 Stage
2. `sub_agent_spawn` x2 — a1, a2 同时启动（并行）
3. `sub_agent_complete` x2 — a1, a2 完成
4. `stage_complete` (s1) — allSuccess=true
5. `sub_agent_spawn` (a3) — Stage 2 启动
6. `sub_agent_complete` (a3)
7. `stage_complete` (s2) — allSuccess=true
8. `orchestration_review_start`
9. `orchestration_complete`

## 验证点

- [ ] Stage 1 的两个 Agent 是否并行启动
- [ ] Stage 2 是否在 Stage 1 全部完成后才启动
- [ ] greeter.rs 和 farewell.rs 内容是否正确
- [ ] lib.rs 是否正确引用了两个模块
- [ ] 前端 Agent 树显示：Stage 1 有 2 个并行节点，Stage 2 有 1 个串行节点
- [ ] DAG 依赖关系视觉上是否清晰
