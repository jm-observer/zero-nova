# DeskApp Agent 可观测性与控制后端详细设计

**时间**: 2026-04-26（创建）/ 2026-04-26（最后更新）

## 项目现状

当前与 DeskApp Agent 工作台后端能力直接相关的基础已经存在，但分散在多个 crate 中，尚未形成可直接对外暴露的运行态聚合层：

- `crates/nova-core`
  - `AgentRuntime::prepare_turn()` 已能生成 `TurnContext { system_prompt, tool_definitions, history, active_skill, capability_policy, skill_tool_enabled, max_tokens, iteration_budget }`
  - `run_turn()` / `run_turn_with_context()` 已能产出流式 `AgentEvent` 与聚合 `TurnResult { messages, usage }`
  - 现有 `AgentEvent` 已覆盖 `thinking`、`tool start/end`、`iteration`、`task`、`skill`、`tool unlocked` 等运行信号
- `crates/nova-app`
  - `ConversationService::execute_agent_turn()` 已串起 session、agent runtime、取消控制与消息持久化
  - 但当前 turn 执行完成后仅把 assistant/user tool result 写回会话，不保留专门的 run 级观测快照
- `crates/nova-conversation`
  - `SessionService` 已有 SQLite 持久化和内存缓存
  - `Session` 当前只持有 `active_agent`、`history`、`cancellation_token` 等最小控制态，没有会话级模型覆盖、最近运行摘要、权限等待态等扩展状态
- `crates/nova-gateway-core`
  - 当前只暴露 `chat`、`sessions`、`agents`、`config` 等基础 handler
  - `handle_chat()` 最后发出的 `ChatCompletePayload` 仍是 `output: None, usage: None`，没有把 `TurnResult.usage` 传给协议层
  - 事件桥接 `app_event_to_gateway()` 主要把 `AppEvent` 映射为 `ChatProgress`，缺少语义化的运行态快照事件
- `crates/nova-protocol`
  - 已有 `ProgressEvent`、`ChatCompletePayload`、若干 skill/tool 事件 payload
  - 但没有针对 `agent.inspect`、`session.runtime`、`session.prompt.preview`、`session.runs`、`permission.pending` 等能力的请求/响应类型

这意味着前端已经可以“看到部分过程事件”，但还无法稳定获得：

- 当前会话最终生效的模型绑定
- 当前 turn 的 prompt 预览与工具快照
- 记忆命中结果
- 可回放的 run / step / artifact / permission / diagnostic 结构化视图
- 应用重启后的工作台恢复快照

## 整体目标

为 DeskApp Agent 工作台补一层后端运行治理能力，目标不是重写现有聊天链路，而是在现有运行时之上增加可观测、可查询、可恢复、可控制的数据面。最终后端需要满足以下能力：

- 在不破坏现有 `chat.start -> progress -> chat.complete` 主链路的前提下，补齐结构化观测视图
- 把 `TurnContext`、`TurnResult`、`AgentEvent` 转成稳定协议对象，而不是让前端猜测内部结构
- 支持会话级模型覆盖、prompt 预览、工具快照、技能绑定、记忆命中、token 统计
- 建立 `run -> step -> artifact -> permission -> diagnostic` 的统一运行记录模型
- 支持当前运行控制、执行历史查询、权限确认和工作区恢复
- 对旧协议客户端保持兼容，对暂未实现能力返回结构化 `capability_not_supported`

## 设计范围

本次详细设计只覆盖后端职责：

- `nova-core`: turn 准备、事件埋点、运行态快照原始数据
- `nova-app`: 应用层聚合服务、运行记录编排、控制与恢复
- `nova-conversation`: session 扩展状态、run 持久化、恢复所需仓储
- `nova-gateway-core`: request handler、事件桥接、能力协商
- `nova-protocol`: request/response/event payload 建模

本次不展开前端布局、组件拆分或样式设计，只在需要时说明前端消费约束。

## 核心设计原则

1. 先聚合后暴露。优先在后端形成稳定 ViewModel，再暴露给 Gateway。
2. 快照与事件并存。请求接口返回快照真源，事件只做增量刷新。
3. 与现有 session/chat 兼容。新增能力不得要求重写现有聊天持久化模型。
4. 区分“无数据”“未支持”“运行失败”。协议层必须显式表达三种状态。
5. 控制面与审计面闭环。凡是 stop、permission、artifact 打开、恢复动作，都必须可追溯。
6. 首期只做必要持久化。优先持久化 run 摘要、step、恢复元数据；不把所有流式 token 原文落库。

## 后端目标架构

建议新增一个运行治理分层，位置位于 `ConversationService` 与 Gateway handler 之间：

```text
Gateway Handler
    -> AgentWorkspaceService
        -> RuntimeSnapshotAssembler
        -> RunTracker
        -> PermissionCoordinator
        -> DiagnosticsService
        -> WorkspaceRestoreService
    -> ConversationService / SessionService / AgentRuntime
```

职责边界：

- `ConversationService`
  - 继续负责一轮对话的主业务执行
  - 不直接承担复杂查询接口拼装
- `AgentWorkspaceService`
  - 提供 `inspect`、`runtime`、`prompt preview`、`runs`、`permissions`、`diagnostics` 等查询和控制入口
- `RuntimeSnapshotAssembler`
  - 从 `TurnContext`、session、agent 配置组装前端可消费的快照
- `RunTracker`
  - 在 turn 生命周期内记录运行摘要、步骤、token、artifact、错误、等待态
- `PermissionCoordinator`
  - 统一承接待确认请求与决策记录
- `WorkspaceRestoreService`
  - 聚合最近工作区状态，提供重启恢复所需信息

## 数据模型总览

后端需要引入三类数据对象。

### 1. 会话运行态快照

只保留当前态或最近一轮态，服务于 `agent.inspect`、`session.runtime`、`session.prompt.preview`、`session.tools.list`、`session.memory.hits`。

建议对象：

- `SessionRuntimeSnapshot`
- `AgentInspectView`
- `PromptPreviewView`
- `ToolAvailabilityView`
- `SkillBindingView`
- `MemoryHitView`
- `TokenUsageView`
- `ModelBindingDetailView`

### 2. 运行记录模型

用于执行历史、当前运行状态、权限联动和诊断回溯。

建议对象：

- `RunRecord`
- `RunStepRecord`
- `RunArtifactRecord`
- `PermissionRequestRecord`
- `AuditLogRecord`
- `DiagnosticIssueRecord`

### 3. 恢复模型

用于应用重启后的工作台恢复。

- `WorkspaceRestoreRecord`
- `RestorableRunState`

## Plan 拆分

### Plan 1: 运行态快照与 Gateway 协议扩展

- 目标：补齐 `agent.inspect`、`session.runtime`、`session.prompt.preview`、`session.tools.list`、`session.skill.bindings`、`session.memory.hits`、`session.model.override`、`sessions.token_usage` 的后端能力
- 依赖：无
- 顺序：第一步

### Plan 2: 运行记录、权限诊断与工作区恢复

- 目标：建立 `run -> step -> artifact -> permission -> diagnostic -> restore` 的统一后端模型，补齐执行历史、控制、审计和恢复接口
- 依赖：Plan 1
- 顺序：第二步

## 实施顺序总览

1. 先补协议模型与应用层聚合入口，避免前后端并行时接口命名继续漂移。
2. 再补 session 扩展状态和 run 跟踪，把单轮 turn 执行转成可查询的运行记录。
3. 然后补 Gateway handler 与事件推送，把聚合结果暴露给前端。
4. 最后补持久化、恢复、审计和测试矩阵，确保重启后仍能回看工作台状态。

## 风险与待定项

- 当前 `ConversationService::execute_agent_turn()` 未返回 `TurnResult` 给 Gateway，因此 `usage`、最终 step 结果和 run 摘要无法自然透出，需要调整应用层返回值。
- `Session` 当前只持有最小控制状态。若把所有运行态都塞进 `Session.control`，复杂度会迅速上升，建议拆分成独立 runtime state 对象。
- memory 命中当前没有任何稳定埋点。要实现 `session.memory.hits`，需要在 prompt 构建或 memory 注入阶段显式记录命中结果。
- `pause/resume` 当前没有 runtime 级安全中断点，首期接口应预留但默认返回 `capability_not_supported`。
- 权限确认现在主要存在于 evolution 安装链路。若要扩展为通用权限中心，需要抽象出统一请求 ID、来源、风险等级和 remember scope。
- SQLite 仓储目前只有 session/message 维度。若要持久化 run/step/audit，需要新增表，但应避免把高频流式 token 逐条落库。
