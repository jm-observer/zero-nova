# DeskApp Agent 工作台能力概要设计

**时间**: 2026-04-26（创建）/ 2026-04-26（最后更新）

> **文档编写进度**：Plan 1-5 详细设计已完成；原 Plan 5 与 Plan 6 已合并为新的 Plan 5。

## 项目现状

`deskapp` 当前已经具备以下基础：

- 前端通过 [deskapp/src/gateway-client.ts](/D:/git/zero-nova/deskapp/src/gateway-client.ts) 与 Gateway 建立 WebSocket 通信，并已封装 `agents.list`、`config.get`、`memory.*`、`evolution.skills.list`、`evolution.tools.list` 等接口。
- 设置页模板 [deskapp/src/ui/templates/settings-template.ts](/D:/git/zero-nova/deskapp/src/ui/templates/settings-template.ts) 已有 `models`、`tools`、`memory` 分区，但目前偏向”配置项编辑”，缺少围绕单个 Agent/会话的运行态观察与临时控制。
- 全局状态 [deskapp/src/core/state.ts](/D:/git/zero-nova/deskapp/src/core/state.ts) 目前主要覆盖会话、消息、Agent 列表、附件、MCP Server 等基础数据，尚未承载”当前模型快照、token 使用、运行工具清单、技能快照、Prompt 展开结果、记忆命中详情”等可观测信息。
- 已存在的能力中，`memory` 与 `evolution skills/tools` 更接近”后台管理入口”；用户在聊天主流程里还不能直观看到”当前 Agent 正在用什么模型、能调用哪些工具、继承了哪些技能、命中了哪些记忆、实际 Prompt 是什么”。
- **已有的运行态基础设施**（新增本节，避免设计时重复造轮子）：
  - **流式事件**：`ProgressEvent` 已支持 `iteration`、`thinking`、`tool_start`、`tool_result`、`tool_log`、`token`、`turn_complete`、`iteration_limit` 等类型，前端 `chat-view.ts` 已对这些事件做渲染处理。
  - **Token 统计**：后端 `ChatCompletePayload.usage` 已返回单次回复的 `Usage { input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens }`，但目前前端未展示，且不区分 orchestration/execution。
  - **停止控制**：`gateway-client.ts` 已有 `stopTask(sessionId)` 方法。
  - **Skill 事件**：后端已推送 `SkillActivated`、`SkillSwitched`、`SkillExited`、`ToolUnlocked`、`SkillRouteEvaluated`、`SkillInvocation` 等事件。
  - **确认机制**：已有 `onEvolutionConfirm` / `respondEvolutionConfirm` 用于技能安装确认，可作为权限确认中心的参考范式。
  - **Artifact 类型**：前端 `types.ts` 已定义 `SessionArtifactView { id, type, path, filename, content, language, size, timestamp }`，`chat.css` 已有 `.artifacts-panel` 样式。
  - **任务追踪**：前端已定义 `TaskRunView { id, taskId, taskName, status, startedAt, completedAt, duration, output, error }` 和 `ScheduledTaskView`。
  - **Agent 模型字段**：`ServerConfigView.agents.list` 中每个 Agent 已有 `model?: { provider: string; model: string }` 字段。
  - **Rust 侧 Turn 上下文**：`nova-core` 已有 `TurnContext { system_prompt, tool_definitions, history, active_skill, capability_policy, skill_tool_enabled, max_tokens, iteration_budget }` 和 `TurnResult { messages, usage }`，但尚未通过协议暴露给前端。
- 当前桌面端还缺少成熟 Agent 产品常见的工作台能力，例如：
  - 运行控制（暂停、继续、停止）
  - 执行历史与运行态诊断
  - 产物统一查看与恢复
  - 权限确认与安全审计
  - 错误归因与修复引导
  - 应用重启后的上下文恢复

## 整体目标

为 `deskapp` 增加一组“Agent 工作台”能力，让用户在聊天、配置、执行控制与排障之间形成闭环：

- 能切换或覆盖当前 Agent / 当前会话所使用的 LLM。
- 能看到本轮/本会话 token 消耗与粗略成本估算。
- 能查看当前 Agent 可用的 tool 列表、skill 列表与来源。
- 能查看 memory 命中摘要与记忆库详情入口。
- 能查看最终送入模型的 Prompt 视图，包括系统提示词、技能拼装结果、运行时上下文摘要。
- 能控制当前执行过程，并查看任务状态、步骤、执行历史和失败原因。
- 能统一查看本次运行产生的文件、代码、输出内容等 artifact。
- 能对高风险操作进行权限确认，并查看审计记录。
- 能在异常场景下看到诊断信息、恢复建议和重新执行入口。
- 能在应用重启后恢复上次工作上下文。
- 保持 `deskapp` 现有 Tauri + TS 架构不大改，优先复用已有 Gateway API；对缺失的数据面，通过小规模新增协议补齐。

## 设计原则

1. 先做“可观测”，再做“可编辑”。例如先支持 Prompt 查看，再考虑 Prompt 在线编辑。
2. 区分“持久配置”和“临时覆盖”。模型切换必须明确作用域，避免误把会话级实验修改成全局默认。
3. 复用现有接口优先。已有 `memory.*`、`evolution.*`、`config.get/update` 的场景，不重复发明第二套数据结构。
4. 运行态信息以只读快照为主。避免前端直接拼装复杂业务逻辑，尽量由 Gateway 返回语义化视图对象。
5. 布局上不把这些能力全塞进 Settings；需要补一个贴近聊天主界面的“Agent Workspace / Agent Console”侧栏或抽屉。
6. 运行控制、安全确认、错误恢复必须是“一等公民”，不能作为零散弹窗附着在旧聊天界面上。

## 功能范围

### 1. LLM 切换

- 支持三层作用域：
  - 全局默认：复用 `config.update` 的 `llm.orchestration` / `llm.execution`。
  - Agent 默认：复用或扩展 `agents.list` 中已有的 `model` 字段。
  - 会话临时覆盖：新增会话级 runtime override，不回写全局配置。
- 前端展示当前生效来源：
  - `global`
  - `agent`
  - `session_override`
- 提供“恢复继承”动作，而不是只允许手工再选一次。

### 2. Token 统计

- 展示维度：
  - 单次回复输入/输出 token
  - 本轮工具调用累计 token
  - 当前会话累计 token
  - 可选：基于模型单价的粗略成本估算
- 数据来源优先级：
  - 优先消费后端 `ChatCompletePayload.usage` 中已有的 `Usage { input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens }`
  - 后端未提供时，仅展示”未知”而不在前端猜测
- **注意**：当前后端 `Usage` 不区分 orchestration / execution，因为 `ChatCompletePayload` 只返回一个聚合 `Usage` 对象。若要按模型类型拆分统计，需要后端在 turn 执行过程中分别记录两类调用的 usage 并通过新协议返回。首期可先展示聚合值，后续按需拆分。
- Cache token（`cache_creation_input_tokens` / `cache_read_input_tokens`）应在 UI 中展示，因为这会影响实际成本。

### 3. Tool 列表

- 展示当前 Agent 在本轮可调用的工具快照，而不是仅展示系统全量工具。
- 分类来源：
  - 内建工具
  - MCP 服务端工具
  - MCP 客户端工具
  - 自定义工具 / evolution tools
  - 技能解锁工具（通过 `ToolUnlocked` 事件动态启用的工具）
- 每个工具至少展示：
  - 名称
  - 来源
  - 描述
  - 参数 schema 摘要（需对齐后端 `ToolDefinition` 的 `input_schema` 结构）
  - 当前可用状态
- **注意**：后端已有 `ToolUnlockedPayload`，记录工具解锁来源（ToolSearch / skill activation / manual），前端应区分"初始可用"和"运行中解锁"两种状态。

### 4. Skill 列表

- 展示当前 Agent 实际挂载的 skill 集合，而不是仅展示“已安装技能”总表。
- 区分：
  - 全局技能
  - Agent 绑定技能
  - 运行期自动注入技能
- 提供“查看内容”与“查看来源”两种只读入口，避免首期就引入复杂编辑器。

### 5. Memory 查看

- 复用现有 `memory.list/search/stats` 能力，增加与当前会话关联的“命中视图”。
- 分为两层：
  - 会话侧：展示本轮命中的 memory 摘要、相似度、命中原因
  - 设置侧：保留当前全库浏览、搜索、删除、清空能力
- 目标是让用户知道“为什么 Agent 会这么回答”，而不仅是“系统里有记忆”。

### 6. Prompt 查看

- 展示发送给模型前的最终 Prompt 视图，至少拆成：
  - system prompt
  - skills 注入片段
  - memory 注入摘要
  - tools 说明片段
  - conversation/context 摘要
- 对于含敏感信息字段的部分，后端可以返回脱敏版与完整版两种模式；桌面端默认展示脱敏版。
- 首期只做查看与复制，不做在线编辑。

### 7. 运行控制与任务状态

- 为当前会话中的执行任务提供显式状态：
  - `queued`
  - `running`
  - `waiting_user`
  - `paused`（注意：首期可能仅支持 `stop`，见风险项）
  - `stopped`
  - `failed`
  - `completed`
- 支持的控制动作：
  - 停止当前任务（已有 `stopTask(sessionId)` 基础）
  - 对”等待确认 / 等待输入”的任务继续执行
  - 对可暂停任务提供暂停/继续入口
- 展示当前任务的阶段信息（可从已有 `ProgressEvent.type` 推导）：
  - 模型推理中（对应 `token` / `thinking` 事件）
  - 工具执行中（对应 `tool_start` 事件）
  - 等待工具返回（对应 `tool_start` 后未收到 `tool_result`）
  - 等待用户确认（对应 `evolution.confirm` 或新增权限确认事件）
  - 结果整理中（对应 `turn_complete` 但未收到 `complete`）
- **注意**：前端已有 `TaskRunView { id, taskId, taskName, status: 'running'|'completed'|'failed'|'paused'|'waiting_user', startedAt, completedAt, duration, output, error }`，状态枚举已扩展完成（含 `paused` 和 `waiting_user`），新状态机设计应沿用此类型。

### 8. 执行历史与任务计划

- 把每轮执行抽象成 `turn` / `run` 记录，保留：
  - 开始时间
  - 结束时间
  - 耗时
  - 使用模型
  - 使用工具
  - token 统计
  - 最终状态
  - 错误摘要
- 若 Agent 具备任务拆解能力，展示步骤树或 plan 列表。
- 支持“查看某次执行详情”与“基于当前输入重跑”。

### 9. Artifact / 输出物面板

- 统一展示本轮或本会话生成的：
  - 文件
  - 代码片段
  - 文本输出
  - 图片
  - 导出结果
- 每个 artifact 至少展示：
  - 类型
  - 名称
  - 生成时间
  - 来源任务
  - 文件路径或内容预览
- 与现有会话消息区分开，避免”文件结果埋在聊天气泡里找不到”。
- **注意**：前端已定义 `SessionArtifactView { id, type: 'file'|'code'|'output', path, filename, content, language, size, timestamp, runId?, turnId? }`，CSS 中已有 `.artifacts-panel` 及相关样式。`runId` 和 `turnId` 字段已补充完成。新设计应复用此类型，而非重新定义。

### 10. 权限确认与审计中心

- 对高风险动作做统一确认：
  - 本地命令执行
  - 文件写入/覆盖
  - 外部网络访问
  - 外部 MCP 工具调用
- 统一提供：
  - 当前待确认请求
  - 最近确认记录
  - 被拒绝记录
  - 触发来源（哪个 Agent / 哪个工具 / 哪个会话）
- 不将确认逻辑散落到多个无状态弹窗中。

### 11. 错误诊断与恢复

- 聚合当前会话或当前任务的异常：
  - 模型调用失败
  - MCP 不可用
  - memory 服务异常
  - prompt 预览失败
  - 权限被拒绝
  - 网关协议不支持
- 错误展示应包含：
  - 错误类别
  - 用户可读摘要
  - 建议动作
  - 重试入口或跳转入口

### 12. 工作上下文恢复

- 在应用重启后恢复：
  - 最近会话
  - 当前 Agent
  - Agent Console 打开状态和当前标签
  - 最近一次运行状态快照
  - 最近查看的 artifact
- 对未完成任务区分两种状态：
  - 可恢复观察
  - 不可恢复，仅保留历史记录与失败说明

## 信息架构

建议新增一个 `Agent Workspace` 交互区，入口位于聊天主界面右侧或顶部工具栏，而不是继续堆在 Settings 中。

建议拆分为 4 个视图层级：

1. **聊天页即时面板**
   - 当前模型
   - token 统计
   - 本轮工具
   - memory 命中摘要
   - Prompt 查看入口
   - 当前运行状态
   - 当前待确认请求

2. **Settings 中的系统配置页**
   - 全局模型默认值
   - Provider/API Key
   - MCP Server 管理
   - 记忆库管理

3. **Agent 详情抽屉/弹窗**
   - Agent 默认模型
   - 绑定技能
   - 默认 system prompt
   - 可用工具快照

4. **工作台二级面板**
   - 执行历史
   - Artifact 列表
   - 权限确认与审计
   - 错误诊断与恢复

## 需要补充的协议视图

当前接口已覆盖部分基础数据，但仍缺少”运行态聚合视图”。下面分”完全新增”和”在已有基础上扩展”两类列出。

### 完全新增的接口

- `agent.inspect`
  - 输入：`{ agentId, sessionId? }`
  - 输出：Agent 当前生效模型、skills、tools、prompt 摘要、memory 命中策略
  - 后端实现思路：聚合 `TurnContext` 中的 `system_prompt`、`tool_definitions`、`active_skill`、`capability_policy` 等字段
- `session.runtime`
  - 输入：`{ sessionId }`
  - 输出：会话级模型覆盖、token 累计、最近一次运行统计
- `session.prompt.preview`
  - 输入：`{ sessionId, messageId? }`
  - 输出：最终 Prompt 分段视图
  - 后端实现思路：调用 `agent.prepare_turn()` 获取 `TurnContext`，将其结构化序列化
- `session.tools.list`
  - 输入：`{ sessionId }`
  - 输出：本会话当前可用工具快照
  - 后端实现思路：从 `TurnContext.tool_definitions` 转换，附加来源分类
- `session.memory.hits`
  - 输入：`{ sessionId, turnId? }`
  - 输出：最近一次或指定轮次的记忆命中结果
  - 后端实现思路：需要在 memory 注入阶段记录命中结果，当前 runtime 未保存此信息
- `session.model.override`
  - 输入：`{ sessionId, orchestration?, execution?, reset?: boolean }`
  - 输出：覆盖后的会话运行配置
- `session.runs`
  - 输入：`{ sessionId, page?, pageSize? }`
  - 输出：会话执行历史列表
- `run.detail`
  - 输入：`{ runId }`
  - 输出：某次执行的详细步骤、工具记录、错误信息、artifact 引用

### 在已有基础上扩展的接口

- `session.artifacts`
  - 输入：`{ sessionId, runId?, type? }`
  - 输出：会话或某次运行的 artifact 列表
  - 扩展点：前端已有 `SessionArtifactView`，需补充 `runId` / `turnId` 字段
- `run.control`
  - 输入：`{ runId, action: “pause” | “resume” | “stop” }`
  - 输出：控制后的运行状态
  - 扩展点：`stop` 可复用已有 `chat.stop` / `stopTask(sessionId)` 逻辑；`pause/resume` 需后端新增可中断点支持
- `permission.pending` / `permission.respond`
  - 扩展点：已有 `evolution.confirm` / `respondEvolutionConfirm` 机制，可泛化为通用权限确认协议
  - `permission.pending` 输入：`{ sessionId? }`，输出：待确认请求列表
  - `permission.respond` 输入：`{ requestId, approved, remember? }`，输出：确认结果
- `audit.logs`
  - 输入：`{ sessionId?, type?, page?, pageSize? }`
  - 输出：权限确认与高风险操作审计记录
- `diagnostics.current`
  - 输入：`{ sessionId? }`
  - 输出：当前会话诊断摘要与建议动作
- `workspace.restore`
  - 输入：无
  - 输出：最近工作区恢复信息

### 新增事件推送

以下事件需明确与已有后端 `AppEvent` 枚举及前端 `EventBus` 事件的映射关系。

> **说明**：下表”已有基础”列指的是**后端 Gateway 推送事件**（Rust 侧 `AppEvent` 枚举），不是前端 `EventBus` 常量。前端接收后端推送后，通过 `gateway-client.ts` 的 handler 转换为前端 `EventBus` 事件（如 `Events.PROGRESS_UPDATE`、字符串字面量 `'tool:start'` 等）。两套命名体系的统一规范参见遗留问题文档。

| 新增事件 | 已有后端基础 | 说明 |
|---------|------------|------|
| `session.token.usage` | `ChatComplete` 中已有 `usage` | 需拆分为增量推送，而非仅在完成时返回 |
| `session.tools.updated` | `ToolUnlocked` 已有 | 需补充”工具移除/禁用”场景 |
| `session.memory.hit` | 无 | 完全新增，需后端在 memory 注入阶段埋点 |
| `run.status.updated` | `ProgressEvent` 已有（前端通过 `'tool:start'`/`'chat:complete'` 等字面量事件处理） | 需扩展状态枚举，增加 `paused`/`waiting_user`（`TaskRunView` 已包含） |
| `run.step.updated` | `ProgressEvent` 已有丰富的 kind 类型 | 可将现有 ProgressEvent 语义化包装 |
| `session.artifacts.updated` | 无 | 完全新增 |
| `permission.requested` | `EvolutionConfirm` 已有 | 泛化为通用权限请求事件 |
| `diagnostics.updated` | 无 | 完全新增 |
| `session.runtime.updated` | 无 | 完全新增，聚合模型覆盖变更等 |

这样前端不需要在每轮回复后做过多轮询。

## Plan 拆分

### Plan 1: 运行态控制台与信息架构

- 目标：定义聊天主界面中的 `Agent Console`，明确入口、布局、状态模型和交互边界。
- 依赖：无
- 顺序：第一步

### Plan 2: LLM 切换与 Token 统计

- 目标：设计全局 / Agent / 会话三级模型切换能力，以及 token 使用展示与统计来源。
- 依赖：Plan 1
- 顺序：第二步

### Plan 3: Tool / Skill / Memory / Prompt 可观测面

- 目标：设计四类 inspection 面板与只读详情视图，明确前后端数据契约。
- 依赖：Plan 1
- 顺序：第三步

### Plan 4: Gateway 协议补齐与测试方案

- 目标：整理需要新增的只读/控制接口、事件协议、状态同步机制以及前端测试矩阵。
- 依赖：Plan 2、Plan 3
- 顺序：第四步

### Plan 5: 运行工作流、权限诊断与工作区恢复

- 目标：统一设计 run/turn 模型、运行控制、执行历史、artifact 工作流、权限确认中心、审计记录、错误诊断和应用重启后的工作区恢复。
- 依赖：Plan 1、Plan 4
- 顺序：第五步

## 风险与待定项

- 当前 `gateway-client.ts` 已有 `memory` 与 `evolution` 管理接口，但”当前 Agent 实际装配结果”未必能直接从已有后端对象推导出来，可能必须由 Gateway 输出聚合视图。后端 `TurnContext` 包含了大部分所需信息，但目前只在 `prepare_turn()` 内部使用，需要新增协议将其暴露。
- token 统计依赖模型供应商是否稳定返回 usage；如果部分 provider 缺失，只能做”部分可见”，不能承诺全量精确。
- **orchestration / execution 拆分统计**：当前后端 `ChatCompletePayload` 只返回一个聚合 `Usage` 对象，不区分两类模型调用。若要拆分展示，需要后端在 `ConversationService::execute_agent_turn()` 中分别记录 orchestration 和 execution 各自的 usage，工作量不小。建议首期只展示聚合值。
- Prompt 查看涉及敏感信息泄露风险，需要明确哪些字段需要脱敏，例如 API key、环境变量、凭证型 memory。
- 会话级模型覆盖若与后端调度器、复制会话（`copySession`）、恢复历史等能力叠加，需要提前定义继承/清除规则，避免状态污染。
- 运行控制是否真正支持 `pause/resume`，取决于后端 runtime 是否具备可中断点；当前 `nova-core` 的 agent loop 没有显式 pause 点，首期前端应降级为仅支持 `stop`，`pause` 标记为”计划中”。
- Artifact 与普通消息附件的边界需要统一，否则会出现同一输出在消息区和工作台区重复展示。前端已有 `SessionArtifactView` 和 `.artifacts-panel`，需要明确新 Artifact 面板与现有实现的关系（替代还是增强）。
- 权限确认若支持”记住选择”，必须明确记忆范围，是仅当前会话、当前 Agent 还是全局。需要与已有的 `evolution.confirm` 记忆机制统一。
- 工作区恢复若跨应用重启保留未完成任务状态，需要界定哪些状态只是展示恢复，哪些能真正继续执行。
- **前端文件复杂度**：`chat-view.ts` 已有 666 行，`chat.css` 已有 1693 行。Agent Console 如果全部加入 chat-view 会导致单文件过大，需要拆分为独立模块。
- **Memory 命中追踪的后端缺口**：当前 memory 注入发生在 prompt 构建阶段，但注入结果（命中了哪些记忆、评分多少）并未被记录或返回。这是一个后端新增需求，不是简单的协议暴露。
- **后端接口就绪状态**：本文档提出的新增接口（`agent.inspect`、`session.runtime`、`session.tools.list`、`session.prompt.preview`、`session.memory.hits` 等）目前后端均未实现。前端调用时需统一降级处理：`ResourceState.error = '接口暂未支持'`，UI 显示"该功能需要后端升级"提示。具体的接口就绪状态矩阵将在 Plan 4 中详细定义。
