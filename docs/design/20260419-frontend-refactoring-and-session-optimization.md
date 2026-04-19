# 前端重构与 Session 业务优化设计

## 1. 基本信息
* **日期**：2026-04-19
* **模块**：Zero-Nova DeskApp (Vanilla TS 前端)
* **负责人**：Antigravity
* **状态**：**Draft**

---

## 2. 现状描述 (Current Status)

### 2.1 代码规模与结构

目前前端系统依赖单体结构开发，呈现出以下技术瓶颈：

| 文件 | 行数 | 职责 |
|------|------|------|
| `src/main.ts` | **~8,150** | 所有 DOM 操作、事件绑定、业务流程、状态管理 |
| `src/gateway-client.ts` | ~1,530 | WebSocket 通信协议封装（已拆出） |
| `src/voice.ts` | ~1,180 | 语音录制/播放/TTS（已拆出） |
| `src/evolution-ui.ts` | ~580 | Agent 自进化 UI（已拆出） |
| `src/cosmicHole.ts` | ~310 | Canvas 粒子动画（已拆出） |
| `src/markdown.ts` | ~130 | Markdown 渲染（已拆出） |
| `src/i18n/` | ~350 | 国际化系统（已拆出） |

`main.ts` 内含 **164 个顶层函数**、**大量模块级变量**作为全局状态，所有功能区域（聊天、侧边栏、设置面板十余个 Tab、调度器、Artifacts、语音模式、Router、WeChat、云端登录等）的 DOM 查询和事件绑定全部耦合在一起。

### 2.2 状态管理现状

无响应式框架，所有状态以模块级 `let` / `const` 变量存储：

```typescript
// 会话相关
let currentSessionId: string | null
let currentAgentId: string | null
const loadingSessions: Set<string>
const sessionDrafts: Map<string, string>
const chatTargetSessionIds: Set<string>
const unreadSessionIds: Set<string>

// 流式消息
let streamingMessageEl: HTMLElement | null
let streamingContent: string

// Agent / Artifacts / 工作模式
let agentsList: Array<...>
let artifacts: Artifact[]
let currentWorkingMode: WorkingMode  // 'standalone' | 'router' | 'managed'
```

状态变化后需手动调用 `renderSessions()` / `renderMessages()` 等函数刷新 DOM，缺乏统一的变更通知机制。

### 2.3 业务概念混淆

* **Agent 与 Session 边界模糊**：侧边栏以 "Agent（会话）" 形式展示，实际每个条目是独立 Session，但被冠以 Agent 名称。
* **功能缺失**：不支持在选定一个 Agent 后管理多条独立对话流（历史对话列表、新建对话）。

---

## 3. 目标 (Objectives)

为了降低系统复杂度和满足后续的交互设计需求，拆分为两个主要目标阶段：

1. **阶段一 — 纯代码模块化**：在不更改当前业务逻辑与用户 UI 外观的前提下，将 `main.ts` 解构为面向对象的模块架构（Service / UI Component）。
2. **阶段二 — Agent 与 Session 分离**：在 UI 侧将 Agent（角色/工具组合）和 Session（对话实例）从根本上分离，支持查看历史 Session 和基于指定 Agent 新建 Session。

---

## 4. 实现方式 (Implementation Strategy)

### 阶段一：纯代码架构拆分（无业务变化）

严格保持现有 UI 与业务逻辑，仅做文件级、模块级的安全转移。

#### 4.1.1 目标目录结构

```
src/
├── main.ts                  # 瘦入口：实例化 + 组装（目标 < 300 行）
├── gateway-client.ts        # （已有）WebSocket 通信层
├── voice.ts                 # （已有）语音模块
├── evolution-ui.ts          # （已有）自进化 UI
├── cosmicHole.ts            # （已有）粒子动画
├── markdown.ts              # （已有）Markdown 渲染
├── preview.ts               # （已有）独立预览窗口
├── feedback.ts              # （已有）反馈窗口
│
├── core/                    # 新增：核心层
│   ├── state.ts             # 集中状态存储 + 简易事件总线
│   ├── event-bus.ts         # 发布/订阅事件中心
│   └── types.ts             # 共享类型定义（Message, Session, Artifact 等）
│
├── ui/                      # 新增：视图层
│   ├── sidebar-view.ts      # 侧边栏 + 会话列表
│   ├── chat-view.ts         # 聊天窗口 + 输入框 + 流式渲染
│   ├── settings-view.ts     # 设置面板框架 + Tab 路由
│   ├── agent-config-view.ts # Agent 配置/编辑表单
│   ├── artifacts-view.ts    # Artifacts 面板
│   ├── scheduler-view.ts    # 调度器视图
│   ├── voice-overlay.ts     # 语音模式覆盖层
│   ├── modals.ts            # 通用模态框（确认、登录、文件预览）
│   └── titlebar.ts          # 标题栏 + 状态指示器
│
├── i18n/                    # （已有）
│   ├── index.ts
│   ├── zh.ts
│   └── en.ts
│
└── styles/
    └── main.css             # （已有）
```

#### 4.1.2 核心层设计

**`core/types.ts`** — 将 `main.ts` 中散落的接口定义集中：

```typescript
// 从 main.ts 提取：Message, MessageAttachment, ToolCall, LogEntry,
// Session, PendingAttachment, Artifact, ArtifactCategory,
// AgentModelItem, SkillItem, WorkingMode, SessionProgressState
// 从 gateway-client.ts 重导出：ProgressEvent, ChatIntentPayload 等
```

**`core/event-bus.ts`** — 轻量发布/订阅，替代目前函数间直接调用的耦合方式：

```typescript
type EventHandler<T = any> = (payload: T) => void;

class EventBus {
  private handlers = new Map<string, Set<EventHandler>>();
  on<T>(event: string, handler: EventHandler<T>): () => void { ... }
  emit<T>(event: string, payload: T): void { ... }
}

// 预定义事件名：
// 'session:selected'    — { sessionId: string }
// 'session:created'     — { session: Session }
// 'session:deleted'     — { sessionId: string }
// 'messages:updated'    — { sessionId: string }
// 'agent:switched'      — { agentId: string }
// 'streaming:start'     — { sessionId: string }
// 'streaming:token'     — { token: string }
// 'streaming:end'       — { sessionId: string, message: Message }
// 'settings:toggle'     — { visible: boolean }
// 'theme:changed'       — { theme: string }
```

**`core/state.ts`** — 集中状态存储，对外暴露读写方法，写入时通过 EventBus 发布变更通知：

```typescript
class AppState {
  constructor(private bus: EventBus) {}

  // Session 状态
  currentSessionId: string | null = null;
  sessions: Session[] = [];
  loadingSessions = new Set<string>();
  sessionDrafts = new Map<string, string>();
  // ... 其他状态字段

  selectSession(id: string) {
    this.currentSessionId = id;
    this.bus.emit('session:selected', { sessionId: id });
  }
  // ... 其他状态修改方法
}
```

#### 4.1.3 视图层设计

每个 View 类遵循统一生命周期接口：

```typescript
interface ViewComponent {
  init(): void;          // 查询 DOM 元素、绑定事件
  destroy?(): void;      // 解绑事件、清理资源
}
```

各 View 通过构造函数注入 `AppState` + `EventBus` + `GatewayClient`，不直接引用其他 View。

**函数归属映射表**（main.ts 中关键函数 → 目标模块）：

| main.ts 函数 | 目标模块 | 说明 |
|---|---|---|
| `loadSessions()`, `renderSessions()`, `selectSession()`, `createSession()` | `ui/sidebar-view.ts` | 会话列表管理 |
| `renderMessages()`, `renderMessagesWithLogs()`, `renderMessage()`, `addMessage()` | `ui/chat-view.ts` | 消息渲染 |
| `createStreamingMessage()`, `appendStreamingToken()`, `finishStreamingMessage()`, `renderStreamingMarkdown()` | `ui/chat-view.ts` | 流式消息 |
| `sendMessage()`, `sendMessageAsync()` | `ui/chat-view.ts` | 消息发送 |
| `showTyping()`, `updateTypingText()`, `hideTyping()` | `ui/chat-view.ts` | 输入指示 |
| `handleGatewayProgress()`, `handleChatIntent()` | `ui/chat-view.ts` | Gateway 事件处理 |
| `toggleSettingsView()`, `loadServerConfig()`, `renderMcpServers()`, `handleClientMcpServers()` | `ui/settings-view.ts` | 设置面板 |
| `loadAgentConfig()`, `saveAgent()`, `loadLocalAgents()`, `renderLocalAgents()`, `switchToAgent()` | `ui/agent-config-view.ts` | Agent 配置 |
| `addArtifact()`, `openFilePreview()`, `isArtifactTool()` | `ui/artifacts-view.ts` | Artifact 管理 |
| `loadSchedulerData()`, `showSchedulerToast()` | `ui/scheduler-view.ts` | 调度器 |
| `initVoice()`, `enterVoiceMode()`, `exitVoiceMode()`, `startVoiceRound()` | `ui/voice-overlay.ts` | 语音模式 |
| `showLoginModalForAtlas()`, `onopenfluxLoggedIn()` | `ui/modals.ts` | 登录模态 |
| `applyTheme()`, `updateSendButtonState()` | `main.ts` 或 `core/state.ts` | 全局逻辑 |
| `showSetupWizard()` | `ui/modals.ts` | 首次运行向导 |
| `loadMemoryData()`, `loadDistillationData()` | `ui/settings-view.ts` | 设置子 Tab |
| `startCloudChat()`, `switchToRouterSession()`, `loadRouterConfig()`, `initRouterListeners()` | `ui/settings-view.ts` | Router Tab |
| `initWeixinListeners()`, `updateManagedLlmUI()` | `ui/settings-view.ts` | WeChat / Managed LLM Tab |
| `hydrateMessageAttachments()` | `core/state.ts` 或 `ui/chat-view.ts` | 附件处理 |

#### 4.1.4 迁移策略

采用**渐进式迁移**，按以下顺序逐步执行，每一步均保证 `vite build` 通过且功能不变：

1. **Step 1：提取类型定义** — 将所有 `interface` / `type` 移入 `core/types.ts`，`main.ts` 和 `gateway-client.ts` 改为 import。
2. **Step 2：建立 EventBus + AppState 骨架** — 创建 `core/event-bus.ts` 和 `core/state.ts`，在 `main.ts` 中实例化但暂不改变逻辑流。
3. **Step 3：逐个提取 View 模块** — 按依赖关系从叶子节点开始：
   - `titlebar.ts` → `modals.ts` → `artifacts-view.ts` → `scheduler-view.ts`
   - → `settings-view.ts` / `agent-config-view.ts`
   - → `sidebar-view.ts`
   - → `chat-view.ts`（最复杂，最后处理）
   - → `voice-overlay.ts`
4. **Step 4：瘦化 main.ts** — 入口仅保留实例化和 `init()` 调度。

每完成一个 View 的提取，立即执行：
- `pnpm build`（TypeScript 编译 + Vite 打包）
- 手动启动 DeskApp 验证对应功能区域

#### 4.1.5 不在此阶段处理的事项

* 不引入 UI 框架（React / Vue / Svelte 等）
* 不修改 `gateway-client.ts` 的 API 接口
* 不改变 `index.html` 中的 DOM 结构
* 不变更任何用户可见的交互行为
* 不修改已拆出的模块（`voice.ts`, `evolution-ui.ts`, `cosmicHole.ts`, `markdown.ts`）

---

### 阶段二：Agent 与 Session 分离（业务优化）

在阶段一架构稳定的基础上进行业务逻辑变更。

#### 4.2.1 概念模型

```
Agent（角色定义）                Session（对话实例）
┌──────────────────┐           ┌──────────────────┐
│ id               │ 1    *   │ id               │
│ name             │──────────│ agentId          │  ← 新增关联字段
│ systemPrompt     │           │ title            │
│ tools / skills   │           │ createdAt        │
│ modelConfig      │           │ messages[]       │
└──────────────────┘           └──────────────────┘
```

核心变化：每个 Session 明确绑定一个 `agentId`，一个 Agent 可以拥有多个 Session。

#### 4.2.2 UI 交互设计

**方案：侧边栏两级结构**

```
┌────────────────────────────────┐
│  [≡] Zero-Nova             [+]│   ← [+] 基于当前 Agent 新建 Session
├────────────────────────────────┤
│  ▾ Agent: 默认助手              │   ← Agent 选择器（点击展开切换）
├────────────────────────────────┤
│  🔵 今天的对话                  │   ← Session 列表（按时间分组）
│     对话标题 A                  │
│     对话标题 B                  │
│  ○ 昨天                        │
│     对话标题 C                  │
│  ○ 更早                        │
│     对话标题 D                  │
├────────────────────────────────┤
│  [⚙] 设置   [📋] Agent 管理    │
└────────────────────────────────┘
```

关键交互流程：
- **切换 Agent**：侧边栏顶部 Agent 选择器，切换后自动加载该 Agent 关联的 Session 列表。
- **新建对话**：点击 [+] 按钮，基于当前选中 Agent 创建空 Session。
- **会话恢复**：点击历史 Session，加载 `getMessages(sessionId)` + `getLogs(sessionId)`。
- **删除对话**：右滑或右键菜单删除单个 Session，不影响同 Agent 下其他对话。

#### 4.2.3 Gateway 协议变更

需与后端确认/协调以下接口变更：

| 操作 | 现有协议 | 需要变更 |
|------|----------|----------|
| 获取 Session 列表 | `sessions.list` → 返回全部 Session | 新增 `agentId` 过滤参数，或前端过滤 |
| 创建 Session | `session.create` | 请求体增加 `agentId` 字段 |
| Session 数据结构 | `Session { id, title, createdAt, ... }` | 增加 `agentId: string` 字段 |
| 获取 Agent 下的 Session | 不存在 | 新增 `agent.sessions` 或复用 `sessions.list?agentId=xxx` |

**前端兼容策略**：如果后端短期内无法新增 API，前端可先在 `sessions.list` 返回结果上按 `agentId` 字段做客户端过滤，待后端支持后切换为服务端过滤。

#### 4.2.4 状态层变更

```typescript
// core/state.ts 新增
class AppState {
  // 新增
  currentAgentId: string | null = null;
  agentSessionsMap: Map<string, Session[]> = new Map();  // agentId → sessions

  switchAgent(agentId: string) {
    this.currentAgentId = agentId;
    this.bus.emit('agent:switched', { agentId });
    // 触发加载该 Agent 的 Session 列表
  }

  getSessionsForAgent(agentId: string): Session[] {
    return this.agentSessionsMap.get(agentId) ?? [];
  }
}
```

#### 4.2.5 阶段二的前置依赖

- 阶段一必须完成并验收通过
- 需与后端确认 Session 数据结构中 `agentId` 字段的存储与返回
- 需确认现有 Session 的数据迁移方案（历史 Session 如何关联到 Agent）

---

## 5. 风险评估 (Risk Assessment)

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 阶段一拆分引入隐性 Bug（事件顺序、DOM 时序） | 功能回退 | 每提取一个 View 即做回归验证；保留 git 分支可随时回滚 |
| 模块间循环依赖 | 编译失败 / 运行异常 | EventBus 解耦；类型定义集中在 `core/types.ts`；View 之间不直接 import |
| 阶段二后端 API 不支持 agentId 过滤 | 功能受限 | 前端先做客户端过滤兜底 |
| 历史 Session 缺少 agentId 字段 | 数据不一致 | 提供数据迁移脚本，将旧 Session 按规则关联到默认 Agent |
| `index.html` DOM 结构变更导致 CSS 失效 | UI 样式错乱 | 阶段一不改 HTML；阶段二仅增量添加新 DOM 节点 |

---

## 6. 测试方法 (Testing Strategy)

### 阶段一验收标准（代码拆分）

1. **构建检查**：
   - `pnpm build` 通过，无 TypeScript 编译错误。
   - 无 `any` 类型逃逸（不新增 `any`，允许保留现有的）。
   - 无循环依赖警告。
2. **代码质量检查**：
   - `main.ts` 行数降至 **300 行以内**。
   - 每个新模块文件不超过 **1,500 行**。
   - 所有 View 类实现 `ViewComponent` 接口。
3. **回归测试（Manual）**：
   - 对话收发正常，Markdown 渲染、代码高亮、Mermaid 图表正常。
   - 流式消息逐字显示正常，进度卡片展开/折叠正常。
   - 语音模式录制/播放/TTS 正常。
   - 侧边栏 Session 列表加载、选择、创建、删除正常。
   - Settings 面板所有 Tab（Model / Agent / Skills / MCP / Memory / Distillation / Evolution / Router / WeChat / Output / Voice / Debug）切换与操作正常。
   - 调度器视图正常。
   - Artifacts 面板正常。
   - 文件预览模态框正常。
   - **该阶段完成时不应呈现任何可见的功能级改变**。

### 阶段二验收标准（业务优化）

1. **Agent-Session 结构**：
   - UI 上能清晰看到当前 Agent，以及该 Agent 下的历史 Session 列表。
   - 能基于选定 Agent 创建新 Session。
   - 切换 Agent 后，Session 列表随之刷新。
2. **数据隔离**：
   - 不同 Session 的消息互相隔离。
   - 删除某个 Session 不影响同 Agent 下其他 Session。
   - 不同 Agent 下的 Session 列表互不干扰。
3. **数据迁移**：
   - 升级后，旧版本创建的 Session 仍然可以访问。
   - 旧 Session 被正确关联到对应 Agent（或默认 Agent）。
4. **后端集成**：
   - Gateway WebSocket 协议中 `session.create` 正确携带 `agentId`。
   - `sessions.list` 返回的 Session 包含 `agentId` 字段。

---

## 7. 里程碑 (Milestones)

| 阶段 | 里程碑 | 交付物 |
|------|--------|--------|
| 阶段一-Step1 | 类型定义提取完成 | `core/types.ts` 创建，`main.ts` / `gateway-client.ts` 编译通过 |
| 阶段一-Step2 | EventBus + State 骨架就绪 | `core/event-bus.ts` + `core/state.ts` 创建，`main.ts` 中接入 |
| 阶段一-Step3 | 所有 View 模块提取完成 | `ui/*.ts` 全部创建，功能回归通过 |
| 阶段一-Step4 | main.ts 瘦化完成 | `main.ts` < 300 行，全面回归通过 |
| 阶段二-Step1 | Agent-Session 数据模型对齐 | 前后端确认 Session 结构变更，数据迁移方案确定 |
| 阶段二-Step2 | 侧边栏 UI 改造完成 | Agent 选择器 + Session 分组列表上线 |
| 阶段二-Step3 | 新建/恢复/删除对话流程完成 | 全面验收通过 |
