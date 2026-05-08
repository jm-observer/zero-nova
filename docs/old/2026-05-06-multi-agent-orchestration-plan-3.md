# Plan 3: 提示词与 Skill 设计（触发机制）

- **前置依赖**：Plan 1（协议与数据模型）
- **状态**：已完成（2026-05-06）

---

## 本次目标

1. 明确触发机制：混合方式（提示词感知 + Skill 激活）
2. 更新 `agent-nova.md`，添加 5 行编排能力声明（不污染简单任务）
3. 创建 `orchestrator` Skill，定义完整编排协议、JSON 格式、并行/串行判断规则
4. 回答"Skill vs. 提示词"的最终设计决策

**可验证标准：**
- `agent-nova.md` 新增内容 < 20 行，不影响现有单 Agent 任务的响应风格
- Orchestrator Skill 激活后，Agent 能输出合规的 OrchestrationPlan JSON
- 用户输入 `/orchestrator <任务>` 能正确触发 Skill
- Agent 在简单任务（如"帮我改一个 typo"）时不会主动激活编排 Skill

---

## 触发机制最终决策

### 方案对比

| 维度 | 纯提示词 | 纯 Skill | **混合（推荐）** |
|---|---|---|---|
| 简单任务 Token 消耗 | 高（每轮携带编排指令） | 低 | **低**（Skill 按需加载）|
| Agent 自主触发 | 可以（判断逻辑在 prompt 中） | 需用户手动 | **可以**（prompt 告知能力，Skill 提供详情）|
| 用户显式控制 | 无 | `/orchestrator` | **有**（`/orchestrator` 强制激活）|
| 编排指令完整性 | 可能被截断/遗忘 | 完整独立 | **完整**（Skill 独立文件，完整加载）|
| 本地模型幻觉风险 | 高（长 prompt 弱化遵从性） | 中（短 Skill 指令更易遵循） | **中-低**（精简 Skill 文件）|

### 混合方案执行逻辑

```
用户输入
    │
    ├─ 简单任务（改 typo、查代码）→ 正常单 Agent 执行
    │
    └─ 复杂任务（多文件实现、并行可分解）
           │
           ├─ 用户显式："/orchestrator 实现用户认证"
           │       → Skill 工具直接激活 Orchestrator Skill
           │
           └─ Agent 自主判断：任务满足以下任一条件时激活
                   - 需要修改 3+ 个相互独立的模块
                   - 用户明确要求"并行"或"多 Agent"
                   - 任务描述中出现明确的独立子任务列表
                   → Agent 调用 Skill 工具激活 Orchestrator Skill
```

---

## 涉及文件

| 文件 | 操作 | 说明 |
|---|---|---|
| `.nova/prompts/agent-nova.md` | **修改** | 新增 `# 多 Agent 编排` 小节（约 15 行）|
| `.nova/skills/orchestrator/SKILL.md` | **新增** | Orchestrator Skill 完整定义 |

---

## 详细设计

### 1. `agent-nova.md` 新增内容

在 `# 任务管理` 章节之后插入：

```markdown
# 多 Agent 编排

当任务满足以下**任一**条件时，主动调用 `Skill` 工具激活 `orchestrator` Skill：

- 需要修改 **3 个或以上相互独立** 的模块/文件，且各模块之间无强依赖
- 用户明确要求"并行执行"或"多 Agent"
- 任务可以清晰地拆分为有先后依赖的若干阶段

否则，对简单任务**不要**触发编排——直接执行效率更高。
```

**说明**：
- 约 8 行，Token 消耗极低
- 不包含编排协议细节（避免 prompt 膨胀）
- 只描述"何时触发"，"如何执行"由 Skill 文件提供

### 2. Orchestrator Skill：`SKILL.md`

```markdown
---
name: orchestrator
description: >
  多 Agent 编排器。将复杂任务分解为 DAG，并行/串行调度子 Agent 执行，
  最后由 Review Agent 评审。当任务需要多个独立子 Agent 协作时使用。
aliases: [multi-agent, parallel-agent, 编排]
tool_policy: allow_list_with_deferred
tools:
  - OrchestrateTask
  - TaskCreate
  - TaskUpdate
  - TaskList
  - Read
  - Bash
---

# Orchestrator

你是一个多 Agent 编排器。你的职责是：
1. 分析任务，识别可并行 / 必须串行的子任务
2. 输出结构化编排计划（JSON）
3. 通过 `OrchestrateTask` 工具提交计划并执行

## 任务分析原则

**可并行的子任务特征：**
- 操作不同的文件或模块，无共享可变状态
- 互相不依赖对方的输出
- 示例：同时实现 `models/`、`routes/`、`tests/` 三个模块

**必须串行的子任务特征：**
- 后续任务依赖前一任务的输出
- 共享同一文件的读写（竞争条件）
- 示例：先实现数据模型（Stage 1），再基于模型编写 API（Stage 2）

**文件范围分配原则（避免并行冲突）：**
- 每个并行子 Agent 分配**互斥的文件/目录范围**
- 若两个子任务必须操作同一文件，将其放入**同一个串行 Stage**

## 输出格式

分析完成后，调用 `OrchestrateTask` 工具，`plan_json` 字段包含以下格式的 JSON：

```json
{
  "plan_id": "plan-<8位随机字符串>",
  "description": "<整体任务的一句话描述>",
  "stages": [
    {
      "stage_id": "s1",
      "mode": "parallel",
      "depends_on": [],
      "agents": [
        {
          "agent_id": "a1",
          "subagent_type": "Coder",
          "description": "<3-5字描述>",
          "prompt": "<完整、自包含的子任务提示词>",
          "context_files": ["src/models/"]
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
          "description": "<描述>",
          "prompt": "<提示词，可引用上一阶段预期输出>",
          "context_files": ["tests/"]
        }
      ]
    }
  ]
}
```

## 子 Agent 提示词规范

每个子 Agent 的 `prompt` 必须：
1. **自包含**：不依赖其他子 Agent 的上下文，包含所有必要信息
2. **范围明确**：明确指定操作的文件/目录
3. **输出说明**：说明期望的输出形式（代码文件、摘要等）
4. **构建验证**：要求子 Agent 在完成后执行 `cargo check` 或相应检查

示例（好）：
```
在 `src/models/user.rs` 中实现 User 结构体，包含字段：id(Uuid)、email(String)、
password_hash(String)、created_at(DateTime)。使用 Diesel ORM 注解。
完成后运行 `cargo check -p nova-models` 确认无编译错误。
```

示例（差）：
```
实现用户模型（参考其他 Agent 的实现）
```

## 并行数量限制

- 单个 Stage 并行子 Agent 数量 **≤ 5**
- 总 Stage 数量 **≤ 8**
- 若任务更大，考虑分多轮编排

## Review Agent 指引

所有 Stage 执行完毕后，Review Agent 会自动触发。你无需手动调用。
Review Agent 会收到每个子 Agent 的输出摘要，并判断：
- 各子任务是否自洽
- 整体目标是否达成
- 是否需要重试某个子 Agent

## 降级策略

若 `OrchestrateTask` 解析失败（如本地模型输出非法 JSON），系统会：
1. 提示错误原因
2. 降级为单 Agent 执行（你直接执行整个任务，不拆分）

## 示例对话

用户：帮我实现一个简单的 JWT 认证系统，包括数据库模型、API 路由和测试

Orchestrator 分析：
- Stage 1（并行）：数据库模型（models/）、路由定义（routes/）可独立实现
- Stage 2（串行，依赖 s1）：集成测试需要依赖 s1 的输出

→ 调用 OrchestrateTask { plan_json: "..." }
```

### 3. `CapabilityPolicy` 配置

`orchestrator` Skill 激活时，工具白名单：

```toml
# .nova/skills/orchestrator/skill.toml（或在 SKILL.md frontmatter 中配置）
tools = ["OrchestrateTask", "TaskCreate", "TaskUpdate", "TaskList", "Read", "Bash"]
```

`OrchestrateTask` 工具在非 Orchestrator Skill 状态下**不出现在工具列表**中（通过 `CapabilityPolicy.always_enabled_tools` 控制，仅 Orchestrator Skill 的 allow list 包含它）。

---

## 本地模型适配注意事项

本地模型（如 Qwen、DeepSeek 等）生成合规 JSON 的能力参差不齐，需额外加固：

1. **JSON 提取**：`planner.rs` 中使用正则从输出中提取 ` ```json ... ``` ` 代码块
2. **少样本示例**：SKILL.md 中提供 1-2 个完整示例（已包含）
3. **Schema 验证前提示**：在 `OrchestrateTask` 的 description 中附加 Schema 摘要
4. **降级路径**：解析失败时提示用户，可选择继续（单 Agent）或修正后重试

---

## 测试案例

### T3-01：简单任务不触发编排
- **输入**：`帮我把 README 里的 typo 改掉`
- **预期**：Agent 直接修改文件，不调用 Skill 工具，不输出 JSON

### T3-02：复杂任务自主触发
- **输入**：`实现一个完整的用户认证系统，包括模型层、接口层和测试`
- **预期**：Agent 调用 `Skill` 工具激活 orchestrator，随后输出合规 OrchestrationPlan JSON

### T3-03：显式触发
- **输入**：`/orchestrator 实现用户认证`
- **预期**：Orchestrator Skill 立即激活，跳过自主判断阶段

### T3-04：JSON 合规性
- **前提**：Orchestrator Skill 已激活
- **输入**：给定一个 3 模块并行任务
- **预期**：输出 JSON 能通过 `planner::parse_and_validate()` 验证

### T3-05：并行任务文件范围互斥
- **输入**：并行任务中两个 Agent 的 `context_files` 无重叠
- **预期**：planner 接受（不拒绝）；若有重叠，给出警告但不强制拒绝

### T3-06：降级路径（JSON 解析失败）
- **输入**：本地模型输出了非法 JSON
- **预期**：系统提示"计划解析失败，回退单 Agent 执行"，任务继续完成

### T3-07：Skill 提示词 Token 统计
- **前提**：激活 Orchestrator Skill 前后对比 prompt token 数
- **预期**：未激活时新增内容 < 50 token；激活后 Skill 内容约 400-600 token
