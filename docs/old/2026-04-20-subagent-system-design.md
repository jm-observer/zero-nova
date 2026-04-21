# Subagent 系统设计文档

| 字段 | 内容 |
|-----------|------|
| 创建时间 | 2026-04-20 |
| 状态 | Draft v2 |
| 本次目标 | 实现 `spawn_subagent` 工具，为 skill-creator 评测流水线提供隔离的子代理执行能力 |

---

## 1. 现状与核心痛点

### 1.1 项目现状
`zero-nova` 正在集成 `skill-creator` 框架，目标是实现 Skill 的自动化创建、测试与评估。系统已具备：
- **工具执行**：`BashTool`、`ReadFileTool`、`WriteFileTool`、`WebSearchTool`、`WebFetchTool`
- **Skill 加载**：`SkillRegistry` 从 `.nova/skills/*/SKILL.md` 加载并注入 system prompt
- **会话管理**：Gateway 支持多 Session 并发，SQLite 持久化
- **事件流**：`AgentEvent` 枚举支持 `TextDelta`、`ToolStart`、`ToolEnd`、`TurnComplete` 等

### 1.2 阻塞性问题

#### 问题 1：执行上下文污染 (Context Pollution)
- **场景**：`skill-creator` 的评测流水线需要对同一 prompt 分别运行 `with_skill`（注入 Skill）和 `baseline`（裸模型）两组，并通过 `comparator` 盲评。
- **当前约束**：没有独立子代理，A/B 两组必须在同一 Session 中串行执行。模型回答 B 时会受到 A 的历史消息影响，导致"盲测"失效。
- **影响级别**：**阻塞** — 评测数据不可信，skill-creator 的核心 `compare → analyze → iterate` 循环无法启动。

#### 问题 2：环境与操作系统不匹配 (OS & Env Mismatch)
- **场景**：`open-source-tech-explorer` 等 Skill 的测试用例针对 Linux + NVIDIA GPU 编写。
- **当前约束**：开发环境为 Windows (MSYS2)，无法原生执行 Linux 脚本。
- **影响级别**：**阻塞** — 涉及平台相关命令的 eval 直接失败。

#### 问题 3：性能指标采集缺失 (Metrics Gap)
- **场景**：`benchmark.json` / `timing.json` 需要每个任务的 `total_tokens` 和 `duration_ms`。
- **当前约束**：这些数据封装在 `AgentRuntime` 的 `TurnResult.usage` 和流式事件中，主代理无法通过 `bash` 工具调用获取。
- **影响级别**：**阻塞** — 无法生成定量的 pass_rate / token 消耗报告。

---

## 2. 设计目标与非目标

### 2.1 目标 (Goals)
1. 提供 `spawn_subagent` 工具，让主代理能派生**上下文隔离**的子代理执行任务
2. 子代理执行完毕后，**自动返回** `output_summary` + `usage { total_tokens, duration_ms }`
3. 子代理支持 `system_prompt_patch` 注入，供 A/B 测试使用
4. 子代理拥有独立的 `workspace` 文件路径，实现文件系统级别隔离
5. 与 skill-creator 的 `grader`、`comparator`、`analyzer` agent 指令兼容

### 2.2 非目标 (Non-Goals)
- 子代理之间的实时通信 / 消息传递（本期不做）
- 子代理的 Docker / 容器级隔离（后续考虑）
- 子代理的 GPU 资源调度
- 前端 UI 对子代理的深度可视化（本期仅显示进度指示）

---

## 3. 架构设计

### 3.1 整体架构

```
┌─────────────────────────────────────────────────────┐
│                    主代理 (Parent)                     │
│  ┌───────────────────────────────────────────────┐  │
│  │  skill-creator SKILL.md 指令                    │  │
│  │  → 调用 spawn_subagent(task, prompt_patch, ws) │  │
│  └───────────────┬───────────────────────────────┘  │
│                  │ Tool Execute                       │
├──────────────────┼──────────────────────────────────┤
│   SpawnSubagentTool::execute()                       │
│   ┌──────────────▼──────────────┐                    │
│   │  1. 创建临时 Session          │                    │
│   │  2. 构建 system_prompt        │                    │
│   │     (base + prompt_patch)    │                    │
│   │  3. 构建 AgentConfig          │                    │
│   │     (workspace, tools, model) │                    │
│   │  4. agent_runtime.run(...)    │                    │
│   │  5. 收集 TurnResult           │                    │
│   │  6. 返回 summary + usage      │                    │
│   └─────────────────────────────┘                    │
│                                                       │
│   ┌─ 子代理 A (with_skill) ──┐  ┌─ 子代理 B (baseline)──┐ │
│   │ 独立 Session              │  │ 独立 Session           │ │
│   │ 独立 message history      │  │ 独立 message history   │ │
│   │ 隔离 workspace            │  │ 隔离 workspace         │ │
│   │ 注入 skill prompt_patch  │  │ 无 prompt_patch        │ │
│   └──────────────────────────┘  └────────────────────────┘ │
└─────────────────────────────────────────────────────┘
```

### 3.2 核心组件

#### 3.2.1 SpawnSubagentTool (新增)

**位置**：`src/tool/builtin/subagent.rs`

**实现 `Tool` trait**：

```rust
pub struct SpawnSubagentTool {
    /// 用于创建子代理运行时的工厂配置
    default_model_config: ModelConfig,
    /// 子代理可用的工具白名单
    allowed_tools: Vec<String>,
    /// 最大迭代次数限制（防止子代理无限循环）
    max_iterations: usize,
}
```

**输入 Schema**：

```json
{
  "type": "object",
  "properties": {
    "task": {
      "type": "string",
      "description": "子代理需要完成的任务描述，将作为 user message 发送"
    },
    "system_prompt_patch": {
      "type": "string",
      "description": "追加到 base system prompt 的内容（如 Skill 指令）。为空时使用裸模型"
    },
    "workspace": {
      "type": "string",
      "description": "子代理的隔离工作目录绝对路径。工具的文件操作限制在此目录内"
    },
    "model": {
      "type": "string",
      "description": "可选，指定子代理使用的模型 ID。默认继承主代理配置"
    },
    "max_iterations": {
      "type": "integer",
      "description": "可选，子代理最大迭代轮次，默认 10"
    },
    "tools": {
      "type": "array",
      "items": { "type": "string" },
      "description": "可选，子代理可使用的工具名列表。默认 [\"bash\", \"read_file\", \"write_file\"]"
    }
  },
  "required": ["task"]
}
```

**输出 Schema**：

```json
{
  "type": "object",
  "properties": {
    "output_summary": {
      "type": "string",
      "description": "子代理最终 assistant message 的完整文本内容"
    },
    "usage": {
      "type": "object",
      "properties": {
        "total_tokens": { "type": "integer", "description": "总 token 消耗（input + output）" },
        "duration_ms": { "type": "integer", "description": "从启动到完成的总耗时（毫秒）" },
        "input_tokens": { "type": "integer" },
        "output_tokens": { "type": "integer" },
        "iterations": { "type": "integer", "description": "实际执行的迭代轮次" }
      }
    },
    "files_created": {
      "type": "array",
      "items": { "type": "string" },
      "description": "子代理在 workspace 中创建或修改的文件路径列表"
    },
    "error": {
      "type": "string",
      "description": "如果执行失败，包含错误信息。成功时为 null"
    }
  }
}
```

#### 3.2.2 执行流程

```
spawn_subagent 被调用
│
├─ 1. 参数校验
│     ├─ task 非空
│     ├─ workspace 路径合法且可写（不存在则创建）
│     └─ max_iterations ∈ [1, 50]
│
├─ 2. 构建子代理环境
│     ├─ 生成临时 session_id (UUID)
│     ├─ 组装 system_prompt = base_prompt + system_prompt_patch
│     ├─ 实例化 ToolRegistry（仅注册 allowed tools）
│     │   └─ BashTool: cwd 设为 workspace
│     │   └─ ReadFileTool / WriteFileTool: 根路径限制为 workspace
│     └─ 构建 AgentConfig { max_iterations, model, tool_timeout }
│
├─ 3. 执行子代理
│     ├─ 记录 start_time = Instant::now()
│     ├─ 创建 AgentRuntime 实例
│     ├─ runtime.run(user_message=task, system_prompt, tools)
│     ├─ 收集所有 TurnResult（可能多轮迭代）
│     └─ 记录 end_time = Instant::now()
│
├─ 4. 汇总结果
│     ├─ output_summary = 最后一个 assistant message 的 content
│     ├─ total_tokens = Σ turn.usage.input_tokens + Σ turn.usage.output_tokens
│     ├─ duration_ms = (end_time - start_time).as_millis()
│     ├─ iterations = turn_results.len()
│     └─ files_created = diff(workspace 前后文件列表)
│
└─ 5. 返回 ToolOutput::Text(json)
```

#### 3.2.3 与现有系统的集成点

| 集成点 | 文件 | 改动说明 |
|--------|------|----------|
| 工具注册 | `src/tool/builtin/mod.rs` | 在 `register_builtin_tools()` 中注册 `SpawnSubagentTool` |
| Agent 运行时 | `src/agent.rs` | 无需修改，子代理复用 `AgentRuntime::run()` |
| LLM Provider | `src/provider/` | 无需修改，子代理通过相同 provider 发起 API 调用 |
| Skill 注入 | `src/skill.rs` | 无需修改，`system_prompt_patch` 直接拼接而非走 SkillRegistry |
| Gateway 协议 | `src/gateway/protocol.rs` | 可选：新增 `SubagentProgress` 事件类型用于前端展示 |
| 前端事件 | `deskapp/src/core/types.ts` | 可选：扩展 `ProgressEvent.type` 增加 `subagent_start` / `subagent_complete` |

### 3.3 关键设计决策

#### 决策 1：进程内执行 vs. 独立进程

| 方案 | 优点 | 缺点 |
|------|------|------|
| **进程内（推荐）** | 无 IPC 开销；可直接复用 `AgentRuntime`、`LlmClient`、连接池 | 共享内存空间，极端情况下 panic 会影响主进程 |
| 独立进程 | 完全隔离 | 需要序列化/反序列化、启动开销大、配置传递复杂 |

**决定**：采用**进程内方案**。子代理通过独立的 `AgentRuntime` 实例运行，共享 LLM provider 的 HTTP client 连接池，但拥有独立的 Session 状态和工具注册表。通过 `catch_unwind` 防止子代理 panic 传播。

#### 决策 2：同步阻塞 vs. 异步非阻塞

**决定**：采用**异步非阻塞**。`SpawnSubagentTool::execute()` 本身是 `async fn`，子代理的 `runtime.run()` 也是异步的。主代理在 `await` 子代理结果期间不会阻塞 tokio 线程池。

当 skill-creator 需要并行运行 A/B 两组时，主代理可以：
1. 先调用 `spawn_subagent` (A)，等待返回
2. 再调用 `spawn_subagent` (B)，等待返回

> 注：由于工具调用是模型驱动的，真正的并行需要模型在一次 response 中发起两个 tool_use。当前先支持串行，后续可在模型支持 parallel tool use 时自动获得并行能力。

#### 决策 3：Workspace 隔离策略

```
evals/
└── eval-001/
    ├── runs/
    │   ├── with_skill/     ← 子代理 A 的 workspace
    │   │   ├── outputs/    ← 子代理生成的文件
    │   │   ├── transcript.md
    │   │   ├── metrics.json
    │   │   └── grading.json
    │   └── baseline/       ← 子代理 B 的 workspace
    │       ├── outputs/
    │       ├── transcript.md
    │       ├── metrics.json
    │       └── grading.json
    └── comparison.json
```

- 子代理的 `BashTool` 的 `cwd` 设为 `workspace/outputs/`
- 子代理的 `WriteFileTool` 写入路径必须在 `workspace/` 下（路径校验）
- 子代理完成后，主代理可读取 workspace 中的文件进行评分

#### 决策 4：子代理的 Tool 集合

默认提供最小工具集，避免子代理执行危险操作：

| 工具 | 默认启用 | 说明 |
|------|----------|------|
| `bash` | ✅ | cwd 限定为 workspace |
| `read_file` | ✅ | 可读 workspace 内外的文件（需要读取 skill 文件等） |
| `write_file` | ✅ | 限定写入 workspace 内 |
| `web_search` | ❌ | 按需启用 |
| `web_fetch` | ❌ | 按需启用 |
| `spawn_subagent` | ❌ | **禁止递归派生**（本期不支持） |

---

## 4. 与 skill-creator 评测流水线的对接

### 4.1 评测执行阶段 (Executor)

skill-creator 的 SKILL.md 指令中，执行评测的步骤可改写为：

```
对于 evals.json 中的每个 eval:
  1. spawn_subagent(
       task = eval.prompt,
       system_prompt_patch = skill_content,   // with_skill 组
       workspace = "evals/eval-{id}/runs/with_skill"
     )
     → 获取 result_A { output_summary, usage }

  2. spawn_subagent(
       task = eval.prompt,
       system_prompt_patch = null,            // baseline 组
       workspace = "evals/eval-{id}/runs/baseline"
     )
     → 获取 result_B { output_summary, usage }

  3. 将 output_summary 写入 workspace/outputs/output.md
  4. 将 usage 写入 workspace/timing.json
```

### 4.2 评分阶段 (Grader)

评分仍由主代理（或另一个 subagent）完成：

```
spawn_subagent(
  task = "按照 grader.md 的指令，评分以下输出...",
  system_prompt_patch = grader_agent_instructions,
  workspace = "evals/eval-{id}/runs/with_skill"
)
→ 生成 grading.json
```

### 4.3 Metrics 采集

`spawn_subagent` 的返回值直接提供 `timing.json` 所需字段：

```json
{
  "total_tokens": result.usage.total_tokens,
  "duration_ms": result.usage.duration_ms,
  "executor_start": "...",
  "executor_end": "..."
}
```

这彻底解决了**问题 3（Metrics Gap）**，因为 token/时间数据由工具实现层直接捕获，无需主代理从流式事件中解析。

### 4.4 对比与分析阶段

```
// 盲评对比
spawn_subagent(
  task = "按照 comparator.md 的指令，盲评 Output A 和 Output B...",
  system_prompt_patch = comparator_agent_instructions,
  workspace = "evals/eval-{id}"
)
→ 生成 comparison.json

// 汇总分析
python -m scripts.aggregate_benchmark evals/ --skill-name "xxx"
→ 生成 benchmark.json + benchmark.md

// 模式分析
spawn_subagent(
  task = "按照 analyzer.md 的指令，分析 benchmark.json...",
  system_prompt_patch = analyzer_agent_instructions,
  workspace = "evals/"
)
→ 生成 analysis.json
```

---

## 5. 错误处理

| 场景 | 处理方式 |
|------|----------|
| 子代理 API 调用失败（网络/限流） | 返回 `error` 字段，`output_summary` 为空，主代理决定是否重试 |
| 子代理超过 `max_iterations` | 强制终止，返回已有的最后一轮结果 + `error: "max iterations reached"` |
| 子代理 panic | `catch_unwind` 捕获，返回 `error: "internal panic: ..."` |
| workspace 路径不可写 | 工具执行前校验，返回明确错误信息 |
| 子代理执行超时 | 可选 `timeout_ms` 参数，默认 5 分钟，通过 `tokio::time::timeout` 实现 |

---

## 6. 跨平台策略 (Windows 适配)

针对**问题 2（OS Mismatch）**，采用分层解决：

1. **Workspace 路径标准化**：子代理内部统一使用正斜杠路径，BashTool 在 Windows 上通过 MSYS2/Git Bash 执行
2. **平台感知的 eval 标记**：`evals.json` 中增加可选 `platform` 字段
   ```json
   { "id": 1, "prompt": "...", "platform": "linux" }
   ```
   主代理在 Windows 上遇到 `platform: "linux"` 的 eval 时，可选择：
   - 跳过并标记 `skipped`
   - 通过 WSL2 执行（如果可用）
   - 以 `--dry-run` 模式执行，仅验证逻辑不执行系统命令
3. **长期方案**：结合 Docker sandbox（`config.toml` 中已有 `sandbox.docker` 配置项），为子代理提供 Linux 容器环境

---

## 7. 实施计划

### Phase 1：核心工具实现（MVP）
- [ ] 新建 `src/tool/builtin/subagent.rs`，实现 `SpawnSubagentTool`
- [ ] 在 `src/tool/builtin/mod.rs` 中注册工具
- [ ] 实现基本的 workspace 创建和路径校验
- [ ] 实现 usage 指标采集（tokens, duration_ms, iterations）
- [ ] 单元测试：验证隔离性、指标准确性

### Phase 2：skill-creator 集成
- [ ] 更新 skill-creator 的 SKILL.md，在评测步骤中使用 `spawn_subagent`
- [ ] 适配 `grader`、`comparator`、`analyzer` agent 指令为 subagent 模式
- [ ] 端到端测试：完整运行一个 Skill 的评测流水线

### Phase 3：前端集成与体验优化
- [ ] Gateway 协议扩展 `SubagentProgress` 事件
- [ ] 前端 `ProgressEvent` 支持子代理状态显示
- [ ] 子代理执行日志的 `LogDelta` 事件转发

### Phase 4：高级特性（后续）
- [ ] 支持 parallel tool use 时的并行子代理
- [ ] Docker sandbox 集成
- [ ] 子代理递归派生（有深度限制）
- [ ] 子代理结果缓存（相同 task + prompt_patch 跳过重复执行）

---

## 8. 测试与验证

### 8.1 隔离性验证
- 子代理 A 在 workspace_A 中创建文件 → 主代理验证 workspace_B 中无该文件
- 子代理的 message history 不包含主代理的对话内容
- 子代理的 system_prompt 仅包含 base + patch，不包含主代理的其他 Skill

### 8.2 指标准确性验证
- 对比 `spawn_subagent` 返回的 `total_tokens` 与 LLM provider 侧的实际计费 token 数
- 验证 `duration_ms` 覆盖从 API 调用到结果返回的完整耗时

### 8.3 评测流水线端到端
- 使用一个简单的测试 Skill（如 "always respond in JSON"）
- 运行完整的 eval → grade → compare → analyze 流程
- 验证生成的 `benchmark.json` 结构符合 `references/schemas.md` 定义

### 8.4 回归测试
- 确保新增工具不影响现有的 `bash`、`read_file`、`write_file` 行为
- 确保 Gateway 协议的向后兼容性

---

> [!NOTE]
> 本设计的核心思路是：**将子代理执行内化为一个标准的 Tool**，复用现有的 `AgentRuntime` 和 LLM Provider 基础设施，以最小改动量解决 skill-creator 评测流水线的三个阻塞性问题。
