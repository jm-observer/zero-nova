# Plan 5: 运行工作流、权限诊断与工作区恢复

## 前置依赖

- Plan 1: 运行态控制台与信息架构
- Plan 4: Gateway 协议补齐与测试方案

## 本次目标

将原 Plan 5 与 Plan 6 合并，围绕 `run` / `turn` 工作流补齐四类能力：

- 当前运行控制与执行历史
- Artifact 聚合与产物检索
- 权限确认、审计与错误诊断
- 应用重启后的工作区上下文恢复

目标是把“执行中发生了什么、为什么卡住、产物在哪里、是否需要用户确认、出错后如何恢复、重启后如何继续观察”整合到同一组工作台能力里，而不是拆散在聊天区、设置页和零散弹窗中。

## 涉及文件

- `deskapp/src/gateway-client.ts`
- `deskapp/src/core/types.ts`
- `deskapp/src/core/state.ts`
- `deskapp/src/core/event-bus.ts`
- `deskapp/src/ui/chat-view.ts`
- `deskapp/src/ui/agent-console-view.ts`
- `deskapp/src/ui/settings-view.ts`
- `deskapp/src/ui/modals.ts`
- `deskapp/src/ui/templates/*`
- `deskapp/src/styles/main/chat.css`
- `deskapp/src/styles/main/agent-console.css`
- `deskapp/src-tauri/src/*`
- `deskapp/e2e/tests/*`
- `crates/nova-protocol/*`
- `crates/nova-gateway-core/*`
- `crates/nova-core/*`

## 详细设计

### 1. 为什么合并 Plan 5 和 Plan 6

原 Plan 5 处理 run、历史和 artifact，原 Plan 6 处理权限、诊断和恢复。两者在实现上高度耦合：

- 权限确认本质上是 run 的中断状态之一，而不是独立于 run 的全新流程。
- 诊断信息需要引用 run、step、artifact 和 permission request，单独设计容易重复建模。
- 工作区恢复也必须恢复“当前聚焦的 run / artifact / pending permission / diagnostics tab”，否则恢复体验是碎片化的。
- 若分成两个 Plan，前端状态层会先做一套 run 缓存，再补一套 permission/diagnostics/restore 缓存，容易出现模型重复和交互边界不清。

因此本 Plan 改为一个完整的“运行治理”设计：

- `run` 是主轴
- `artifact`、`permission`、`diagnostic` 是挂接在 run / session 上的子视图
- `workspace restore` 是这些视图状态的恢复层

### 2. 领域模型与关系

本 Plan 的核心关系如下：

```ts
interface RunSummaryView {
    id: string;
    sessionId: string;
    turnId?: string;
    agentId?: string;
    status: 'queued' | 'running' | 'waiting_user' | 'paused' | 'stopped' | 'failed' | 'completed';
    title?: string;
    startedAt: number;
    finishedAt?: number;
    durationMs?: number;
    modelSummary?: string;
    toolCount?: number;
    tokenUsage?: TokenUsageView;
    errorSummary?: string;
    waitingReason?: 'permission' | 'user_input' | 'external_callback';
}

interface RunStepView {
    id: string;
    runId: string;
    type: 'thinking' | 'tool' | 'approval' | 'message' | 'artifact' | 'system';
    title: string;
    status: 'running' | 'completed' | 'failed' | 'skipped';
    startedAt?: number;
    finishedAt?: number;
    toolName?: string;
    description?: string;
    artifactIds?: string[];
    permissionRequestId?: string;
}

interface PermissionRequestView {
    id: string;
    sessionId?: string;
    runId?: string;
    stepId?: string;
    agentId?: string;
    kind: 'command' | 'file_write' | 'network' | 'mcp_tool';
    title: string;
    reason?: string;
    target?: string;
    createdAt: number;
    riskLevel: 'low' | 'medium' | 'high';
    status: 'pending' | 'approved' | 'denied' | 'expired';
    rememberScope?: 'session' | 'agent' | 'global';
}

interface AuditLogView {
    id: string;
    sessionId?: string;
    runId?: string;
    permissionRequestId?: string;
    actionType: 'permission' | 'run_control' | 'artifact_open' | 'workspace_restore';
    actor: 'user' | 'system' | 'agent';
    result: 'approved' | 'denied' | 'failed' | 'completed';
    summary: string;
    createdAt: number;
}

interface DiagnosticIssueView {
    id: string;
    category: 'llm' | 'mcp' | 'memory' | 'permission' | 'protocol' | 'artifact' | 'runtime' | 'unknown';
    severity: 'info' | 'warn' | 'error';
    title: string;
    message: string;
    suggestedActions: string[];
    relatedRunId?: string;
    relatedStepId?: string;
    relatedSessionId?: string;
    updatedAt: number;
    retryable?: boolean;
}

interface WorkspaceRestoreView {
    sessionId?: string;
    agentId?: string;
    consoleVisible: boolean;
    activeTab?: 'overview' | 'model' | 'tools' | 'skills' | 'prompt-memory' | 'runs' | 'permissions' | 'diagnostics';
    selectedRunId?: string;
    selectedArtifactId?: string;
    selectedPermissionRequestId?: string;
    selectedDiagnosticId?: string;
    restorableRunState?: 'none' | 'view_only' | 'reattachable';
    updatedAt: number;
}
```

关系约束：

- 一个 `RunSummaryView` 可关联多个 `RunStepView`
- 一个 `RunStepView` 可关联多个 `SessionArtifactView`
- 一个 `PermissionRequestView` 必须能追溯到 `sessionId`，优先再追溯到 `runId` / `stepId`
- 一个 `DiagnosticIssueView` 可以关联 run、step、permission 或 artifact，但不强制全部都有
- `WorkspaceRestoreView` 只恢复“用户上次在看什么”，不伪造运行继续结果

### 3. Run / Turn 工作流设计

#### 3.1 run 与 turn 的职责

- `turn` 代表一轮对话请求的业务边界
- `run` 代表该轮对话在 Agent runtime 中的一次实际执行记录

首期 UI 可以把两者合并展示为 “一次运行”，但协议层建议保留：

- `turnId`：对齐聊天消息和 Prompt/Memory 视图
- `runId`：对齐步骤、artifact、权限确认、诊断、重试

这样后续若支持“同一 turn 的重跑”或“基于原输入再次执行”，无需重构 ID 体系。

#### 3.2 状态机

建议状态流转如下：

- `queued -> running`
- `running -> waiting_user`
- `running -> paused`
- `running -> stopped`
- `running -> failed`
- `running -> completed`
- `waiting_user -> running`
- `waiting_user -> stopped`
- `paused -> running`
- `paused -> stopped`

约束说明：

- 当前后端尚无稳定 `pause/resume` 机制，首期只正式开放 `stop`
- `waiting_user` 是一等状态，不把它折叠成普通 `running`
- `waiting_user` 必须带 `waitingReason`
- 当等待原因是权限确认时，当前 run、当前 permission request、当前 diagnostic 必须能互相跳转

#### 3.3 step 视图设计

`RunStepView` 不是简单事件日志，而是对运行过程的语义化整理：

- `thinking`：模型推理中
- `tool`：工具执行中或已完成
- `approval`：等待权限确认
- `message`：结果整理或消息输出
- `artifact`：产物生成
- `system`：停止、恢复、诊断、回源等系统动作

step 生成规则：

- `ProgressEvent.thinking` 归并为 `thinking`
- `tool_start` / `tool_result` 归并为 `tool`
- `permission.requested` 归并为 `approval`
- `turn_complete` 归并为 `message`
- `session.artifacts.updated` 可补出 `artifact`
- `run.control` 成功/失败可补出 `system`

同一工具多次调用时：

- 每次调用都应有独立 step，不能只保留最后一次
- 但 run 列表中的 `toolCount` 仍可只展示总次数

### 4. 运行控制设计

#### 4.1 控制动作

首期支持动作：

- `stop`
- `resume_waiting`（用于等待权限确认/用户输入后的继续）

保留协议但默认不开放的动作：

- `pause`
- `resume`
- `retry`

建议协议：

```ts
interface RunControlRequest {
    runId: string;
    action: 'stop' | 'resume_waiting' | 'pause' | 'resume' | 'retry';
}
```

动作约束：

- `stop` 可复用现有 `stopTask(sessionId)` 逻辑，但对前端暴露时统一走 `run.control`
- `resume_waiting` 只对 `waiting_user` 状态可用
- `pause/resume` 若后端未支持，必须返回 `capability_not_supported`
- `retry` 不直接复用“再次发送消息”，而应由后端明确决定是否复用原输入、原模型覆盖与原上下文

#### 4.2 当前运行卡片

工作台 `运行` 标签首屏展示当前运行卡片，包含：

- 当前状态
- 运行标题或最后一条用户输入摘要
- 开始时间 / 已运行时长
- 当前阶段
- 当前使用模型摘要
- 当前已产生 artifact 数
- 当前待确认数
- 快捷动作按钮

若当前无运行中任务：

- 展示最近一次 run 的结果摘要
- 提供“查看最近执行详情”入口

### 5. 执行历史与详情设计

#### 5.1 历史列表

工作台历史列表展示最近 runs，最少包含：

- 状态徽章
- 标题
- 开始时间
- 耗时
- 模型摘要
- 工具次数
- token 摘要
- 错误摘要

交互要求：

- 默认按时间倒序
- 支持按状态筛选
- 支持按“仅失败”“仅等待确认”“仅有产物”快速筛选

#### 5.2 run 详情区

选中某条 run 后，右侧详情区分为四段：

1. 基本信息
2. 步骤时间线
3. 关联产物
4. 关联异常 / 权限 / 审计

这样用户无需切换多个标签，就能在一次执行的上下文内完成定位。

#### 5.3 基于当前输入重跑

“重跑”入口建议放在 run 详情区，不放在普通消息气泡中。原因：

- 重跑需要明确作用域，是复用原输入还是复用原上下文
- 重跑可能涉及会话级模型覆盖、权限策略、当前 skills/tools 快照
- 应让用户在看清上次错误与产物后再决定是否重试

首期只定义入口与协议预留，不强制在本 Plan 实现完整重跑能力。

### 6. Artifact 工作流设计

#### 6.1 与现有 artifact 面板的关系

现有 `.artifacts-panel` 和 `SessionArtifactView` 继续复用，但职责调整为：

- 聊天区：展示“刚产生的产物”
- 工作台：展示“可检索、可过滤、可回溯来源的产物总表”

不是新增第二套 artifact 模型，而是增强现有模型的上下文能力。

#### 6.2 artifact 分区

建议提供两个视图层级：

- 会话级 artifact 列表
- run 级 artifact 子集

筛选维度：

- `全部`
- `文件`
- `代码`
- `输出`
- `图片`

每个 artifact 至少展示：

- 名称 / 文件名
- 类型
- 生成时间
- 来源 run
- 来源 step
- 文件路径或内容预览

#### 6.3 artifact 动作

支持动作：

- 打开预览
- 定位到来源 run
- 复制路径
- 在文件系统中打开
- 复制内容（针对 code/output）

约束：

- “在文件系统中打开”属于宿主层动作，应记录审计日志
- 文件不存在时，不静默失败，需生成 `artifact` 类诊断

### 7. 权限确认与审计设计

#### 7.1 确认中心

工作台新增 `权限` 标签，分三块：

- 待确认请求
- 最近处理记录
- 审计日志

高风险请求可继续触发即时浮层，但浮层只是快捷入口，不是唯一入口。

#### 7.2 权限请求与 run 的关系

权限请求进入待确认队列时：

- run 状态切到 `waiting_user`
- 生成一个 `approval` step
- 若请求超时或被拒绝，run 应进入 `failed` 或 `stopped`，由后端统一决定

这条链路必须是可回溯的，不能只在 UI 上看见一个孤立弹窗。

#### 7.3 remember 语义

权限确认若支持“记住本次选择”，建议范围只允许：

- `session`
- `agent`
- `global`

并明确：

- 默认不勾选
- 高风险动作不允许直接默认 `global`
- 范围解释文案必须清楚显示给用户

#### 7.4 审计日志

审计记录不仅记录 permission，也应覆盖：

- stop / retry / resume_waiting 等运行控制
- artifact 打开失败或宿主打开动作
- workspace restore 触发的自动恢复行为

这样诊断面板可以引用审计信息，而无需再重复建一套“操作历史”。

### 8. 诊断与恢复设计

#### 8.1 诊断来源

诊断来源分三类：

1. 后端主动生成：
   - 模型调用失败
   - MCP 不可用
   - 权限被拒绝
   - memory 服务异常
2. 前端推导生成：
   - artifact 文件不存在
   - Gateway 未支持某能力
   - 跳转目标不存在
3. 运行归并生成：
   - run 长时间停留在 waiting_user
   - 某个 step 失败后无明确用户提示

#### 8.2 诊断展示

工作台新增 `诊断` 标签，至少支持：

- 当前会话问题摘要
- 按严重程度排序
- 建议动作按钮
- 跳转到对应 run / permission / settings section

建议动作类型：

- 重试请求
- 打开设置
- 前往权限中心
- 查看来源 run
- 忽略当前提示

#### 8.3 与 unsupported 的区别

Plan 4 中的 `unsupported` 是能力协商结果，不等同于诊断错误。

规则：

- 未支持能力默认不进入高优先级诊断列表
- 只有当用户主动点击某功能且该能力未支持时，才生成一条 `protocol` 类低严重度诊断
- 避免工作台长期堆积“后端未升级”的噪音提示

### 9. 工作区恢复设计

#### 9.1 恢复边界

恢复分两层：

- 本地 UI 恢复：由 Tauri 或前端本地存储负责
- 业务态恢复：由 Gateway 返回

本地恢复内容：

- 最近会话 ID
- 当前 Agent ID
- Console 是否打开
- 当前标签
- 最近选中的 run / artifact / permission / diagnostic

业务恢复内容：

- 当前是否存在未完成 run
- 当前待确认请求
- 当前诊断摘要
- 最近一次运行状态快照

#### 9.2 恢复规则

应用启动时：

1. 先恢复本地 UI 上下文
2. 再请求 `workspace.restore`
3. 若本地记录的 `sessionId` 与后端已不存在，前端清理本地记录并回退安全默认值
4. 若后端返回 `view_only`，前端只恢复到对应 run 详情，不展示“继续执行”假按钮
5. 若后端返回 `reattachable`，前端允许重新订阅当前 run 的事件流

#### 9.3 不恢复的内容

以下内容不能由前端自行恢复：

- 伪造 `running` 状态
- 伪造某个 permission request 仍然有效
- 伪造某个 artifact 文件仍然存在
- 伪造某个重试动作仍可执行

这些都必须以 Gateway 或宿主层回源结果为准。

### 10. 协议补齐范围

在 Plan 4 的基础上，本 Plan 需要新增或扩展以下接口：

| 接口 | 类型 | 用途 |
|------|------|------|
| `session.runs` | 新增 request | 拉取会话执行历史 |
| `run.detail` | 新增 request | 拉取单次执行详情 |
| `run.control` | 扩展 request | stop / resume_waiting / 预留 pause/resume/retry |
| `session.artifacts` | 扩展 request | 会话或 run 维度的 artifact 列表 |
| `permission.pending` | 新增 request | 获取待确认权限请求 |
| `permission.respond` | 新增 request | 同意/拒绝权限请求 |
| `audit.logs` | 新增 request | 获取审计日志 |
| `diagnostics.current` | 新增 request | 获取当前会话诊断 |
| `workspace.restore` | 新增 request | 获取工作区恢复信息 |

需要新增或补语义的事件：

| 事件 | 说明 |
|------|------|
| `run.status.updated` | 运行状态变化 |
| `run.step.updated` | 运行步骤变化 |
| `session.artifacts.updated` | 产物列表变化 |
| `permission.requested` | 新权限请求到达 |
| `permission.resolved` | 权限请求被处理 |
| `audit.logs.updated` | 审计记录刷新 |
| `diagnostics.updated` | 当前诊断变化 |
| `workspace.restore.available` | 启动后存在可恢复工作区 |

### 11. 前端信息架构

合并后的工作台建议新增 3 个一级标签：

- `运行`
- `权限`
- `诊断`

其中：

- `运行`：当前 run、历史列表、run 详情、artifact 子区域
- `权限`：pending requests、recent decisions、audit logs
- `诊断`：当前问题、建议动作、恢复入口

artifact 不再单独占一个一级标签，避免与 run 详情重复。它应作为：

- `运行` 标签内的二级分区
- 聊天区右侧的轻量快捷面板

### 12. 详细实施步骤

本 Plan 的实施顺序分 6 个阶段。

#### 阶段 A：协议与类型建模

1. 在 `nova-protocol` 中定义：
   - `RunSummaryView`
   - `RunDetailView`
   - `RunStepView`
   - `PermissionRequestView`
   - `AuditLogView`
   - `DiagnosticIssueView`
   - `WorkspaceRestoreView`
2. 定义 request/response：
   - `SessionRunsRequest/Response`
   - `RunDetailRequest/Response`
   - `RunControlRequest/Response`
   - `SessionArtifactsRequest/Response`
   - `PermissionPendingRequest/Response`
   - `PermissionRespondRequest/Response`
   - `AuditLogsRequest/Response`
   - `DiagnosticsCurrentRequest/Response`
   - `WorkspaceRestoreRequest/Response`
3. 补齐事件 payload 类型与序列化测试。
4. 明确 `pause/resume/retry` 的 `capability_not_supported` 返回规范。

阶段完成标准：

- 类型模型稳定
- request/response 与事件命名一致
- 权限、诊断、恢复不再各自发明独立 ID 体系

#### 阶段 B：后端运行聚合与埋点

1. 在 runtime 层建立 `run -> steps -> artifacts -> permissions -> diagnostics` 的聚合链路。
2. 将已有 `ProgressEvent` 语义化映射为 step。
3. 在权限确认流程中补：
   - pending 队列
   - resolved 结果
   - 审计记录
4. 在 artifact 生成或宿主打开失败时补诊断埋点。
5. 在应用恢复入口补 `workspace.restore` 聚合逻辑。

阶段完成标准：

- 任一 pending permission 都能追溯到 session 和 run
- 任一 failed run 至少能给出错误摘要或诊断引用
- workspace restore 能区分 `view_only` 和 `reattachable`

#### 阶段 C：前端状态层与事件归并

1. 在 `gateway-client.ts` 增加：
   - `listSessionRuns`
   - `getRunDetail`
   - `controlRun`
   - `listSessionArtifacts`
   - `getPendingPermissions`
   - `respondPermissionRequest`
   - `getAuditLogs`
   - `getCurrentDiagnostics`
   - `restoreWorkspace`
2. 增加事件订阅：
   - `onRunStatusUpdated`
   - `onRunStepUpdated`
   - `onSessionArtifactsUpdated`
   - `onPermissionRequested`
   - `onPermissionResolved`
   - `onDiagnosticsUpdated`
3. 在 `AppState` 中按 `sessionId` 增加：
   - run 列表缓存
   - run 详情缓存
   - artifact 缓存
   - permission 缓存
   - audit 缓存
   - diagnostics 缓存
   - workspace restore 状态
4. 增加当前选中 run / artifact / permission / diagnostic 的 UI 状态。
5. 对旧 Gateway 未支持的能力接入 Plan 4 的 `unsupported` 语义。

阶段完成标准：

- 多会话不会串 run、artifact、permission、diagnostic
- 事件乱序不会覆盖掉更新的 run 详情
- restore 状态与实时状态能共存，不相互污染

#### 阶段 D：运行与 artifact UI

1. 在 `agent-console-view.ts` 中增加 `运行` 标签。
2. 实现当前运行卡片。
3. 实现执行历史列表与筛选。
4. 实现 run 详情时间线。
5. 在 run 详情中嵌入 artifact 子区域。
6. 补充 artifact 预览、路径复制、定位到 run 等动作。

阶段完成标准：

- 用户不翻聊天记录也能找到执行历史和产物
- run 与 artifact 的关联可视化完整

#### 阶段 E：权限、诊断与恢复 UI

1. 在工作台中增加 `权限` 标签。
2. 将即时浮层与确认中心打通，确保同一请求可在两个入口处理。
3. 实现最近确认记录与审计筛选。
4. 在工作台中增加 `诊断` 标签。
5. 为诊断项接入跳转动作：
   - 跳到设置页
   - 跳到权限页
   - 跳到 run 详情
6. 应用启动时先恢复本地 UI，再接入 `workspace.restore` 结果修正。

阶段完成标准：

- 高风险请求有统一入口和可追踪记录
- 错误展示不再只有原始报错字符串
- 应用重启后用户能回到上次工作上下文

#### 阶段 F：测试与回归

1. 为协议对象和事件 payload 补序列化测试。
2. 为 `AppState` 补 run/permission/diagnostic 归并测试。
3. 为 Console 补组件测试：
   - 当前运行态
   - waiting_user
   - artifact 过滤
   - permission pending
   - diagnostic 跳转
4. 为 workspace restore 补集成测试。
5. 为宿主动作失败补诊断回归测试。
6. 为多会话并发和旧 Gateway 降级补 E2E。

阶段完成标准：

- 关键运行链路和恢复链路都有自动化覆盖
- unsupported、失败、空态三类场景都被区分验证

### 13. 测试矩阵

#### 13.1 运行与历史

- 正常路径：会话运行后生成 run 记录，并在历史列表中展示。
- 状态流转：收到 `waiting_user`、`completed`、`failed`、`stopped` 后 UI 正确更新。
- 降级场景：后端不支持 `pause` 时，前端不展示暂停按钮或明确标记不可用。
- 多会话隔离：切换会话后只展示当前会话的 runs、steps 和详情。

#### 13.2 Artifact

- Artifact 聚合：同一 run 生成多个文件时，工作台可按类型过滤。
- 来源回溯：从 artifact 能跳转到来源 run 和 step。
- 文件缺失：本地文件不存在时，UI 显示失败状态并生成 `artifact` 诊断。
- 宿主打开：调用系统打开文件后生成一条审计记录。

#### 13.3 权限与审计

- 权限确认：收到高风险请求时，浮层出现，同时确认中心出现对应记录。
- 审批结果：批准和拒绝都会更新 request 状态，并生成审计日志。
- waiting_user 联动：permission pending 时对应 run 进入 `waiting_user`。
- rememberScope：选择 session/agent/global 时，UI 提示作用范围正确。

#### 13.4 诊断

- MCP 断开：诊断页显示类别、摘要和建议动作。
- 权限被拒绝：诊断能跳转到对应 permission 和 run。
- unsupported 能力：仅在用户主动触发时生成低严重度 `protocol` 诊断。
- 重试建议：`retryable = true` 的错误展示重试入口。

#### 13.5 工作区恢复

- 应用重启后恢复最近会话和工作台标签。
- 本地 sessionId 已失效时，前端清理本地恢复记录并回退默认视图。
- 后端返回 `view_only` 时，只恢复查看上下文，不显示继续执行按钮。
- 后端返回 `reattachable` 时，前端能重新订阅运行事件。

#### 13.6 旧 Gateway 降级

- 不支持 `session.runs` 时，运行标签显示 unsupported，而不会影响其他标签。
- 不支持 `permission.pending` 时，权限标签显示“需要后端升级”，不会阻塞聊天。
- 不支持 `workspace.restore` 时，仅恢复本地 UI 上下文。

### 14. 验收标准

- 用户能实时看到当前 run 状态、历史记录、步骤上下文和产物清单。
- 权限请求、等待原因、处理结果和审计记录形成完整链路。
- 错误发生后，用户能看到可读诊断、建议动作和关联 run。
- 应用重启后能恢复最近工作上下文，但不会伪造业务运行状态。
- 多会话场景下 run、artifact、permission、diagnostic、restore 状态完全隔离。
- 旧 Gateway 下，相关标签可按能力粒度平滑降级。

## 测试案例

- 正常路径：一轮会话执行完成后，工作台中同时出现 run 记录、步骤时间线和关联 artifact。
- 权限阻塞：工具调用触发高风险写文件请求时，run 进入 `waiting_user`，权限标签出现 pending request，批准后 run 恢复推进。
- 失败诊断：MCP 调用失败后，run 详情显示错误摘要，诊断标签展示 `mcp` 类问题并提供跳转到设置页的入口。
- Artifact 缺失：历史 run 中引用的文件被用户删除后，打开 artifact 会生成诊断并在审计中记录失败动作。
- 恢复场景：应用重启后恢复到上次查看的 run 详情；若该 run 已不可恢复，只保留历史查看态。
- 降级兼容：旧 Gateway 不支持 `workspace.restore` 和 `permission.pending` 时，前端仍可恢复本地标签与会话选择，不显示伪造业务状态。
