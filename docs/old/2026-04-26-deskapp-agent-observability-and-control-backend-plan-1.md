# Plan 1: 运行态快照与 Gateway 协议扩展

## 前置依赖

无

## 本次目标

建立 DeskApp Agent 工作台首批后端只读/轻写能力，使后端能够稳定回答以下问题：

- 当前会话最终生效的模型是什么，来源是全局、Agent 还是会话覆盖
- 当前 Agent 在本轮最终装配出的 prompt、tool、skill、memory 命中分别是什么
- 当前和最近一轮 token 使用量是多少
- 会话级模型覆盖如何设置、重置与查询

本 Plan 的落地产物是“运行态快照层”，而不是执行历史层。它解决的是当前态查询和增量刷新，不负责 run 历史、权限中心和恢复。

## 涉及文件

- `crates/nova-protocol/src/agent.rs`
- `crates/nova-protocol/src/chat.rs`
- `crates/nova-protocol/src/session.rs`
- `crates/nova-protocol/src/lib.rs`
- `crates/nova-gateway-core/src/router.rs`
- `crates/nova-gateway-core/src/handlers/mod.rs`
- `crates/nova-gateway-core/src/handlers/agents.rs`
- `crates/nova-gateway-core/src/handlers/sessions.rs`
- `crates/nova-gateway-core/src/bridge.rs`
- `crates/nova-app/src/application.rs`
- `crates/nova-app/src/types.rs`
- `crates/nova-app/src/conversation_service.rs`
- `crates/nova-core/src/agent.rs`
- `crates/nova-core/src/prompt.rs`
- `crates/nova-core/src/event.rs`
- `crates/nova-conversation/src/control.rs`
- `crates/nova-conversation/src/session.rs`
- `crates/nova-conversation/src/service.rs`

## 详细设计

### 1. 职责边界

Plan 1 只解决“当前态快照”的采集、存储、查询和协议化，不引入 run 历史持久化表。

包括：

- 会话级模型覆盖状态
- 最近一次 `prepare_turn()` 产出的 prompt/tool/skill 快照
- 最近一轮 usage 与会话累计 token
- memory hits 的最近一轮结果
- 面向前端的请求/响应与增量事件

不包括：

- run 列表和 step 时间线
- artifact 列表与文件系统联动
- 权限确认中心、审计日志、恢复视图

### 2. 新增后端聚合服务

建议在 `nova-app` 新增 `AgentWorkspaceService`，由 `application.rs` 统一持有，向 Gateway 暴露以下能力：

- `inspect_agent(agent_id, session_id) -> AgentInspectView`
- `get_session_runtime(session_id) -> SessionRuntimeSnapshot`
- `preview_session_prompt(session_id, message_id) -> PromptPreviewView`
- `list_session_tools(session_id) -> SessionToolsView`
- `list_session_skill_bindings(session_id) -> SessionSkillBindingsView`
- `get_session_memory_hits(session_id, turn_id) -> SessionMemoryHitsView`
- `override_session_model(session_id, request) -> SessionRuntimeSnapshot`
- `get_session_token_usage(session_id) -> SessionTokenUsageView`

理由：

- `ConversationService` 当前已经偏重执行链路，不适合继续承接只读聚合查询
- `Gateway` handler 应保持轻薄，只负责参数校验、错误翻译和消息发送
- 将聚合层放在 `nova-app`，可以同时复用 `SessionService`、`AgentRegistry`、`ConversationService` 与未来的 run tracker

### 3. Session 扩展状态设计

当前 `Session.control` 只有 `active_agent`。Plan 1 需要把会话级运行态扩展为显式结构，建议在 `nova-conversation/src/control.rs` 内引入：

```rust
pub struct SessionRuntimeControl {
    pub active_agent: String,
    pub model_override: SessionModelOverride,
    pub last_turn_snapshot: Option<LastTurnSnapshot>,
    pub token_counters: SessionTokenCounters,
}
```

子对象建议：

- `SessionModelOverride`
  - `orchestration: Option<ModelRef>`
  - `execution: Option<ModelRef>`
  - `updated_at: i64`
- `LastTurnSnapshot`
  - `turn_id: String`
  - `prepared_at: i64`
  - `prompt_preview: PromptPreviewSnapshot`
  - `tools: Vec<ToolAvailabilitySnapshot>`
  - `skills: Vec<SkillBindingSnapshot>`
  - `memory_hits: Option<Vec<MemoryHitSnapshot>>`
  - `usage: Option<Usage>`
- `SessionTokenCounters`
  - `input_tokens`
  - `output_tokens`
  - `cache_creation_input_tokens`
  - `cache_read_input_tokens`
  - `updated_at`

设计意图：

- 会话级覆盖和最近一轮快照都属于“当前运行态”，不应分散在 Gateway 缓存里
- `LastTurnSnapshot` 只保留最近一轮，避免首期引入大量持久化复杂度
- `SessionTokenCounters` 在 session 生命周期内累加，可作为 `session.runtime` 的真源

### 4. Prompt 快照来源

现有 `AgentRuntime::prepare_turn()` 已产出 `TurnContext`，Plan 1 不重复发明 prompt 组装流程，而是在该节点旁路记录快照。

建议新增一个结构化转换器：

- `TurnContextSnapshot::from_turn_context(ctx: &TurnContext, metadata: TurnSnapshotMetadata)`

输出字段：

- `system_prompt`
- `tool_sections`
- `skill_sections`
- `conversation_summary`
- `history_message_count`
- `active_skill`
- `capability_policy_summary`
- `max_tokens`
- `iteration_budget`

关键点：

- 不直接返回单个巨大 prompt 字符串给前端作为唯一视图
- 保留结构化分段，便于前端按 system/skills/tools/memory/history 展示
- 同时保留 `rendered_prompt` 可选字段，供复制和问题排查

### 5. Tool / Skill 快照设计

#### 5.1 Tool 快照

当前真实工具集合由 `TurnContext.tool_definitions` 决定，因此 `session.tools.list` 的真源应为该字段，而不是系统全量 tool registry。

建议 ViewModel：

```rust
pub struct ToolAvailabilityView {
    pub name: String,
    pub source: ToolSourceView,
    pub description: Option<String>,
    pub schema_summary: serde_json::Value,
    pub enabled: bool,
    pub unlocked_by: Option<String>,
}
```

来源分类：

- `builtin`
- `mcp_server`
- `mcp_client`
- `custom`
- `skill_unlocked`

首期允许 `source` 由工具命名空间与 registry 元数据推导；若信息不足，再在 `ToolDefinition` 元数据层补充。

#### 5.2 Skill 快照

当前 skill 运行态信息分散在：

- `TurnContext.active_skill`
- `AgentEvent::SkillActivated/SkillSwitched/SkillExited/SkillInvocation`
- skill registry 的静态安装信息

Plan 1 建议把技能绑定快照分为三类：

- `active`: 当前本轮实际激活技能
- `bound`: Agent 默认绑定技能
- `available`: 当前可路由技能，但未激活

这样前端能分清“已安装”和“当前有效”。

### 6. Memory Hit 设计

这是 Plan 1 最大的后端缺口。当前系统没有稳定记录“哪些记忆被命中并注入了 prompt”。

建议方案：

1. 在 memory 注入发生的那一层引入 `MemoryHitRecorder`
2. 记录最近一轮命中结果到 `LastTurnSnapshot.memory_hits`
3. `session.memory.hits` 只返回最近一次或指定 `turn_id` 的命中视图

建议字段：

- `memory_id`
- `title`
- `score`
- `reason`
- `excerpt`
- `source`
- `injected: bool`

如果当前 turn 没有接入 memory：

- 返回 `hits: []`
- `enabled: false`

如果当前后端版本尚未支持命中埋点：

- 返回结构化 `capability_not_supported`

不能把两者混为一谈。

### 7. Token 统计设计

#### 7.1 单轮 usage

现有 `TurnResult.usage` 已能聚合整轮 turn 的 usage，问题在于没有返回给 Gateway。Plan 1 要求：

1. `ConversationService::start_turn()` 或其内部执行链路返回 `TurnResult`
2. `nova-app` 将 `TurnResult.usage` 映射为 `AppEvent` 或直接回传给 Gateway handler
3. `handle_chat()` 最终发出带 `usage` 的 `ChatCompletePayload`

#### 7.2 会话累计 token

`SessionTokenCounters` 需要在 turn 完成时累加：

- `input_tokens += turn_usage.input_tokens`
- `output_tokens += turn_usage.output_tokens`
- `cache_creation_input_tokens += turn_usage.cache_creation_input_tokens`
- `cache_read_input_tokens += turn_usage.cache_read_input_tokens`

#### 7.3 增量事件

Plan 1 首期不要求逐 token 成本流式统计，但至少在 turn 完成时推送：

- `session.token.usage`
- `session.runtime.updated`

这样前端可避免每轮都主动回源。

### 8. 会话级模型覆盖设计

#### 8.1 覆盖模型

会话级覆盖需要独立于全局配置和 agent 默认配置。建议规则：

- `None` 表示继承
- `Some(model_ref)` 表示会话覆盖
- 允许只覆盖 orchestration 或 execution 其中之一

生效优先级：

1. session override
2. agent default
3. global default

#### 8.2 应用时机

每次 `prepare_turn()` 前，从 session runtime control 读取覆盖配置并生成本轮实际 `ModelBindingDetailView`。

不要在前端直接拼覆盖后的模型字符串；后端才是最终解释者。

#### 8.3 复制会话规则

`copy_session()` 默认复制：

- `active_agent`
- `model_override`

不复制：

- `last_turn_snapshot`
- `token_counters`

原因是复制后是新会话，不应继承旧会话累计 token 和旧 prompt 快照。

### 9. Gateway 协议设计

建议新增以下 request/response：

| 接口 | 请求 | 响应 |
|------|------|------|
| `agent.inspect` | `AgentInspectRequest` | `AgentInspectResponse` |
| `session.runtime` | `SessionRuntimeRequest` | `SessionRuntimeResponse` |
| `session.prompt.preview` | `SessionPromptPreviewRequest` | `SessionPromptPreviewResponse` |
| `session.tools.list` | `SessionToolsListRequest` | `SessionToolsListResponse` |
| `session.skill.bindings` | `SessionSkillBindingsRequest` | `SessionSkillBindingsResponse` |
| `session.memory.hits` | `SessionMemoryHitsRequest` | `SessionMemoryHitsResponse` |
| `session.model.override` | `SessionModelOverrideRequest` | `SessionRuntimeResponse` |
| `sessions.token_usage` | `SessionTokenUsageRequest` | `SessionTokenUsageResponse` |

统一要求：

- 全部包含 `session_id`
- 响应全部带 `updated_at`
- 对旧后端或未实现能力返回 `capability_not_supported`

### 10. 事件补齐

Plan 1 推荐补充或语义化封装以下事件：

- `session.runtime.updated`
- `session.token.usage`
- `session.tools.updated`
- `session.skill.bindings.updated`
- `session.memory.hit`

事件来源：

- `prepare_turn()` 完成后
- `ToolUnlocked` / skill 切换后
- turn 完成并写入 usage 后
- memory 命中结果写入后
- 模型覆盖修改后

### 11. 详细实施步骤

#### 阶段 A：协议与类型定义

1. 在 `nova-protocol` 中新增运行态快照相关 ViewModel 和请求/响应类型。
2. 为新增事件定义 payload，并在 `lib.rs` 导出。
3. 定义结构化错误码：
   - `capability_not_supported`
   - `invalid_request`
   - `session_not_found`
   - `service_error`

完成标准：

- 协议层不再依赖 `serde_json::Value` 承载整个快照主体
- 新增接口和事件具备稳定类型名

#### 阶段 B：Session 运行态扩展

1. 扩展 `ControlState` 或引入新 runtime control 结构，承载 model override、last turn snapshot、token counters。
2. 在 `Session` 上增加读写 runtime state 的方法。
3. 在 `SessionService::create()`、`load_all()`、`load_session_from_db()` 中初始化这些结构。
4. 明确 `copy_session()` 的复制与不复制字段。

完成标准：

- 新会话、冷加载会话、复制会话三条路径的 runtime state 一致
- 不需要前端额外缓存会话级模型覆盖

#### 阶段 C：Turn 快照采集

1. 在 `ConversationService::execute_agent_turn()` 中，为每轮生成 `turn_id`。
2. 在 `prepare_turn()` 之后立刻把 `TurnContext` 转成 `LastTurnSnapshot` 的 prompt/tool/skill 部分。
3. 在 memory 注入完成后写入命中结果。
4. 在 turn 完成后补齐 usage 并累加 session token counters。

完成标准：

- 最近一轮快照可以独立于聊天消息被查询
- `ChatCompletePayload.usage` 与 `session.runtime.token_usage` 语义一致

#### 阶段 D：应用层聚合服务

1. 在 `nova-app` 增加 `AgentWorkspaceService`。
2. 实现 inspect/runtime/prompt/tools/skills/memory/model override/token usage 八个方法。
3. 为未支持的 memory hits、pause 等能力统一返回结构化错误。

完成标准：

- Gateway handler 不再直接访问 session 内部结构拼 JSON
- 应用层聚合逻辑集中可测试

#### 阶段 E：Gateway handler 与事件桥接

1. 在 `router.rs` 注册新增请求路由。
2. 在 `handlers/agents.rs`、`handlers/sessions.rs` 实现新接口。
3. 更新 `bridge.rs`，加入运行态相关事件转换。
4. 修复 `handle_chat()`，让 `ChatCompletePayload` 返回真实 `usage`。

完成标准：

- 前端可以通过请求获取快照，也能通过事件收到增量更新
- 旧聊天主流程不受影响

#### 阶段 F：测试补齐

1. 为协议对象补序列化/反序列化测试。
2. 为 `AgentWorkspaceService` 补单元测试。
3. 为会话级模型覆盖、token 累计、prompt 快照、memory hits 补集成测试。
4. 为 `ChatCompletePayload.usage` 回填补回归测试。

完成标准：

- 运行态快照的关键字段都有自动化校验
- “未支持”和“无数据”路径可被明确区分

## 测试案例

- 模型覆盖：设置 session override 后，`session.runtime` 返回来源为 `session_override`；reset 后恢复到 agent/global 继承。
- token 统计：完成两轮对话后，`ChatCompletePayload.usage` 返回单轮值，`session.runtime.token_usage` 返回累计值。
- prompt 预览：调用 `session.prompt.preview` 时，能看到 system/tools/skills/history 分段，而非只有一段原始字符串。
- tools 快照：运行中触发 `ToolUnlocked` 后，`session.tools.list` 与 `session.tools.updated` 能反映工具状态变化。
- memory hits：未接入命中埋点时返回 `capability_not_supported`；接入后能返回命中列表和分数。
- skill bindings：有 active skill 时，`session.skill.bindings` 能区分 `active`、`bound`、`available`。
- 复制会话：`copy_session()` 后新会话继承模型覆盖，但不会继承旧会话 token 累计与 last turn snapshot。
- 兼容性：旧客户端只使用 `chat` 基础接口时，新增快照层不会改变其消息顺序和停止行为。
