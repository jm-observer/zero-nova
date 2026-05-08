# Plan 2: Read 输出收敛与上下文预算控制

| 章节 | 内容 |
|---|---|
| Plan 编号与标题 | Plan 2: Read 输出收敛与上下文预算控制 |
| 前置依赖 | Plan 1 |
| 本次目标 | 减少重复 `Read` 带来的上下文膨胀，让模型优先消费“已读摘要”和最新必要信息，而不是反复接收同一段原始文件内容。 |
| 涉及文件 | `crates/nova-agent/src/tool/builtin/read.rs`、`crates/nova-agent/src/agent.rs`、`crates/nova-agent/src/prompt.rs`、必要时新增 `crates/nova-agent/src/tool/read_cache.rs` |

## 详细设计

### 1. Read 工具的收敛职责

`ReadTool` 当前只有“读取文件并返回原文”这一职责，缺少对以下情形的区分：

- 首次读取
- 同范围重复读取
- 同文件不同范围分页读取
- 超长文件读取

首版设计中，`ReadTool` 需要能根据上下文返回不同层级的信息。

这里必须先约束状态归属：

1. `ReadTool` 保持 stateless，不在工具实例内部存储 turn 或 session 级可变状态。
2. 已读范围跟踪应挂在单次执行上下文中，由 `ToolContext` 或 `AgentRuntime` 的局部状态承载。
3. 不引入跨 session 的全局 `DashMap<SessionId, ...>` 风格缓存，避免状态污染和同步复杂度上升。

除状态归属外，`ReadTool` 还需要补足“分页协议可理解性”：

1. 输入 schema 需要显式声明 `offset` / `limit` 的默认值。
2. 返回结果需要包含总长度与下一页信息，避免模型反复读取文件开头。
3. 默认分页尺寸需要更保守，优先鼓励分页浏览而不是一次灌入超长正文。

### 2. 已读范围跟踪

现有 `read_files` 只记录“这个文件读过”，粒度太粗，无法区分分页与重复。

建议新增 turn-scoped 读取记录结构，并将其挂在 `ToolContext` 可达的局部状态上：

- `canonical_path`
- `ranges: Vec<ReadRange>`
- `last_excerpt_fingerprint`

`ReadRange` 包含：

- `offset_start`
- `offset_end`
- `returned_line_count`

如果现有 `ToolContext` 不便直接扩展，可由 `AgentRuntime` 持有 `Arc<RwLock<TurnToolState>>`，再在调用 `tool.execute(..., ctx)` 时把该状态引用透传给 `ReadTool`。

### 2.1 输入参数协议优化

当前实现虽然在代码里隐式使用：

- `offset` 默认 `1`
- `limit` 默认 `2000`

但这些默认值没有在 tool schema 中稳定暴露，模型很容易省略参数并重复读取同一段开头内容。

建议将输入协议调整为：

- `offset`
  - 明确为 **1-based 起始行号**
  - schema 中显式写出 `default: 1`
  - description 中强调“继续向后读取时应显式增大 offset”
- `limit`
  - schema 中显式写出默认值
  - 默认值从 `2000` 下调到更保守的区间，例如 `200` 或 `300`
  - 仍允许通过上限控制支持更大分页

如果暂时不修改字段名，至少要在说明中反复强调：`offset` 是“起始行号”，不是页码，也不是字节偏移。

### 3. 输出策略

建议按以下顺序判定：

1. **首次读取**  
   返回带行号原文，并附带结构化元信息：
   - `file_path`
   - `total_lines`
   - `returned_range`
   - `has_more`
   - `next_offset`

2. **合法分页读取**  
   若 `offset/limit` 与历史范围不重叠或仅小部分重叠，返回新片段原文，并附带新的 `returned_range` / `next_offset`，同时在前缀提示“继续读取同一文件的新范围”。

3. **同范围重复读取**  
   返回受控文本，而不是再次完整回显：
   - 已读提示
   - 上次读取范围
   - 文件总行数
   - 可直接使用的 `next_offset`
   - 建议模型复用已有结果继续分析
   - 可选附带简短摘要

4. **超长重复读取**  
   强制摘要模式，不再返回完整原文。

返回协议不要求立即切到纯 JSON，但至少应采用“元信息头 + 正文”的稳定格式，让模型能可靠提取：

- 当前读到了哪里
- 文件是否还有后续
- 下一次应传什么参数

### 4. 摘要模式

摘要模式不要求一开始就引入复杂 summarizer。首版可以使用轻量规则摘要：

- 文件路径
- 总行数
- 当前请求范围
- 下一推荐偏移量
- 片段首尾各若干行
- “此范围已在本 turn 内返回过一次”

这样可以显著减少 token，同时保留定位能力。

### 5. 单轮 iteration 增量裁剪

`HistoryTrimmer` 当前只在 `prepare_turn()` 时应用一次。Plan 2 需要在 `run_turn_*` 的 iteration 循环中增加增量裁剪步骤。

建议流程：

1. 工具结果写入 `all_messages`
2. 估算当前 turn token
3. 若超出 iteration 安全预算，则调用新 helper 做局部裁剪

局部裁剪规则：

- 优先保留最近一组 assistant/tool/result
- 优先删除更早的重复 `Read` 原文消息
- 必须保证 tool use 与 tool result 成对保留或成对删除

如果 `Read` 已经输出结构化元信息，则裁剪时应优先保留：

- 最近一次 `returned_range`
- `total_lines`
- `has_more`
- `next_offset`

而不是优先保留整段原文正文。

### 6. 与 Plan 1 的协同

Plan 1 负责“阻止完全无效的重复调用”，Plan 2 负责“就算允许重复，也不要再次灌入大量冗余文本”。两者不能互相替代。

另外，Plan 2 不负责决定“是否熔断整个 turn”。它只负责：

- 查询 turn-scoped read history
- 判定当前应返回原文、提示还是摘要
- 将新的读取范围写回 turn-scoped 状态

真正的 reject / 熔断决策仍由 Plan 1 的 loop guard 给出。

## 测试案例

1. 同一文件首次读取：返回原文，带正确行号。
2. 同一文件不同 `offset` 连续读取：返回新范围原文，不触发重复命中。
3. 省略 `offset` / `limit` 时：schema 与返回内容都能明确告诉模型实际采用的默认值。
4. 同一文件同一范围再次读取：返回已读提示，而不是完整原文，并附带 `next_offset`。
5. 超长文件重复读取：返回摘要模式内容，输出长度明显小于首次读取。
6. 单个 turn 内多次 `Read` 后触发 iteration 裁剪：仍能保留最近工具对话闭环。
7. 裁剪后继续执行 `Edit`：不会因为删除了必要读取记录而破坏“先读再改”约束。
8. 两个不同 session 并发读取相同路径：read history 不会互相污染。
