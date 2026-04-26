# Plan 3: Tool / Skill / Memory / Prompt 可观测面

## 前置依赖

- Plan 1: 运行态控制台与信息架构

## 本次目标

设计四类 inspection 视图，让用户能看见当前 Agent 的实际装配结果与本轮运行依据。

## 涉及文件

- `deskapp/src/gateway-client.ts`
- `deskapp/src/core/types.ts`
- `deskapp/src/core/state.ts`
- `deskapp/src/ui/agent-console-view.ts`（Plan 1 新增）
- `deskapp/src/ui/templates/agent-console-template.ts`（Plan 1 新增）
- `deskapp/src/styles/main/agent-console.css`（Plan 1 新增）

## 详细设计

### 1. Tool 列表

定义工具快照对象，附运行态字段：

```ts
interface ToolDescriptorView {
    name: string;
    description?: string;
    source: 'builtin' | 'mcp_server' | 'mcp_client' | 'custom' | 'skill_unlocked';
    providerName?: string;
    enabled: boolean;
    parameterSchema?: Record<string, unknown>;
    lastUsedAt?: number;
    lastCallStatus?: 'success' | 'error' | 'running';
    unlockedBy?: string;
    unlockedReason?: string;
}
```

**与后端的对齐说明**：

- `parameterSchema` 对应后端 `ToolDefinition.input_schema`（JSON Schema 格式），前端展示时应简化为参数名 + 类型列表，不需要完整渲染 JSON Schema。
- `source: 'skill_unlocked'` 对应后端已有的 `ToolUnlockedPayload`，记录工具解锁来源（`tool_search` / `skill_activation` / `manual`）。`unlockedBy` 和 `unlockedReason` 字段直接映射 `ToolUnlockedPayload.source` 和 `ToolUnlockedPayload.reason`。
- `lastCallStatus` 可从已有的 `ProgressEvent`（`tool_start` → `running`，`tool_result` + `isError` → `error`/`success`）推导，无需新增后端接口。

**与已有 MCP 类型的关系**：

前端已有 `McpServerView { name, location, transport, ..., toolCount, status }` 用于 MCP Server 管理。`ToolDescriptorView` 中 MCP 来源的工具，其 `providerName` 应匹配 `McpServerView.name`，方便用户跳转到 Settings 中对应的 MCP Server 配置。

交互要求：

- 默认展示当前会话可用工具，而不是系统所有工具。
- 支持按来源筛选（增加 `skill_unlocked` 筛选项）。
- 点击工具项可展开 schema 摘要和最近一次调用状态。
- 运行中动态解锁的工具应有视觉提示（如 "新" 标签或高亮动画）。

### 2. Skill 列表

定义技能视图：

```ts
interface SkillBindingView {
    id: string;
    title: string;
    source: 'global' | 'agent' | 'runtime';
    enabled: boolean;
    summary?: string;
    contentPreview?: string;
    loadedFrom?: string;
    activatedAt?: number;
    sticky?: boolean;
}
```

`loadedFrom` 用于描述：

- 全局配置（`.nova/config.toml` 中的 skills）
- Agent 默认配置（Agent 定义中绑定的 skills）
- 某次运行临时注入（通过 SkillActivated 事件动态加载）

**与已有 Skill 事件的关联**：

后端已推送以下事件，前端应实时更新技能列表：

| 事件 | 前端动作 |
|------|---------|
| `SkillActivated` | 向列表新增条目，`source: 'runtime'`，展示 `sticky` 标记 |
| `SkillSwitched` | 更新当前活跃技能高亮 |
| `SkillExited` | 将对应条目标记为"已退出"（非 sticky 的移除，sticky 的保留但灰显） |

**与已有 SkillItem 类型的关系**：

前端 `types.ts` 已有 `SkillItem { id, title, content, enabled }`，用于 evolution.skills.list 返回的安装列表。`SkillBindingView` 是运行态绑定视图，两者职责不同但结构相似。建议：
- `SkillItem` 保留用于 Settings 中的技能管理
- `SkillBindingView` 用于 Agent Console 中的运行态展示
- 当用户从 Console 中点击"查看来源"时，跳转到 Settings 的技能列表并定位到对应 `SkillItem`

交互要求：

- 默认展示标题、来源、启用状态。
- 查看详情时展示内容预览和来源链路。
- 与 `evolution.skills.list` 区分：后者是安装表，这里是运行绑定表。
- runtime 来源的技能需要视觉区分（如标注"运行时加载"徽章）。

### 3. Memory 命中视图

定义命中对象：

```ts
interface MemoryHitView {
    id: string;
    summary: string;
    score?: number;
    reason?: string;
    sourceType?: 'semantic' | 'keyword' | 'distillation';
    turnId?: string;
    hitAt?: number;
}
```

**后端能力缺口说明**：

当前后端在 memory 注入阶段（prompt 构建时查询相关记忆并注入 system prompt）**未记录命中结果**。具体来说：
- `nova-core` 中 memory 查询发生在 prompt 构建过程中
- 查询结果直接拼装入 prompt，但没有保存"命中了哪些记忆、每条的评分和匹配原因"
- 因此 `session.memory.hits` 接口需要后端在 memory 注入阶段增加埋点逻辑

**首期替代方案**：在 `session.memory.hits` 后端实现之前，前端可以：
1. 调用已有的 `memory.search(lastUserMessage)` 做一次相似度搜索作为近似展示
2. 在 UI 上明确标注"以下为近似匹配结果，非实际注入内容"
3. 后端补充埋点后切换为精确数据

交互要求：

- 在回复完成后展示"本轮引用了哪些记忆"。
- 允许从命中项跳转到设置页中的 memory 详情或搜索结果。
- 复用现有 `memory.stats/list/search` 作为库级管理入口。

建议进一步区分：

- `最近一轮命中`：用于解释本轮回答
- `当前会话高频命中`：用于帮助用户理解长期上下文偏向

### 4. Prompt 预览

定义分段视图（需对齐后端 `TurnContext` 结构）：

```ts
interface PromptPreviewView {
    systemPrompt: string;
    skillFragments: Array<{ title: string; content: string }>;
    toolDescriptions: Array<{ name: string; description: string }>;
    memoryFragments: Array<{ id: string; content: string }>;
    conversationFragments: Array<{ role: string; summary: string }>;
    capabilityPolicy?: string;
    activeSkill?: { id: string; title: string };
    tokenBudget?: { maxTokens: number; iterationBudget: number };
    redacted: boolean;
}
```

**与后端 TurnContext 的映射关系**：

| PromptPreviewView 字段 | TurnContext 来源 | 说明 |
|------------------------|-----------------|------|
| `systemPrompt` | `TurnContext.system_prompt` | 最终拼装后的 system prompt |
| `skillFragments` | `TurnContext.active_skill` | 当前活跃技能的内容片段 |
| `toolDescriptions` | `TurnContext.tool_definitions` | 工具定义的 name + description 摘要 |
| `memoryFragments` | system_prompt 中的记忆注入部分 | 需要后端在构建时分离记忆片段 |
| `conversationFragments` | `TurnContext.history` | 历史对话的角色+摘要 |
| `capabilityPolicy` | `TurnContext.capability_policy` | 当前能力策略描述 |
| `activeSkill` | `TurnContext.active_skill` | 当前活跃技能标识 |
| `tokenBudget` | `TurnContext.max_tokens` / `iteration_budget` | token 和迭代预算 |

**注意**：原设计中的 `toolFragments: Array<{ name, content }>` 改为 `toolDescriptions: Array<{ name, description }>`，因为后端 `ToolDefinition` 的实际结构是 `{ name, description, input_schema }`，不存在 `content` 字段。前端展示工具说明时应使用 `description` 而非重新拼装 content。

交互要求：

- 以分段折叠面板展示，而不是一大段纯文本。
- 支持复制单段或复制全部。
- 默认展示脱敏版；若未来开放完整版，需要额外显式确认。
- `tokenBudget` 和 `capabilityPolicy` 作为辅助信息展示在面板底部。

### 5. 运行链路串联

建议形成以下用户链路：

1. 在聊天中看到回复结果
2. 打开 Agent Console 查看本轮 token / tools / memory hits
3. 进入 Prompt 预览理解 system prompt 与 skills 拼装结果
4. 再决定是否调整模型、技能或全局配置

**从 Prompt 预览到行动的跳转**：

- 点击 skillFragment → 跳转到"技能"标签并定位到该技能
- 点击 toolDescription → 跳转到"工具"标签并定位到该工具
- 点击 memoryFragment → 跳转到 Settings 的 Memory 管理并搜索该条目
- 点击 capabilityPolicy → 跳转到"模型"标签查看当前配置

### 6. 视图分区设计

建议标签页内容按下述方式组织：

- `工具`
  - 顶部筛选器：全部 / 内建 / MCP / 自定义 / 技能解锁
  - 工具数量摘要（如"12 个工具可用，3 个本轮已调用"）
  - 中间工具列表（卡片式，含状态图标）
  - 点击展开：schema 摘要 + 最近调用状态
- `技能`
  - 当前绑定技能列表
  - 每项含：标题、来源标签、启用状态开关（只读）
  - 点击后查看内容预览（折叠展开，非弹窗）
  - runtime 来源的技能带"运行时"徽章
- `Prompt/Memory`
  - 上半区：Prompt 分段折叠视图（默认折叠，点击展开各片段）
  - 下半区：最近一轮 memory hits（含评分和来源标签）
  - 底部：辅助信息（capability policy、token budget）

### 7. 与现有能力的衔接

- Tool 管理与 `mcp` 设置页是两套职责：
  - Settings 负责"配置有哪些工具源"（MCP Server 增删改、启用禁用）
  - Console 负责"当前会话实际可用哪些工具"（运行态快照）
  - Console 中 MCP 工具的 `providerName` 可跳转到 Settings 中对应的 MCP Server 配置
- Skill 运行绑定与 `evolution.skills.list` 也分离：
  - `evolution.skills.list` 是安装列表（对应 `SkillItem` 类型）
  - `SkillBindingView[]` 是生效列表（运行态）
  - Console 中的"查看来源"可跳转到 Settings 的技能安装列表
- Memory 命中不替代设置页 memory 浏览，只增加"命中解释层"。
  - Console 中的 memory hit 点击后，调用已有的 `memory.search` 在 Settings 中定位
- **Skill 事件的实时更新**：前端已有 `onSkillsUpdated` 回调注册，Agent Console 应订阅此回调以及 `SkillActivated`/`SkillSwitched`/`SkillExited` 事件，实时刷新技能列表。

### 8. 实施步骤

1. 在 `core/types.ts` 定义 `ToolDescriptorView`、`SkillBindingView`、`MemoryHitView`、`PromptPreviewView`。
2. 在 `gateway-client.ts` 增加：
   - `listSessionTools(sessionId)` → `ToolDescriptorView[]`
   - `getSessionMemoryHits(sessionId, turnId?)` → `MemoryHitView[]`
   - `previewSessionPrompt(sessionId)` → `PromptPreviewView`
   - `getAgentInspect(agentId, sessionId?)` → 聚合视图
3. 在 `AppState` 中增加工具、技能、Prompt、memory 命中缓存（使用 Plan 1 的 `ResourceState<T>` 包装）。
4. 在 `agent-console-view.ts` 中实现"工具"标签页：只读列表 + 来源筛选。
5. 在 `agent-console-view.ts` 中实现"技能"标签页：绑定列表 + 内容预览展开。
6. 在 `agent-console-view.ts` 中实现"Prompt/Memory"标签页：分段折叠 + memory hits。
7. 接入已有 Skill 事件（`SkillActivated`/`SkillSwitched`/`SkillExited`）和 Tool 事件（`ToolUnlocked`）的实时刷新。
8. 实现 `ProgressEvent`（`tool_start`/`tool_result`）到 `lastCallStatus` 的推导逻辑。
9. 补工具筛选、技能详情展开、Prompt 分段复制交互。
10. 接入 memory hit 到 Settings memory 搜索结果的跳转链路。

### 9. 验收标准

- 用户能从当前会话看到实际可用工具，而不是系统总表。
- 技能内容展示的是运行绑定结果，不是安装仓库总表。
- runtime 动态加载的技能和工具有明确的视觉区分。
- Prompt 预览按片段展示，且默认脱敏。
- Memory 命中与当前回复有明确关联，不是孤立列表。
- `ToolUnlocked`/`SkillActivated` 等事件到达后，对应标签页内容实时更新。

## 测试案例

- Tool 视图：MCP 工具、内建工具、自定义工具、技能解锁工具能正确分组展示。
- Tool 筛选：选择"MCP"筛选后，只显示 MCP 来源的工具。
- Tool 运行态：工具被调用后，`lastCallStatus` 从 `running` 变为 `success` 或 `error`。
- Tool 动态解锁：收到 `ToolUnlocked` 事件后，新工具出现在列表中并带"新"标签。
- Skill 视图：某个 Agent 未绑定技能时显示空态；绑定多来源技能时来源标记正确。
- Skill 动态：收到 `SkillActivated` 事件后，技能列表新增 runtime 来源的条目。
- Skill 退出：收到 `SkillExited` 事件后，非 sticky 技能从列表移除，sticky 技能灰显。
- Memory 命中：本轮没有命中记忆时显示"未引用记忆"，不是空白。
- Memory 近似：在后端未实现精确命中前，近似搜索结果有明确的"近似"标注。
- Prompt 预览：脱敏字段不会暴露 API key、环境变量、原始密钥内容。
- Prompt 字段映射：`toolDescriptions` 展示的是工具 description 而非完整 input_schema。
- 跳转链路：从 memory hit 点击进入 memory 管理区后，能定位到对应条目或搜索结果。
- 跳转链路：从 Prompt 中的 skill fragment 点击后，切换到"技能"标签并高亮对应条目。
