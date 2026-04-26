# Plan 2: LLM 切换与 Token 统计

## 前置依赖

- Plan 1: 运行态控制台与信息架构

## 本次目标

定义模型切换的作用域、数据结构、交互约束，以及 token 统计的展示方式与数据来源。

## 涉及文件

- `deskapp/src/gateway-client.ts`
- `deskapp/src/core/types.ts`
- `deskapp/src/core/state.ts`
- `deskapp/src/ui/agent-console-view.ts`（Plan 1 新增）
- `deskapp/src/ui/templates/agent-console-template.ts`（Plan 1 新增）

## 详细设计

### 1. 模型切换作用域

定义统一结构：

```ts
interface ModelBindingView {
    provider: string;
    model: string;
    source: 'global' | 'agent' | 'session_override';
}
```

同一时间展示两套绑定：

- `orchestration`
- `execution`

建议扩展为可直接用于 UI 的完整视图：

```ts
interface ModelBindingDetailView {
    provider: string;
    model: string;
    source: 'global' | 'agent' | 'session_override';
    inheritedFrom?: string;
    editableScopes: Array<'global' | 'agent' | 'session_override'>;
}
```

这样可以直接支持：

- 当前生效来源标签
- "恢复继承"按钮是否可点
- 当前用户可以在哪个层级修改

**与已有类型的对齐**：

当前 `ServerConfigView.llm` 已有 `{ provider: string; model: string }` 结构用于全局模型，`ServerConfigView.agents.list` 中每个 Agent 已有 `model?: { provider: string; model: string }` 字段。`ModelBindingView` 应复用相同的 `{ provider, model }` 二元组格式，确保与 `config.update` 的数据结构一致。

**presetModels 的利用**：`ServerConfigView.presetModels` 已有 `Record<string, { value, label, multimodal }[]>` 按 provider 分组的预设模型列表，模型选择器应直接复用此数据，避免前端硬编码模型列表。

### 2. 操作模型

- 全局默认修改：沿用 `config.update`（对应 `ServerConfigUpdate.orchestration` / `ServerConfigUpdate.execution`）
- Agent 默认修改：沿用 `config.update`（对应 `ServerConfigUpdate.agents.list[].model`）
- 会话覆盖：新增 `session.model.override`

会话覆盖应满足：

- 只影响当前会话后续轮次
- 不回写 Agent 默认
- 支持"一键恢复继承"

**纠正**：原文档中"Agent 默认修改"写的是"复用或扩展 `agents.update`"，但当前代码中 Agent 模型修改实际走的是 `config.update` 的 `agents.list` 字段（见 `ServerConfigUpdate.agents`），并没有独立的 `agents.update` 接口。应统一使用 `config.update`。

### 2.1 推荐交互流程

用户在 `模型` 标签页中点击某个模型配置块后：

1. 先选择作用域：`全局默认` / `当前 Agent 默认` / `仅本会话`
2. 再选择 provider（从 `ServerConfigView.providers` 已配置的 provider 中选择）
3. 再选择 model（从 `ServerConfigView.presetModels[provider]` 中选择，或手动输入）
4. 提交后显示 toast 和来源标记更新

为了避免误操作，默认推荐：

- 聊天页只默认暴露"仅本会话"
- 全局和 Agent 默认修改放在展开区或二级操作中
- 全局修改需要二次确认弹窗

### 2.2 会话覆盖接口约束

`session.model.override` 建议语义如下：

```json
{
  "sessionId": "xxx",
  "orchestration": { "provider": "openai", "model": "gpt-4.1" },
  "execution": { "provider": "openai", "model": "gpt-4.1-mini" },
  "reset": false
}
```

- 只传某一项时表示局部覆盖
- `reset = true` 时忽略 `orchestration/execution`，恢复继承
- 返回最新完整 `SessionRuntimeSnapshot`（若后端仅返回 `{ success: true }` 而非完整 snapshot，前端应在操作成功后立即调用 `getSessionRuntime(sessionId)` 刷新缓存，以确保展示一致性）

### 2.3 与 copySession 的交互

当用户使用 `copySession` 复制会话时：

- **默认不继承**旧会话的 session override，新会话回落到 Agent/全局默认
- 后端在 `sessions.copy` 响应中应包含新会话的模型绑定信息
- 若后续需要支持"连带复制 override"，作为显式可选参数，不作为默认行为

### 3. Token 统计视图

定义展示对象（需对齐后端已有 `Usage` 结构）：

```ts
/** 对齐后端 nova-protocol::Usage 结构，纯数据传输用 */
interface UsageView {
    inputTokens: number;
    outputTokens: number;
    cacheCreationInputTokens?: number;
    cacheReadInputTokens?: number;
}

/** UI 展示用的 token 统计（含成本），用于 AppState 缓存 */
interface TokenUsageView {
    inputTokens: number;
    outputTokens: number;
    cacheCreationInputTokens?: number;
    cacheReadInputTokens?: number;
    totalCost?: number;
}

/** 单轮 token 统计 */
interface TurnTokenUsageView {
    turnId: string;
    usage: UsageView;
    estimatedCostUsd?: number;
}

/** 会话累计 token 统计 */
interface SessionTokenUsageView {
    sessionId: string;
    totalUsage: UsageView;
    turnCount: number;
    estimatedCostUsd?: number;
    lastUpdatedAt: number;
}
```

> **类型职责划分**：
> - `UsageView`：对齐后端 `Usage` 结构，用于 WebSocket 消息解析和接口返回值
> - `TokenUsageView`：前端 UI 展示用，在 `UsageView` 基础上增加 `totalCost`，作为 `sessionTokenUsageStates: Map<string, ResourceState<TokenUsageView>>` 的泛型参数
> - `TurnTokenUsageView`：单轮快照，用于聊天消息尾部的 `TokenUsageBadge`
> - `SessionTokenUsageView`：后端 `sessions.token_usage` 接口的返回类型，前端收到后映射为 `TokenUsageView` 写入缓存

**与原设计的差异说明**：

- 移除了 `orchestrationInput/orchestrationOutput/executionInput/executionOutput` 的拆分。原因：当前后端完成态只返回一个聚合 `Usage` 对象，不区分 orchestration / execution 两类模型调用。要实现拆分统计需要后端在 `ConversationService::execute_agent_turn()` 中分别记录两类调用的 usage，工作量较大。
- **首期策略**：展示聚合值（input_tokens / output_tokens / cache tokens），不做 orchestration/execution 拆分。
- **后续扩展**：如果后端支持分类 usage 推送，只需在 `UsageView` 中增加 `category?: 'orchestration' | 'execution'` 字段。
- 新增了 `cacheCreationInputTokens` / `cacheReadInputTokens`，因为这两个字段已在后端 `Usage` 中存在且影响实际成本计算。

### 4. 数据来源

- 会话累计值以 `session.runtime` / `sessions.token_usage` 返回的后端聚合结果为真源。
- 前端在收到完成态消息时，可根据 payload 中的 `usage` 做当前界面的增量刷新，但刷新后或重新进入会话时必须以 runtime 快照回填，避免双源状态漂移。
- 成本估算只在后端掌握模型单价映射时返回；前端不自行硬编码价格表。

**已有完成态消息结构**（WebSocket `chat.complete` payload）：

```json
{
  "session_id": "xxx",
  "output": "...",
  "usage": {
    "input_tokens": 1200,
    "output_tokens": 380,
    "cache_creation_input_tokens": 0,
    "cache_read_input_tokens": 500
  }
}
```

**前端归并逻辑**：在 `AppState` 中维护 `sessionTokenUsageStates`，每次收到完成态消息时：

1. 提取 `usage` 字段
2. 若当前会话已有 runtime 快照，则基于该快照做界面增量更新
3. 若当前会话尚未拉到 runtime 快照，则先写入临时值，并在随后一次 `session.runtime` 拉取后覆盖
4. 触发 `CONSOLE_DATA_UPDATED` 事件

**注意**：前端临时累加仅用于实时展示优化，不作为持久状态来源。页面刷新、切换会话、复制会话后，均应重新读取 `session.runtime` 或 `sessions.token_usage`。

**累加误差回退策略**：前端增量累加可能因 WebSocket 断线重连导致消息丢失（累计偏低）或重复处理（累计偏高）。为此：
- 每次用户切换到 Console 的"概览"或"模型"标签时，强制调用 `getSessionTokenUsage()` 校正缓存值
- WebSocket 重连成功后，自动触发一次校正拉取
- 连续 3 轮累加后即使用户未切换标签，也在下一次 `chat.complete` 事件时顺带校正一次（避免长时间偏移）

### 4.1 前端归并规则

- 事件到达后优先更新当前轮次展示卡片。
- 若 `sessionId === currentSessionId`，同步更新控制台概览。
- 若是非当前会话，仅更新缓存，不主动打断用户界面。

### 5. UI 交互

- 在 Agent Console `模型` 标签中展示当前双模型与来源。
- 在聊天回复尾部展示本轮 token 摘要（紧凑格式：`↑1.2k ↓380 | $0.012`）。
- 鼠标悬停或点击后展示更细的输入/输出/cache 拆分。

建议组件区块：

- `ModelBindingCard`：展示 orchestration / execution 两组模型绑定，含来源标签和切换入口
- `TokenUsageBadge`：紧凑的 token 摘要，嵌入聊天消息尾部
- `TokenUsageDetailPanel`：展开后的完整统计，含 cache tokens 和成本估算

### 6. 边界约束

- 若当前 provider 不返回 usage，显示 `--`，同时注明"该模型未返回 token 统计"。
- 若会话正运行中切换模型，应提示"仅影响下一轮请求"。

补充约束：

- 正在流式回复时不允许提交同作用域模型变更（按钮置灰 + tooltip 说明）。
- 当前 session 没有 `sessionId` 时（如新建会话未发送消息），不展示"仅本会话覆盖"操作。
- 复制会话时，默认不继承旧会话的 session override（见 2.3 节）。
- provider 未配置 API key 时，该 provider 下的模型选项置灰并标注"未配置"。判定规则：`ServerConfigView.providers[name].apiKey` 为 `undefined`、`null` 或空字符串 `""` 时均视为"未配置"；provider key 存在但 `apiKey` 和 `baseUrl` 均为空时同样视为未配置。

### 7. 实施步骤

1. 在 `core/types.ts` 增加 `UsageView`、`TurnTokenUsageView`、`SessionTokenUsageView`、`ModelBindingView`、`ModelBindingDetailView`。
2. 在 `gateway-client.ts` 增加：
   - `getSessionRuntime(sessionId)` → 返回 `SessionRuntimeSnapshot`（含模型绑定和 token 累计）
   - `overrideSessionModel(sessionId, override)` → 返回更新后的 `SessionRuntimeSnapshot`
3. 在 `AppState` 中增加 runtime/token usage 缓存，并在完成态消息到达时做增量刷新；缓存真源仍为 `session.runtime` / `sessions.token_usage`。
4. 在控制台 `模型` 标签渲染当前绑定与来源（`ModelBindingCard`）。
5. 在聊天消息区接入本轮 token 摘要展示（`TokenUsageBadge`）。
6. 增加会话级覆盖交互，并实现"恢复继承"。
7. 最后再把全局/Agent 默认修改入口与现有 settings/agents 配置联动（复用 `config.update` 和已有的 Settings 模型选择器逻辑）。

### 8. 验收标准

- 三层模型来源显示正确。
- 会话级覆盖不会污染全局和 Agent 默认配置。
- token 统计在 provider 缺失 usage 时平滑降级（显示 `--`，不报错）。
- 本轮和累计 token 两种展示不会互相覆盖或串值。
- cache tokens（cache_creation / cache_read）在 UI 中有展示入口。

## 测试案例

- 正常路径：切换会话级 orchestration 模型后，下一轮请求使用新模型，概览显示 `session_override`。
- 恢复继承：点击恢复后，会话重新回落到 Agent 或全局默认模型。
- 累计统计：连续多轮对话后，会话 token 累计值持续增长。
- 缺失 usage：后端不返回 token 时，前端降级显示 `--`，不报错。
- 并发场景：回复流式输出期间收到 token 更新事件，UI 仅更新统计区域，不影响正文渲染。
- 流式期间切换：流式回复期间模型切换按钮不可点击。
- presetModels 联动：选择不同 provider 后，model 下拉列表自动更新为该 provider 的预设模型。
- 未配置 provider：API key 为空的 provider 在模型选择器中标注"未配置"。
- copySession 后：复制的新会话不继承原会话的 session override，模型来源回落到 agent 或 global。
- cache tokens 展示：有 cache_read_input_tokens 时，token 详情面板中能看到 cache 命中量。
