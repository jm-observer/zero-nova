# Plan 6: 权限确认、诊断恢复与工作区持久化

## 前置依赖

- Plan 1: 运行态控制台与信息架构
- Plan 4: Gateway 协议补齐与测试方案
- Plan 5: 运行控制、执行历史与 Artifact 工作流

## 本次目标

为 `deskapp` 增加统一的权限确认中心、审计记录、错误诊断面板，以及应用重启后的工作区恢复能力。

## 涉及文件

- `deskapp/src/gateway-client.ts`
- `deskapp/src/core/types.ts`
- `deskapp/src/core/state.ts`
- `deskapp/src/ui/chat-view.ts`
- `deskapp/src/ui/modals.ts`
- `deskapp/src/ui/templates/*`
- `deskapp/src/styles/main/chat.css`
- `deskapp/src-tauri/src/*`
- `crates/nova-protocol/*`
- `crates/nova-gateway-core/*`

## 详细设计

### 1. 权限确认模型

建议新增：

```ts
interface PermissionRequestView {
    id: string;
    sessionId?: string;
    runId?: string;
    agentId?: string;
    kind: 'command' | 'file_write' | 'network' | 'mcp_tool';
    title: string;
    reason?: string;
    target?: string;
    createdAt: number;
    riskLevel: 'low' | 'medium' | 'high';
}

interface AuditLogView {
    id: string;
    sessionId?: string;
    runId?: string;
    actionType: string;
    actor: string;
    result: 'approved' | 'denied' | 'failed';
    summary: string;
    createdAt: number;
}
```

### 2. 确认中心设计

- 工作台中新增 `权限` 标签。
- 结构分为：
  - 待确认请求列表
  - 最近确认记录
  - 审计日志筛选区

高优先级请求仍可触发即时浮层，但所有请求必须同时进入确认中心，确保可追溯。

### 3. 诊断面板

建议新增：

```ts
interface DiagnosticIssueView {
    id: string;
    category: 'llm' | 'mcp' | 'memory' | 'permission' | 'protocol' | 'artifact' | 'unknown';
    severity: 'info' | 'warn' | 'error';
    title: string;
    message: string;
    suggestedActions: string[];
    relatedRunId?: string;
    relatedSessionId?: string;
    updatedAt: number;
}
```

工作台中新增 `诊断` 标签，支持：

- 查看当前会话问题摘要
- 查看建议动作
- 跳转到对应配置页或确认页
- 对支持重试的错误执行重试

### 4. 工作区恢复

恢复对象建议保存在桌面端本地：

- 最近会话 ID
- 当前 Agent ID
- 工作台打开状态
- 当前标签
- 最近查看的 runId / artifactId

注意：

- 未完成运行状态由 Gateway 决定是否可恢复。
- 前端本地只恢复“查看上下文”，不擅自伪造运行继续状态。

### 5. Tauri 与前端职责

- Tauri 层负责本地持久化轻量 UI 状态。
- Gateway 层负责恢复运行态、诊断态、权限请求等业务状态。
- 前端负责把两者合并成最终恢复体验。

### 6. 实施步骤

1. 在协议层定义 `permission.pending/respond`、`audit.logs`、`diagnostics.current`、`workspace.restore`。
2. 在 `gateway-client.ts` 中增加：
   - `getPendingPermissions`
   - `respondPermissionRequest`
   - `getAuditLogs`
   - `getCurrentDiagnostics`
   - `restoreWorkspace`
   - `onPermissionRequested`
   - `onDiagnosticsUpdated`
3. 在 `AppState` 中增加权限、诊断和恢复状态缓存。
4. 在工作台中加入 `权限` 与 `诊断` 标签。
5. 在 Tauri 层或前端本地存储中保存 UI 恢复信息。
6. 应用启动时合并本地恢复信息与 Gateway 返回的业务恢复信息。

### 7. 验收标准

- 所有高风险操作都能在确认中心找到记录。
- 错误发生后用户能看到明确建议，而不是只有原始报错字符串。
- 应用重启后能恢复最近工作上下文，不需要重新手动定位会话。
- 业务恢复和 UI 恢复职责边界清晰，不产生伪状态。

## 测试案例

- 权限确认：收到高风险请求时，浮层出现，同时确认中心出现对应记录。
- 审计记录：批准和拒绝都会生成可筛选日志。
- 诊断恢复：MCP 断开或模型失败时，诊断页能显示类别、摘要和建议动作。
- 工作区恢复：应用重启后恢复最近会话和工作台标签，不恢复错误的旧运行状态。
- 降级兼容：旧 Gateway 不支持恢复/诊断接口时，前端只恢复本地 UI 上下文。
