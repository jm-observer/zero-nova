# Orchestrator Read Loop Guard

| 章节 | 内容 |
|---|---|
| 时间 | 创建：2026-05-08；最后更新：2026-05-08 |
| 项目现状 | 当前 orchestrator / agent 运行时允许模型在单个 turn 内进行多轮 `Read`、`Bash`、`Agent` 等工具调用，但缺少“重复调用检测”“单轮内历史收敛”“长文本读取摘要化”三类保护。一次失败案例中，同一文件被连续读取十余次，assistant 分析文本也被反复追加回上下文，最终形成高成本的重复 `Read` 循环。 |
| 整体目标 | 为 orchestrator 和通用 agent 增加面向单轮执行的循环保护机制：在不破坏正常探索能力的前提下，主动识别重复读取、限制无效重试、压缩长文件回显，并让模型在进入重复模式前得到明确反馈或被运行时终止。 |
| Plan 拆分 | 1. **Plan 1: 单轮循环检测与熔断** - 在 `AgentRuntime` 内识别重复 tool call、重复 reasoning 模式与无进展 iteration，并提供 warning / hard stop。依赖：无。状态：已完成。<br>2. **Plan 2: Read 输出收敛与上下文预算控制** - 为 `Read` 增加重复读取提示、可选摘要模式，并在单轮 iteration 间引入增量裁剪。依赖：Plan 1。状态：待开始。<br>3. **Plan 3: 观测、配置与测试补齐** - 暴露循环保护指标、可配置阈值，补充单测和集成测试。依赖：Plan 1、Plan 2。状态：待开始。 |
| 风险与待定项 | 1. 过严的重复检测可能误伤合法的分页读取与逐段分析。<br>2. 如果只在 `Read` 工具侧截断，不同步优化 runtime 历史裁剪，仍可能因重复 assistant 文本而膨胀上下文。<br>3. 循环保护策略需要对 orchestrator 和普通 developer/nova 会话保持一致语义，否则用户很难理解何时会被拦截。<br>4. `ReadTool` 必须保持无状态，turn-scoped 读取历史不能挂在全局共享 tool 实例上。 |

## 项目现状

当前“重复 `Read` 循环”不是单点 bug，而是以下机制叠加后的系统性问题：

1. `AgentRuntime::run_turn_with_model_config` 与 `run_turn_with_context_and_model_config` 在单个 turn 的 iteration 循环中持续向 `all_messages` 追加 assistant 输出、tool use 和 tool result，但不会在 iteration 之间再次裁剪。
2. `ReadTool` 返回的是带行号的原始文本，默认可达 2000 行；工具本身不会告知“这份文件你刚读过”，也不会自动提供摘要化结果。
3. `read_files` 当前仅用于 `Edit` / `Write` 的“先读再改”校验，不承担去重或循环检测职责。
4. orchestrator skill 对 `Read` / `Bash` / `OrchestrateTask` 都开放，但提示词没有明确要求在重复读取前先复用已有结果，也没有运行时兜底。
5. `HistoryTrimmer` 只在 `prepare_turn()` 期间处理进入 turn 前的历史，不能抑制单个 turn 内部的消息膨胀。

结果是：当模型没有获得足够强的中间约束时，它会把“再次读一下确认”当成低风险动作；而运行时会把这些重复分析和大段原文完整保留，进一步放大模型继续重复读取的倾向。

## 整体目标

本次修复目标不是简单限制 `Read` 次数，而是定义一套“探索允许，但循环要可控”的运行时收敛策略：

1. 在 **工具调用前** 检测“是否与最近若干轮输入等价”，优先提醒或阻断重复动作。
2. 在 **工具结果层** 避免大段原始文本反复灌入上下文，优先输出结构化提示、摘要、分页元信息和缓存命中信息。
3. 在 **iteration 层** 控制 `all_messages` 的增长速度，避免单个 turn 因重复工具调用而失控。
4. 在 **观测层** 明确记录“命中循环保护”的原因，便于调试阈值、回放问题和分析误判。

实现边界同步明确如下：

1. `ReadTool` 保持 stateless；已读范围、重复计数和命中缓存属于 `ToolContext` 或 `AgentRuntime` 的 turn-scoped 局部状态。
2. loop guard 逻辑必须拆到独立模块，`agent.rs` 只保留主循环编排和少量调用胶水代码。
3. assistant 文本指纹采用极轻量启发式，不引入高成本字符串相似度算法。
4. `ReadTool` 的输入 schema 和输出协议需要更强的“可继续分页”信号，包括显式默认值、总行数、返回范围、`has_more` 和 `next_offset`。
5. 被拒绝的重复调用必须返回带操作指引的 guard 文案，而不是只有笼统错误。

目标状态如下：

```text
模型生成 tool call
    ↓
单轮循环检测器检查近邻重复 / 无进展模式
    ↓
允许执行 or 注入 warning / 拒绝执行
    ↓
ReadTool 返回原文 / 摘要 / 重复命中提示
    ↓
runtime 对 iteration 历史做增量收敛
    ↓
日志 / 事件 / 测试可观测
```

## Plan 拆分

### Plan 1: 单轮循环检测与熔断

- 在 `AgentRuntime` 内新增 turn-scoped 的调用轨迹结构，记录最近若干次 tool call 的规范化签名。
- 规范化签名至少包含：工具名、关键输入字段、文件路径规范化结果、offset/limit 等分页参数。
- 对连续重复的完全相同 tool call 增加阈值控制：
  - 第 1 次重复：允许执行，但在 tool result 前注入 warning；
  - 超过阈值：返回受控错误或拒绝执行，并提示模型改用已有结果继续分析。
- 增加“无进展 iteration”检测：
  - 连续多轮仅出现近似相同的 assistant 文本和相同 tool call 组合时，直接终止该 turn；
  - 终止原因通过事件与日志明确输出。

### Plan 2: Read 输出收敛与上下文预算控制

- 为 `ReadTool` 增加重复读取感知：
  - 同一文件、同一 offset/limit 被重复请求时，优先返回“已读提示 + 已读范围摘要”，而不是再次完整回显原文；
  - 合法分页读取（如 `offset` 递增）必须继续允许。
- 为长文本读取引入显式模式：
  - 默认保留原始读取能力；
  - 新增可选摘要模式或自动摘要分支，用于超长文件和重复读取场景。
- 在 `run_turn_with_context_and_model_config` / `run_turn_with_model_config` 的 iteration 末尾增加增量裁剪钩子：
  - 对超过预算的 `all_messages` 做收敛；
  - 优先保留最新 assistant/tool 成对消息，避免打断语义闭环。

### Plan 3: 观测、配置与测试补齐

- 在 `gateway` 或 runtime 配置中引入具名阈值：
  - 最大连续相同 tool call 次数；
  - 最大无进展 iteration 次数；
  - 重复读取时是否只警告或直接拒绝。
- 记录结构化观测信息：
  - `loop_guard_triggered`
  - `duplicate_read_hit`
  - `iteration_trimmed`
  - 对应原因、工具名、文件路径、命中阈值。
- 补充单元测试和集成测试，覆盖正常探索、分页读取、重复读取、误判回归和 turn 熔断行为。

## 风险与待定项

1. 分页读取与重复读取的边界需要准确定义。`file_path` 相同但 `offset` 不同通常是合法探索，不能直接判为循环。
2. `ReadTool` 当前是全局共享的内置工具实例，不能把 turn 内部的读取范围状态直接塞进 `ReadTool` 结构体，否则会引入跨 session 污染和额外同步复杂度。
3. assistant 文本“相似度”检测若实现过重，可能引入额外 CPU 开销并阻塞 Tokio worker；首版应优先使用轻量规则，如“前 64 个字符 hash + 文本长度 + tool call 签名”。
4. 当前 `ReadTool` 默认 `limit=2000` 对模型上下文过于激进；若不显式下调并暴露分页元信息，重复读取和上下文膨胀都更容易发生。
5. 若将拒绝执行设计为工具错误，需要确保模型能够从错误中恢复，而且错误消息要明确告诉模型下一步该怎么做，避免继续盲目重试。
6. 对历史进行 iteration 级增量裁剪时，必须保留 tool use / tool result 对应关系，避免生成断裂上下文。
