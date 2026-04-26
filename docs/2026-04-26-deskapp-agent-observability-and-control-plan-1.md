# Plan 1: 运行态控制台与信息架构

## 前置依赖

无

## 本次目标

为 `deskapp` 定义一个贴近聊天主流程的 `Agent Console`，承载运行态可观测与临时控制能力，避免继续把所有功能堆进 Settings。

## 涉及文件

- `deskapp/src/main.ts`
- `deskapp/src/core/state.ts`（165 行，需扩展）
- `deskapp/src/core/types.ts`（321 行，需扩展）
- `deskapp/src/ui/chat-view.ts`（666 行，注意：不应在此文件中直接添加控制台渲染逻辑，应拆分）
- `deskapp/src/ui/agent-console-view.ts`（**新增**：独立的控制台视图模块）
- `deskapp/src/ui/sidebar-view.ts`
- `deskapp/src/ui/templates/agent-console-template.ts`（**新增**）
- `deskapp/src/styles/main/agent-console.css`（**新增**：独立样式文件，不膨胀 chat.css）
- `deskapp/src/styles/main/chat.css`（1693 行，仅做入口按钮和布局衔接修改）

## 详细设计

### 1. 入口与布局

- 在聊天主界面增加 `Agent Console` 入口按钮。
- 桌面宽屏使用右侧抽屉；窄屏和小窗口使用覆盖式面板。
- 面板分为 5 个标签：
  - `概览`
  - `模型`
  - `工具`
  - `技能`
  - `Prompt/Memory`

建议布局如下：

- 聊天头部右上角新增 `Inspect` 按钮。
- 点击后打开 `agent-console` 容器。
- 容器内部采用：
  - 头部：当前 Agent 名称、当前会话标题、连接状态、刷新按钮
  - 次级导航：5 个标签页
  - 主体：对应标签的内容区域
  - 页脚：最近更新时间、数据来源说明
- 快捷键：`Ctrl/Cmd + I` 切换控制台开关（需避免与浏览器默认快捷键冲突）。

### 1.1 DOM 结构建议

建议在 `chat-view` 的 HTML 模板中新增固定结构，避免运行时拼接大量散碎节点：

```html
<div id="chat-main" class="chat-main">
  <!-- 现有聊天区域 -->
  <div id="messages" class="messages">...</div>
  <div class="input-container">...</div>
</div>
<aside id="agent-console" class="agent-console hidden">
  <header class="agent-console-header">
    <span class="agent-console-title"></span>
    <span class="agent-console-status"></span>
    <button class="agent-console-refresh" title="刷新"></button>
    <button class="agent-console-close" title="关闭"></button>
  </header>
  <nav class="agent-console-tabs">
    <button class="tab active" data-tab="overview">概览</button>
    <button class="tab" data-tab="model">模型</button>
    <button class="tab" data-tab="tools">工具</button>
    <button class="tab" data-tab="skills">技能</button>
    <button class="tab" data-tab="prompt-memory">Prompt/Memory</button>
  </nav>
  <section class="agent-console-body"></section>
</aside>
```

这样做的目的是让：

- 面板开关只影响单个容器 class（`hidden` ↔ 移除）
- 标签切换只替换 body 子区域
- 样式集中到独立的 `agent-console.css`，不污染 chat 或 settings 样式

**与现有布局的空间关系**：`agent-console` 与 `#chat-main` 使用 flex 并排布局（宽屏）或绝对定位覆盖（窄屏）。现有 `.artifacts-panel`（chat.css 中已定义）与 Agent Console 的共存规则如下：

| 操作 | artifacts-panel 状态 | Agent Console 状态 | 说明 |
|------|---------------------|-------------------|------|
| 打开 Console | 自动收起（添加 `.collapsed`） | 打开 | Console 优先占据右侧空间 |
| 关闭 Console | 恢复到打开 Console 之前的状态 | 关闭 | 使用 `artifactsPanelWasOpen` 标记恢复 |
| 打开 artifacts-panel（Console 已开） | 不操作 | 保持打开 | 不允许同时展示两个右侧面板 |
| 会话切换 | 重置为默认状态 | 保持当前开关状态 | 面板内容刷新，开关状态不变 |

实现方式：在 `AppState` 中维护 `artifactsPanelWasOpen: boolean` 标记，Console 打开时记录并收起，Console 关闭时根据标记恢复。两者为互斥关系，不需要动画同步。

### 1.2 窄屏适配策略

- 宽度 ≥ 1024px：右侧抽屉，宽度 360px，聊天区等比收窄
- 宽度 < 1024px：全屏覆盖面板，带半透明遮罩和关闭按钮
- 宽度 < 768px：覆盖面板高度 100vh，底部留出输入框区域的安全距离

### 2. 状态建模

在 `AppState` 中新增一组只读快照状态。

建议新增统一 loading/error 包装：

```ts
interface ResourceState<T> {
    loaded: boolean;
    loading: boolean;
    error?: string;
    data?: T;
    updatedAt?: number;
}
```

然后在 `AppState` 中统一保存：

```ts
// --- Agent Console 状态 ---
consoleVisible: boolean;
consoleActiveTab: 'overview' | 'model' | 'tools' | 'skills' | 'prompt-memory';
agentRuntimeState: ResourceState<AgentRuntimeSnapshot>;
// 注意：使用 Map 而非 Record，支持任意 key 和高效迭代
sessionRuntimeStates: Map<string, ResourceState<SessionRuntimeSnapshot>>;
sessionPromptStates: Map<string, ResourceState<PromptPreviewView>>;
sessionToolStates: Map<string, ResourceState<ToolDescriptorView[]>>;
sessionMemoryHitStates: Map<string, ResourceState<MemoryHitView[]>>;
sessionTokenUsageStates: Map<string, ResourceState<TokenUsageView>>;
```

这样可避免：

- 打开不同会话时状态互相污染
- UI 无法区分"未加载"和"加载失败"

**与已有 AppState 的衔接**：

当前 `AppState` 使用 `EventBus` 广播状态变更（`SESSION_SELECTED`、`MESSAGES_UPDATED` 等）。控制台状态应沿用同一模式，新增以下事件：

- `CONSOLE_TOGGLED`：控制台打开/关闭
- `CONSOLE_TAB_CHANGED`：标签页切换
- `CONSOLE_DATA_UPDATED`：某个 ResourceState 数据刷新（payload 携带 key 标识哪个状态变了）

**ResourceState 工具方法**（实际实现为 `AppState` 的实例方法）：

```ts
// 在 AppState 类中
createEmptyResource<T>(): ResourceState<T> {
    return { loaded: false, loading: false };
}

setLoadingResource<T>(state: ResourceState<T>): ResourceState<T> {
    return { ...state, loading: true, error: undefined };
}

setLoadedResource<T>(data: T): ResourceState<T> {
    return { loaded: true, loading: false, data, updatedAt: Date.now() };
}

setErrorResource<T>(error: string): ResourceState<T> {
    return { loaded: true, loading: false, error, updatedAt: Date.now() };
}
```

> 这些方法挂在 `AppState` 实例上而非顶层函数，以便后续扩展（如自动触发 EventBus 通知）。

### 3. 交互边界

- Settings 保留"系统配置与管理"职责。
- Agent Console 聚焦"当前 Agent / 当前会话运行态"。
- 对用户来说，模型切换等动作必须在 UI 上明确提示作用域：
  - 改全局 → 需要二次确认
  - 改 Agent 默认 → 需要提示"影响该 Agent 所有新会话"
  - 仅本会话覆盖 → 默认推荐选项，无需额外确认

**与 Settings 的跳转**：Agent Console 中的某些项目应提供"在设置中打开"的链接，例如：
- 模型标签 → 跳转到 Settings 的 Models 标签
- Memory 命中 → 跳转到 Settings 的 Memory 标签并定位到对应条目

### 4. 数据加载策略

- 打开 Agent Console 时按需加载。
- 会话切换时刷新当前会话 runtime snapshot。
- 收到增量事件后局部刷新对应区块，不整体重绘整个面板。

建议采用以下加载优先级：

1. 打开面板先拉 `agent.inspect` 和 `session.runtime`
2. 切到 `工具` 标签再拉 `session.tools.list`
3. 切到 `Prompt/Memory` 标签再拉 `session.prompt.preview` 和 `session.memory.hits`
4. 收到 runtime/token/tool/memory 事件时增量覆盖当前会话缓存

这样可以把首屏成本控制在最小范围。

**增量事件映射**：打开控制台后，以下已有事件应自动触发对应区块刷新：

| 已有事件 | 刷新区块 |
|---------|---------|
| `ChatComplete`（含 usage） | Token 统计卡片 |
| `ToolUnlocked` | 工具列表 |
| `SkillActivated` / `SkillSwitched` / `SkillExited` | 技能列表 |
| `ProgressEvent`（tool_start/tool_result） | 概览中的运行状态 |

### 4.1 缓存策略

- 当前会话缓存常驻，直到会话切换或主动刷新。
- 非当前会话缓存允许保留最近 3 个，超出后淘汰（LRU）。淘汰在 `updateSessionResourceState()` 中自动执行：每次写入新会话缓存时，检查 Map size，若超出阈值则删除最早插入的非当前会话条目（JS `Map` 天然保持插入顺序）。
- Agent 切换时清空 `agentRuntimeState`，防止沿用旧 Agent 视图。
- 连接断开时不清空缓存数据，仅在 UI 上标记"数据可能过期"。

### 5. UI 原则

- 首屏先展示"当前生效模型 / token / tool 数量 / skill 数量 / memory 命中数"五个摘要卡片。
- 每张卡片可跳转到对应明细标签。
- 空状态必须明确区分：
  - 未加载（skeleton 占位）
  - 当前无数据（明确文案，如"当前会话暂无工具调用记录"）
  - 当前 provider/agent 不支持（明确说明原因，如"该模型不返回 token 统计"）
  - 加载失败（显示错误信息 + 重试按钮）
- 数值卡片使用等宽字体，避免数字变化时布局跳动。
- 所有文案支持 i18n（沿用已有的 `data-i18n` 模式）。

### 6. 模块拆分建议

**关键变更：控制台逻辑从 chat-view.ts 独立出来。**

`chat-view.ts` 已有 666 行，再堆入控制台逻辑会导致维护困难。建议拆分为：

- `ui/agent-console-view.ts`（**新增**）
  - 负责控制台开关、标签切换、按需加载、局部渲染协调
  - 导出 `AgentConsoleView` 类，由 `main.ts` 初始化
  - 监听 `EventBus` 事件驱动数据刷新
- `ui/templates/agent-console-template.ts`（**新增**）
  - 控制台 HTML 模板（概览卡片、标签页内容骨架）
- `styles/main/agent-console.css`（**新增**）
  - 控制台独立样式，不膨胀 chat.css（已 1693 行）
- `chat-view.ts`
  - 仅负责在模板中预留 `#agent-console` 占位容器
  - 入口按钮点击事件通过 EventBus 广播给 `agent-console-view`
- `core/state.ts`
  - 负责 runtime snapshot 缓存和事件广播
  - 新增 `setConsoleVisible()`、`setConsoleTab()`、`updateResourceState()` 等方法
- `core/types.ts`
  - 增加控制台相关 View 类型（ResourceState、各 Snapshot 类型）
- `gateway-client.ts`
  - 增加控制台只读接口（`getAgentInspect`、`getSessionRuntime` 等）
  - 增加事件订阅（复用已有的 handler 注册模式）

### 7. 实施步骤

1. 在 `core/types.ts` 定义控制台基础 ViewModel 和 `ResourceState<T>`。
2. 在 `AppState` 中增加控制台缓存字段、UI 状态字段与更新方法。
3. 新建 `ui/templates/agent-console-template.ts`，定义控制台 HTML 模板。
4. 新建 `ui/agent-console-view.ts`，实现开关、标签切换、按需加载逻辑。
5. 在 `chat-view.ts` 的模板中加入 `#agent-console` 占位容器和入口按钮（最小改动）。**注意**：`#inspect-btn` 已在 `ChatView` 中声明并绑定，入口按钮的 DOM 挂载点已就位，仅需确认点击事件正确广播到 EventBus。
6. 新建 `styles/main/agent-console.css`，实现宽屏抽屉和窄屏覆盖层样式。
7. ~~在 `main.ts` 中初始化 `AgentConsoleView` 实例。~~ **已完成**：`main.ts` 已创建 `agentConsole` 实例并调用 `init()`。
8. 接入事件总线，让会话切换（`SESSION_SELECTED`）、Agent 切换（`AGENT_SWITCHED`）、连接断开（`onConnectionChange`）能驱动控制台状态刷新。
9. 补充控制台空态、错误态和 skeleton 样式。
10. 处理与现有 `.artifacts-panel` 的共存/切换逻辑。

### 8. 验收标准

- 控制台在不打开时不额外触发重型请求。
- 会话切换后 1 次交互内即可展示新会话概览。
- 断线时控制台不报 JS 异常，只显示不可用状态。
- 小窗口下控制台仍可打开、关闭和切标签。
- `chat-view.ts` 新增代码不超过 30 行（仅占位容器和入口按钮）。
- `agent-console.css` 与 `chat.css` 无样式冲突（使用 `.agent-console` 命名空间前缀）。

## 测试案例

- 正常路径：连接成功后打开控制台，概览能显示当前 Agent 与会话摘要。
- 会话切换：切换到另一会话后，控制台内容同步刷新且不残留上一个会话状态。
- 窄屏适配：小窗口下控制台以覆盖面板打开，不遮挡输入框的关键操作。
- 空状态：无 memory、无 skill、无 tool 时，显示明确空态文案（不是空白）。
- 连接异常：Gateway 断开时，控制台展示"数据不可用"状态，不应卡死 UI。
- 快捷键：`Ctrl/Cmd + I` 可切换控制台开关，连续快速按键不导致状态异常。
- 与 Artifact 面板共存：控制台打开时，现有 artifacts-panel 自动收起或合并，关闭控制台后恢复。
- 缓存淘汰：切换超过 4 个不同会话后，最早的非当前会话缓存被清除。
- 标签切换后数据保留：从"工具"切到"技能"再切回"工具"，已加载的工具列表不重新请求。
