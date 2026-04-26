# Plan 5: 运行控制、执行历史与 Artifact 工作流

## 前置依赖

- Plan 1: 运行态控制台与信息架构
- Plan 4: Gateway 协议补齐与测试方案

## 本次目标

为 `deskapp` 引入以 `run` / `turn` 为中心的执行工作流，补齐运行控制、执行历史、任务计划和 artifact 聚合视图。

## 涉及文件

- `deskapp/src/gateway-client.ts`
- `deskapp/src/core/types.ts`
- `deskapp/src/core/state.ts`
- `deskapp/src/ui/chat-view.ts`
- `deskapp/src/ui/templates/*`
- `deskapp/src/styles/main/chat.css`
- `crates/nova-protocol/*`
- `crates/nova-gateway-core/*`

## 详细设计

### 1. 核心数据模型

建议新增：

```ts
interface RunSummaryView {
    id: string;
    sessionId: string;
    status: 'queued' | 'running' | 'waiting_user' | 'paused' | 'stopped' | 'failed' | 'completed';
    title?: string;
    startedAt: number;
    finishedAt?: number;
    durationMs?: number;
    modelSummary?: string;
    toolCount?: number;
    tokenUsage?: TokenUsageView;
    errorSummary?: string;
}

interface RunDetailView {
    summary: RunSummaryView;
    steps: RunStepView[];
    artifacts: SessionArtifactView[];
    errorDetail?: DiagnosticIssueView;
}

interface RunStepView {
    id: string;
    type: 'thinking' | 'tool' | 'approval' | 'message' | 'artifact';
    title: string;
    status: 'running' | 'completed' | 'failed' | 'skipped';
    startedAt?: number;
    finishedAt?: number;
    toolName?: string;
    description?: string;
}
```

### 2. 状态机设计

- `queued -> running`
- `running -> waiting_user`
- `running -> paused`
- `running -> stopped`
- `running -> failed`
- `running -> completed`
- `waiting_user -> running`
- `paused -> running`

如后端当前不支持真正 `pause`，前端仍预留状态字段，但 UI 上只开放 `stop`。

### 3. UI 组织

- 在 `Agent Workspace` 中新增 `运行` 标签。
- 分为三块：
  - 当前运行卡片
  - 最近执行历史列表
  - 选中 run 的详情区

### 4. Artifact 面板

建议单独二级标签或与 `运行详情` 并列：

- `全部`
- `文件`
- `代码`
- `输出`
- `图片`

每个 artifact 支持：

- 打开
- 定位到文件
- 复制路径
- 查看生成来源 run

### 5. 与聊天区关系

- 聊天气泡里仍可展示轻量结果。
- 工作台里的 run/artifact 面板承载完整运行信息和产物检索。
- 不要求消息区承担所有产物浏览职责。

### 6. 实施步骤

1. 在协议层定义 `run summary/detail`、`run status`、`artifacts` 相关 view。
2. 在 `gateway-client.ts` 增加：
   - `listSessionRuns`
   - `getRunDetail`
   - `listSessionArtifacts`
   - `controlRun`
   - `onRunStatusUpdated`
   - `onRunStepUpdated`
   - `onSessionArtifactsUpdated`
3. 在 `AppState` 中增加 run/artifact 缓存。
4. 在工作台中加入 `运行` 标签和基本列表 UI。
5. 接入实时状态更新。
6. 增加 artifact 预览和文件打开/定位动作。

### 7. 验收标准

- 当前运行状态可实时刷新。
- 历史 run 可浏览且不会与其它会话串数据。
- artifact 能从会话维度稳定检索，不需要翻聊天记录。
- run 失败时能在详情里看到错误摘要和步骤上下文。

## 测试案例

- 正常路径：会话运行后生成 run 记录，并在历史列表中展示。
- 状态流转：运行中收到 `waiting_user`、`completed`、`failed` 等事件后 UI 正确更新。
- Artifact 聚合：同一 run 生成多个文件时，工作台可按类型过滤。
- 降级场景：后端不支持 `pause` 时，前端不展示暂停按钮或明确标记不可用。
- 多会话隔离：切换会话后只展示当前会话的 runs 和 artifacts。
