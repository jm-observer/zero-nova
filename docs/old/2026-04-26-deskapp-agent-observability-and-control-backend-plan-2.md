# Plan 2: 运行记录、权限诊断与工作区恢复

## 前置依赖

- Plan 1: 运行态快照与 Gateway 协议扩展

## 本次目标

在 Plan 1 的“当前态快照”基础上，补齐可回溯的运行记录模型，使后端能够支持：

- 当前运行控制与状态查询
- 历史 run / step 查询
- artifact 关联与检索
- 通用权限确认与审计
- 诊断问题聚合
- 应用重启后的工作台恢复

本 Plan 的核心是把当前零散事件流提升为统一运行治理模型，而不是单纯增加几个查询接口。

## 涉及文件

- `crates/nova-protocol/src/chat.rs`
- `crates/nova-protocol/src/session.rs`
- `crates/nova-protocol/src/system.rs`
- `crates/nova-protocol/src/lib.rs`
- `crates/nova-gateway-core/src/router.rs`
- `crates/nova-gateway-core/src/handlers/chat.rs`
- `crates/nova-gateway-core/src/handlers/sessions.rs`
- `crates/nova-gateway-core/src/handlers/system.rs`
- `crates/nova-gateway-core/src/bridge.rs`
- `crates/nova-app/src/application.rs`
- `crates/nova-app/src/types.rs`
- `crates/nova-app/src/conversation_service.rs`
- `crates/nova-app/src/lib.rs`
- `crates/nova-conversation/src/repository.rs`
- `crates/nova-conversation/src/sqlite_manager.rs`
- `crates/nova-conversation/src/service.rs`
- `crates/nova-conversation/src/session.rs`
- `crates/nova-core/src/event.rs`
- `crates/nova-core/src/tool/builtin/task.rs`
- `crates/nova-core/src/agent.rs`

## 详细设计

### 1. 领域模型

Plan 2 统一引入以下记录模型。

#### 1.1 RunRecord

代表一次实际执行。

建议字段：

- `run_id`
- `session_id`
- `turn_id`
- `agent_id`
- `status`
- `started_at`
- `finished_at`
- `duration_ms`
- `orchestration_model`
- `execution_model`
- `usage`
- `error_summary`
- `waiting_reason`

#### 1.2 RunStepRecord

代表 run 内的语义化步骤，而非逐条裸事件。

建议类型：

- `thinking`
- `tool`
- `approval`
- `message`
- `artifact`
- `system`

关键字段：

- `step_id`
- `run_id`
- `step_type`
- `title`
- `status`
- `tool_name`
- `started_at`
- `finished_at`
- `payload_json`

#### 1.3 ArtifactRecord

只记录可检索元数据，不强制存全部大文本内容。

建议字段：

- `artifact_id`
- `session_id`
- `run_id`
- `step_id`
- `artifact_type`
- `path`
- `filename`
- `content_preview`
- `language`
- `size`
- `created_at`

#### 1.4 PermissionRequestRecord

统一 evolution 安装确认与未来高风险操作确认。

建议字段：

- `request_id`
- `session_id`
- `run_id`
- `step_id`
- `agent_id`
- `kind`
- `title`
- `reason`
- `target`
- `risk_level`
- `status`
- `remember_scope`
- `created_at`
- `resolved_at`

#### 1.5 AuditLogRecord

覆盖 permission、run control、artifact open、workspace restore 等动作。

#### 1.6 DiagnosticIssueRecord

用于聚合用户可见问题，而不是直接暴露底层错误字符串。

#### 1.7 WorkspaceRestoreRecord

用于恢复最近工作区视角：

- 最近 session
- 最近选中 run
- 最近打开 tab
- 最近选中 artifact / permission / diagnostic
- 最近未完成 run 的恢复方式

### 2. 持久化策略

#### 2.1 为什么要落库

如果 run 记录只在内存里：

- 应用重启后无法恢复工作台
- 历史执行与审计不可回看
- 失败诊断没有稳定追溯对象

因此 Plan 2 建议为 SQLite 新增最小必要表：

- `runs`
- `run_steps`
- `artifacts`
- `permission_requests`
- `audit_logs`
- `diagnostic_issues`
- `workspace_restore_state`

#### 2.2 什么不落库

首期不落库：

- 每个 token 的流式原文
- 全量 thinking 长文本明细
- 每次 tool stdout/stderr 的全部日志片段

这些内容可以：

- 保留到消息历史或 artifact 预览中
- 只在 step payload 中存摘要
- 需要时后续再扩展归档机制

### 3. Run 生命周期设计

#### 3.1 创建时机

在 `ConversationService::execute_agent_turn()` 进入实际执行前创建 `RunRecord`：

1. 生成 `turn_id`
2. 生成 `run_id`
3. 写入 `status = queued`
4. 获取 chat lock 后转为 `running`

这样即使后续在 `prepare_turn()` 前失败，也能留下失败 run 记录。

#### 3.2 状态流转

后端支持的标准状态：

- `queued`
- `running`
- `waiting_user`
- `paused`
- `stopped`
- `failed`
- `completed`

首期实现约束：

- 运行中可进入 `waiting_user`
- `pause/resume` 仅保留协议和记录能力，默认返回 `capability_not_supported`
- `stop` 走现有 cancellation token 机制

#### 3.3 结束规则

结束时必须统一写入：

- `finished_at`
- `duration_ms`
- `usage`
- `error_summary` 或完成摘要
- 最终状态

不要把这些字段散落在多个 handler 补写，否则容易出现 run 结束但 usage 未入库的问题。

### 4. Step 归并规则

现有 `AgentEvent` 粒度较细，Plan 2 需要把它归并成稳定 step。

归并映射建议：

- `Iteration` + `ThinkingDelta` -> `thinking`
- `ToolStart` + `ToolEnd` + `LogDelta` -> `tool`
- 通用权限请求 -> `approval`
- `AssistantMessage` / `TurnComplete` -> `message`
- artifact 生成通知 -> `artifact`
- `IterationLimitReached` / `Error` / `stop` -> `system`

关键规则：

1. 同一工具调用按 `tool_use_id` 归并为一个 step。
2. `ToolStart` 到 `ToolEnd` 之间的 stdout/stderr 只追加到该 step 摘要或 payload。
3. `waiting_user` 状态下必须有一个 `approval` 或 `system` step 对应原因。
4. 一个 run 至少有一条 step，哪怕是启动即失败。

### 5. Artifact 链路设计

当前代码库已有 `SessionArtifactView` 概念，但后端未形成统一 artifact 真源。Plan 2 需要把 artifact 视为 run 子资源。

建议接入点：

- 工具返回结果中显式生成文件、代码、输出时记录 artifact
- 宿主层保存文件时产生 artifact 元数据
- 后续若前端从消息中抽取 artifact，也应能回写或映射到 `run_id` / `step_id`

接口目标：

- `session.artifacts { session_id, run_id?, type? }`

要求：

- 同一 artifact 需要能定位到来源 run 和 step
- 文件缺失时返回正常记录，同时由 diagnostics 标记问题，不要直接把 artifact 记录删除

### 6. 通用权限协调器

#### 6.1 统一入口

当前 evolution confirm 已有确认机制，但只覆盖技能安装。Plan 2 应抽象成统一 `PermissionCoordinator`。

统一承接来源：

- 本地命令执行
- 文件写入/覆盖
- 外部网络访问
- MCP 工具调用
- evolution 安装确认

#### 6.2 与 run 的联动

当权限请求创建时：

1. 新增 `PermissionRequestRecord`
2. 新增 `approval` step
3. 将 run 状态切到 `waiting_user`
4. 推送 `permission.requested` 和 `run.status.updated`

当权限被处理时：

1. 更新 request 状态
2. 写入 audit log
3. 若批准且可继续，run 回到 `running`
4. 若拒绝或超时，run 转为 `failed` 或 `stopped`

### 7. 诊断服务设计

#### 7.1 诊断来源

诊断服务需要消费以下来源：

- `AgentEvent::Error`
- provider 调用失败
- permission denied / expired
- artifact 文件缺失
- Gateway 能力不支持
- run 长时间卡在 `waiting_user`
- memory hits / prompt preview 构建失败

#### 7.2 生成规则

不是所有底层错误都直接生成诊断。建议规则：

- 用户可采取行动的错误 -> 生成诊断
- 重复噪音错误 -> 只更新已有诊断的 `updated_at` 和计数
- 纯内部调试日志 -> 不进入诊断列表

建议诊断类别：

- `llm`
- `mcp`
- `memory`
- `permission`
- `protocol`
- `artifact`
- `runtime`
- `unknown`

### 8. 工作区恢复设计

#### 8.1 恢复目标

`workspace.restore` 不负责恢复执行本身，而是恢复“用户上次看到哪里”以及“哪些运行仍可观察”。

建议返回：

- `session_id`
- `agent_id`
- `console_visible`
- `active_tab`
- `selected_run_id`
- `selected_artifact_id`
- `selected_permission_request_id`
- `selected_diagnostic_id`
- `restorable_run_state`
- `updated_at`

#### 8.2 恢复状态分类

- `none`
- `view_only`
- `reattachable`

含义：

- `none`: 没有可恢复运行
- `view_only`: 有历史记录，但运行已结束或不可继续
- `reattachable`: 后端仍有活动 run，可重新订阅状态

#### 8.3 写入时机

以下动作更新 `workspace_restore_state`：

- 切换会话
- 切换工作台 tab
- 选中 run / artifact / permission / diagnostic
- 当前运行状态发生重大变化

### 9. 应用层服务拆分

Plan 2 建议在 `nova-app` 继续扩展或新增以下服务：

- `RunTrackerService`
- `PermissionCoordinator`
- `DiagnosticsService`
- `WorkspaceRestoreService`

服务关系：

- `ConversationService` 调用 `RunTrackerService` 创建与收尾 run
- `PermissionCoordinator` 与 tool/runtime 协作，管理等待态
- `DiagnosticsService` 消费运行事件和错误，生成诊断对象
- `WorkspaceRestoreService` 从 session + run + UI 恢复元数据聚合输出

### 10. Gateway 协议设计

建议新增或扩展：

| 接口 | 用途 |
|------|------|
| `session.runs` | 查询会话执行历史 |
| `run.detail` | 查询单次执行详情 |
| `run.control` | `stop` / `resume_waiting` / 预留 `pause` `resume` `retry` |
| `session.artifacts` | 查询会话或 run 下 artifact |
| `permission.pending` | 查询待确认请求 |
| `permission.respond` | 处理权限请求 |
| `audit.logs` | 查询审计记录 |
| `diagnostics.current` | 查询当前会话诊断 |
| `workspace.restore` | 查询工作区恢复快照 |

新增事件：

- `run.status.updated`
- `run.step.updated`
- `session.artifacts.updated`
- `permission.requested`
- `permission.resolved`
- `audit.logs.updated`
- `diagnostics.updated`
- `workspace.restore.available`

### 11. 详细实施步骤

#### 阶段 A：协议建模

1. 在 `nova-protocol` 中定义 run、step、artifact、permission、audit、diagnostic、restore 相关 ViewModel。
2. 定义对应 request/response。
3. 为事件 payload 建立稳定类型。
4. 约定 `pause/resume/retry` 未实现时统一返回 `capability_not_supported`。

完成标准：

- 所有运行治理能力都有稳定协议名称
- 不再依赖前端从 `ProgressEvent` 自己拼 run 历史

#### 阶段 B：SQLite 仓储扩展

1. 为 `nova-conversation` 新增 run 相关表与 repository 方法。
2. 增加插入/更新：
   - run
   - run_step
   - artifact
   - permission_request
   - audit_log
   - diagnostic_issue
   - workspace_restore_state
3. 设计必要索引：
   - `runs(session_id, started_at desc)`
   - `run_steps(run_id, started_at)`
   - `artifacts(session_id, run_id)`
   - `permission_requests(session_id, status)`

完成标准：

- 重启后能恢复运行历史和待处理权限请求
- 查询路径不会退化成全表扫描

#### 阶段 C：运行跟踪接入

1. 在 `ConversationService::execute_agent_turn()` 的入口创建 run。
2. 在事件转发链路中，把 `AgentEvent` 同步送入 `RunTrackerService`。
3. 在成功、取消、失败三条收尾路径统一结束 run。
4. 在 `stop_turn()` 时同步写入 run control 审计记录。

完成标准：

- 成功/失败/停止路径都能留下完整 run 记录
- `ChatComplete` 与 run 结束状态一致

#### 阶段 D：权限与诊断接入

1. 抽象 evolution confirm 到通用权限请求模型。
2. 在高风险工具执行链路中接入 `PermissionCoordinator`。
3. 创建诊断服务，消费错误和等待超时等事件。
4. 将权限拒绝、artifact 缺失、协议不支持等转成可查询诊断。

完成标准：

- 任一待确认请求都能回溯到 session/run/step
- 用户可见问题可以通过 diagnostics 接口集中查询

#### 阶段 E：恢复能力接入

1. 在应用层提供 `workspace.restore` 聚合接口。
2. 为会话切换、tab 切换、run 选中等动作预留恢复状态写入入口。
3. 在应用启动阶段加载最近恢复记录和活动 run 状态。
4. 区分 `view_only` 和 `reattachable`。

完成标准：

- 应用重启后能恢复最近工作台上下文
- 不会伪造已结束 run 为 running

#### 阶段 F：Gateway handler 与测试

1. 实现新增 handler 和路由注册。
2. 在 `bridge.rs` 中增加运行治理事件映射。
3. 补单元、集成、仓储测试。
4. 补 run/permission/diagnostic/restore 的兼容性测试。

完成标准：

- 运行治理接口可独立调用
- 旧客户端仍可只走基础 chat 流程

## 测试案例

- run 创建与完成：一次正常对话后，`session.runs` 返回 `completed` run，`run.detail` 有 step、usage 和模型摘要。
- stop 控制：运行中调用 `run.control(stop)` 后，run 状态转为 `stopped`，同时生成审计记录。
- tool step 归并：同一 `tool_use_id` 的 start/log/result 被归并为单个 `tool` step。
- permission pending：触发高风险工具后，run 进入 `waiting_user`，`permission.pending` 返回待处理请求。
- permission deny：拒绝权限请求后，run 转为 `failed` 或 `stopped`，`audit.logs` 与 `diagnostics.current` 都能查询到对应记录。
- artifact 缺失：历史 artifact 指向的文件不存在时，`session.artifacts` 仍返回记录，`diagnostics.current` 增加 `artifact` 类问题。
- workspace restore：应用重启后 `workspace.restore` 返回最近 session、tab 和 selected run；若 run 已结束，则 `restorable_run_state = view_only`。
- 兼容路径：后端未实现 `pause/resume/retry` 时，`run.control` 对这些动作返回 `capability_not_supported`，不会影响 `stop`。
