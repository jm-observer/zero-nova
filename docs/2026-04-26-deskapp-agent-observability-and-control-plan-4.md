# Plan 4: Gateway 协议补齐与测试方案

## 前置依赖

- Plan 2: LLM 切换与 Token 统计
- Plan 3: Tool / Skill / Memory / Prompt 可观测面

## 本次目标

补齐 Agent Console 所需的 Gateway 请求接口、事件协议、能力协商、前端状态同步约束与测试矩阵，并把 Plan 1-3 已识别的遗留问题收敛为可执行的实施步骤。

## 涉及文件

- `deskapp/src/gateway-client.ts`
- `deskapp/src/core/types.ts`
- `deskapp/src/core/state.ts`
- `deskapp/src/core/event-bus.ts`
- `deskapp/src/ui/agent-console-view.ts`
- `deskapp/src/ui/settings-view.ts`
- `deskapp/src/i18n/en.ts`
- `deskapp/src/i18n/zh.ts`
- `deskapp/e2e/tests/*`
- `crates/nova-protocol/*`
- `crates/nova-gateway-core/*`
- `crates/nova-core/*`

## 详细设计

### 1. Plan 4 的职责边界

本 Plan 只负责把前后端交互契约、状态同步规则和测试方案定义清楚，不展开 Plan 5/6 的完整业务设计。

本 Plan 负责解决的问题：

- Plan 2/3 提出的新增只读接口与会话级覆盖接口如何命名、如何返回、如何降级
- 新旧 Gateway 如何做能力协商，避免前端依赖字符串错误文案
- 运行态事件如何映射到前端 EventBus，避免继续新增散落的字符串字面量事件
- `gateway-client.ts` 在继续扩展之前如何拆分或收敛复杂度
- 现有评审问题如何落地到明确修复步骤和测试用例

本 Plan 不负责：

- `run.control`、执行历史、Artifact 面板的完整领域模型
- 权限确认中心、审计中心、诊断恢复、工作区恢复的完整协议细节
- Prompt 在线编辑、Skill 在线启停等写操作扩展

### 2. 协议补齐范围

Plan 1-3 已确定 Console 首期必须覆盖的能力为：

- Agent 静态/半静态 inspect
- 会话 runtime 快照
- Prompt 预览
- 当前会话工具快照
- 当前会话技能绑定快照
- 记忆命中或近似命中
- 会话级模型覆盖
- 会话级 token 累计与单轮 usage 同步

因此 Plan 4 首期协议范围限定为以下 8 个接口：

| 接口 | 类型 | 用途 |
|------|------|------|
| `agent.inspect` | 新增 request | 聚合 Agent 静态与半静态视图 |
| `session.runtime` | 新增 request | 会话运行态快照真源 |
| `session.prompt.preview` | 新增 request | Prompt 分段预览 |
| `session.tools.list` | 新增 request | 当前会话最终工具装配结果 |
| `session.skill.bindings` | 新增 request | 当前会话技能绑定结果 |
| `session.memory.hits` | 新增 request | 最近一轮或指定轮次记忆命中 |
| `session.model.override` | 新增 request | 设置或重置会话级模型覆盖 |
| `sessions.token_usage` | 新增或扩展 request | 按会话回源校正累计 token |

其中：

- `session.skill.bindings` 虽未在总览文档的“完全新增接口”中单列，但 Plan 3 已经依赖该能力输出运行态技能绑定表，因此在 Plan 4 中明确纳入协议补齐范围。
- `sessions.token_usage` 的命名沿用 backlog 文档中的表述；若后端最终决定并入 `session.runtime`，需在本 Plan 的能力矩阵中同步更新，不允许前后端各自发明别名。

### 3. 协议对象与返回约束

新增接口返回值必须满足以下共性约束：

1. 返回稳定的 ViewModel，不直接暴露 `TurnContext`、内部 runtime struct 或数据库实体。
2. 返回字段优先使用前端既有语义名，例如 `sessionId`、`turnId`、`lastUpdatedAt`，避免同一概念在不同接口中出现 `session_id` / `sessionId` / `sid` 混用。
3. 所有运行态接口都必须显式包含 `sessionId`，即使是由请求参数可推导，也不省略。
4. 所有可被事件增量更新的快照接口都必须带 `updatedAt` 或等价时间戳，便于前端做乱序覆盖保护。
5. 缺失字段优先使用可选字段表达，不用魔法空字符串代替。

建议的协议对象如下：

```ts
interface CapabilityErrorPayload {
    code: 'capability_not_supported' | 'invalid_request' | 'internal_error';
    message: string;
    capability?: string;
}

interface SessionRuntimeSnapshot {
    sessionId: string;
    modelBindings: {
        orchestration: ModelBindingDetailView;
        execution: ModelBindingDetailView;
    };
    tokenUsage?: TokenUsageView;
    lastTurnId?: string;
    lastRunStartedAt?: number;
    lastRunCompletedAt?: number;
    updatedAt: number;
}

interface SessionSkillBindingsView {
    sessionId: string;
    bindings: SkillBindingView[];
    updatedAt: number;
}
```

补充约束：

- `session.model.override` 成功时优先返回完整 `SessionRuntimeSnapshot`；若后端短期内只能返回 `{ success: true }`，前端必须立即回源 `session.runtime`，但这只作为过渡策略，不作为长期协议目标。
- `session.memory.hits` 尚未实现前，不允许用空数组伪装“没有命中”；必须返回 `capability_not_supported`，由前端触发近似搜索降级。

### 4. 接口就绪状态矩阵

为避免前后端实施阶段重复猜测，本 Plan 将首期接口按四种状态定义：

- `ready`：后端已实现，可直接接入
- `partial`：后端已有部分数据，但字段不完整或语义不稳定
- `planned`：后端尚未实现，需要新增
- `fallback`：前端允许通过已有接口近似降级

当前建议矩阵如下：

| 接口 | 当前状态 | 真源/基础 | 前端降级 |
|------|----------|-----------|----------|
| `agent.inspect` | `planned` | `TurnContext`、Agent 配置 | 概览面板显示“需要后端升级” |
| `session.runtime` | `planned` | session runtime 聚合层 | 概览面板显示“需要后端升级” |
| `session.prompt.preview` | `planned` | `agent.prepare_turn()` + `TurnContext` | Prompt 面板显示“需要后端升级” |
| `session.tools.list` | `planned` | `TurnContext.tool_definitions` | 工具面板仅显示本轮调用过的工具高亮，不展示完整快照 |
| `session.skill.bindings` | `planned` | skill runtime 绑定表 | 技能面板退化为 `evolution.skills.list` 安装表并标注“非运行态” |
| `session.memory.hits` | `planned` | memory 注入埋点 | 调用 `memory.search(lastUserMessage)` 近似展示 |
| `session.model.override` | `planned` | session runtime override 存储 | 会话级覆盖入口置灰并说明未支持 |
| `sessions.token_usage` | `partial` | `chat.complete.usage` 聚合 | 使用完成态增量值并在重连/切会话时校正 |

矩阵维护规则：

- 本表是 Plan 4 的实施输入，后续进入开发阶段后必须在实现 PR 中逐项更新状态。
- 如果某接口状态从 `planned` 改为 `ready`，对应前端降级路径不能立即删除，必须至少保留一个版本周期作为兼容保护。

### 5. 能力协商与错误码约定

新增接口不能依赖“方法不存在”或后端报错文案做能力识别，统一约定：

1. Gateway 对未支持能力返回结构化错误：
   - `code = 'capability_not_supported'`
   - `capability = '<requestType>'`
2. 前端 `gateway-client.ts` 将其转换为统一错误对象：
   - `kind: 'unsupported' | 'request_failed'`
   - `capability?: string`
3. `AppState` 中的 `ResourceState<T>` 需要支持区分“未支持”和“加载失败”：

```ts
interface ResourceState<T> {
    status: 'idle' | 'loading' | 'ready' | 'error';
    data?: T;
    error?: string;
    unsupported?: boolean;
    updatedAt?: number;
}
```

前端 UI 约束：

- `unsupported = true` 时展示“该功能需要后端升级”，可带重试按钮，但不展示危险错误态样式。
- `unsupported = false` 且 `status = 'error'` 时展示真实失败态，如网络错误、解析错误、服务异常。
- 组件不得通过匹配 `error === '接口暂未支持'` 来分支逻辑，必须读取 `unsupported`。

### 6. 事件协议与 EventBus 统一

Plan 4 要同时解决后端 Gateway 事件和前端 EventBus 命名分裂问题。

#### 6.1 Gateway 侧新增事件

首期新增或语义化包装以下事件：

| Gateway 事件 | 触发时机 | 说明 |
|-------------|----------|------|
| `session.runtime.updated` | 模型覆盖、最近 turn、运行时间变化 | 更新概览真源 |
| `session.token.usage` | 单轮完成或累计值刷新 | 更新 token 展示 |
| `session.tools.updated` | 工具装配变化、解锁、移除 | 更新工具快照 |
| `session.skill.bindings.updated` | skill 激活、切换、退出后 | 更新技能快照 |
| `session.memory.hit` | 一轮完成并记录命中后 | 更新命中详情 |

payload 最低要求：

- `sessionId: string`
- `updatedAt: number`
- 能唯一标识覆盖目标的关键字段，如 `turnId`、`toolName`、`bindingId`

#### 6.2 前端 EventBus 常量收敛

所有新增前端事件必须注册到 `Events` 常量，禁止继续在业务层散落字符串字面量。

建议新增常量：

```ts
Events.CONSOLE_RUNTIME_UPDATED
Events.CONSOLE_TOKEN_UPDATED
Events.CONSOLE_TOOLS_UPDATED
Events.CONSOLE_SKILLS_UPDATED
Events.CONSOLE_MEMORY_UPDATED
Events.SETTINGS_NAVIGATE
```

规范要求：

- `gateway-client.ts` 负责把 Gateway 原始事件转换为语义化的前端事件。
- `agent-console-view.ts`、`settings-view.ts` 不直接消费后端裸消息类型。
- 旧的字符串字面量事件不要求在本 Plan 一次性全部迁移，但与 Agent Console 新增链路相关的事件必须同步迁移。

#### 6.3 Settings 跳转协议统一

评审记录已指出 `navigateTo()` 当前链路不闭环。Plan 4 明确统一事件：

```ts
interface SettingsNavigatePayload {
    visible: true;
    section?: 'models' | 'memory' | 'mcp' | 'skills';
    search?: string;
    itemId?: string;
}
```

约束：

- Console 跳转到 Settings 时统一发送 `Events.SETTINGS_NAVIGATE`
- `settings-view.ts` 监听 `Events.SETTINGS_NAVIGATE`
- 需要定位的列表项必须提供稳定的 `data-item-id`
- Settings 打开与定位是同一条链路，不再拆成“先 toggle 再自己猜测滚动”

### 7. `gateway-client.ts` 的组织方式

backlog 已明确 `gateway-client   .ts` 已经膨胀，Plan 4 必须在继续接协议前先做结构收敛。首期建议采用“按职责拆分文件、保留统一实例入口”的折中方案：

| 文件 | 职责 |
|------|------|
| `gateway-client.ts` | transport、连接管理、通用 `request()`、公共订阅基础设施 |
| `gateway-console-api.ts` | `agent.inspect`、`session.runtime`、`session.prompt.preview`、`session.tools.list`、`session.skill.bindings`、`session.memory.hits`、`session.model.override` |
| `gateway-evolution-api.ts` | 已有 `evolution.*` 相关能力 |
| `gateway-memory-api.ts` | 已有 `memory.*` 能力 |

实现约束：

- 不引入新的 transport 层，不改变调用方通过 `GatewayClient` 单实例使用的方式。
- 可采用组合模式：`GatewayClient` 聚合若干子 API 模块。
- 若当前迭代不方便立即拆文件，至少先抽出 console 相关私有 helper，禁止继续把 Plan 4-6 新接口全部堆入原文件底部。

### 8. 前端状态同步规则

运行态数据同步必须遵守“快照为真源、事件为增量、回源为校正”的三层模型。

#### 8.1 加载顺序

1. 进入会话时加载最小化会话元信息。
2. 打开 Console 时先请求 `session.runtime`。
3. 用户切到具体标签时懒加载：
   - `tools` → `session.tools.list`
   - `skills` → `session.skill.bindings`
   - `prompt-memory` → `session.prompt.preview` 与 `session.memory.hits`
4. 收到事件时做增量覆盖。
5. 回复完成、WebSocket 重连、会话切换后做一次轻量回源校正。

#### 8.2 会话隔离

所有 Console 运行态缓存必须按 `sessionId` 存储，至少包括：

- runtime snapshot
- token usage
- tools
- skill bindings
- prompt preview
- memory hits
- token 校正计数器

禁止以下做法：

- 用全局单值记录 token 校正次数
- 用 `Map<skillId, ...>` 保存跨会话 skill 绑定状态
- 切换会话时直接复用上一会话的 Prompt/Memory 数据

#### 8.3 Prompt / Memory 分离失败处理

`prompt` 与 `memory` 不能再放在一个 `Promise.all()` 失败单元中。规则如下：

- `session.prompt.preview` 失败时，只影响 Prompt 子区域
- `session.memory.hits` 未支持时，切换到近似搜索降级
- `session.memory.hits` 真失败时，只影响 Memory 子区域
- 页面整体标签仍保持可用

#### 8.4 token 校正规则

- `chat.complete.usage` 只用于增量刷新
- `sessions.token_usage` 或 `session.runtime.tokenUsage` 是累计真源
- 校正计数按 `sessionId` 维护
- WebSocket 重连成功后，对当前会话强制校正一次

### 9. 详细实施步骤

为避免 Plan 4 再次停留在概念层，实施顺序细化为 5 个阶段。

#### 阶段 A：协议建模与错误语义

1. 在 `nova-protocol` 中补齐 request/response 类型：
   - `AgentInspectRequest/Response`
   - `SessionRuntimeRequest/Response`
   - `SessionPromptPreviewRequest/Response`
   - `SessionToolsListRequest/Response`
   - `SessionSkillBindingsRequest/Response`
   - `SessionMemoryHitsRequest/Response`
   - `SessionModelOverrideRequest/Response`
   - `SessionsTokenUsageRequest/Response`
2. 增加结构化错误码定义，明确 `capability_not_supported`。
3. 为新增 Gateway 事件定义 payload 类型与序列化测试。
4. 与后端确认字段命名与时间戳字段统一策略。

阶段完成标准：

- 所有新增协议都有稳定类型名
- 未支持能力有统一错误码
- 协议测试覆盖序列化与反序列化

#### 阶段 B：Gateway 聚合与后端埋点

1. 在 Gateway 路由层为新增 request 注册 handler。
2. 在 session runtime 层新增 `SessionRuntimeSnapshot` 聚合逻辑。
3. 从 `TurnContext` 组装：
   - Prompt 预览
   - 工具快照
   - Agent inspect 数据
4. 为 skill 运行态绑定增加快照读取能力。
5. 在 memory 注入阶段补埋点，保存最近一轮命中结果。
6. 对暂未完成的接口显式返回 `capability_not_supported`，禁止静默空实现。

阶段完成标准：

- 所有 handler 至少有成功路径或“未支持”路径
- memory hits 的“未实现”与“没有命中”语义可区分
- `session.model.override` 的返回值约定已经固定

#### 阶段 C：前端 transport 与状态层改造

1. 拆分或整理 `gateway-client.ts` 的 console 相关 API。
2. 新增语义方法：
   - `getAgentInspect`
   - `getSessionRuntime`
   - `previewSessionPrompt`
   - `listSessionTools`
   - `getSessionSkillBindings`
   - `getSessionMemoryHits`
   - `overrideSessionModel`
   - `getSessionTokenUsage`
3. 新增事件订阅封装：
   - `onSessionRuntimeUpdated`
   - `onSessionTokenUsage`
   - `onSessionToolsUpdated`
   - `onSessionSkillBindingsUpdated`
   - `onSessionMemoryHit`
4. 扩展 `ResourceState<T>`，支持 `unsupported`。
5. 把 Console 相关缓存全部改为按 `sessionId` 存储。
6. 把 Prompt / Memory 的加载逻辑拆分为两个独立资源状态。
7. 把 token 校正计数器改为会话维度。

阶段完成标准：

- `AppState` 不再有 Console 相关的全局混用缓存
- `unsupported` 状态能够在状态层被可靠表达
- 评审记录中的会话隔离问题有对应结构性修复

#### 阶段 D：UI 链路闭环与降级视图

1. `agent-console-view.ts` 接入新的资源状态与事件订阅。
2. `navigateTo()` 统一改用 `Events.SETTINGS_NAVIGATE`。
3. 为 tools / skills / memory / settings 列表项补 `data-item-id`。
4. 在 UI 中区分三类状态：
   - `loading`
   - `unsupported`
   - `error`
5. 对 `session.memory.hits` 未支持场景接入近似搜索降级，并加醒目标识。
6. 对 `session.skill.bindings` 未支持场景显示“安装表，不代表当前运行态”。
7. 对会话级模型覆盖未支持场景将入口置灰，而不是点击后报错。

阶段完成标准：

- Console 到 Settings 的跳转可用
- 各标签的降级语义明确，不再把“未支持”伪装成“加载失败”
- Prompt 与 Memory 面板可以独立成功或失败

#### 阶段 E：测试补齐与回归

1. 为新增协议补单元测试和序列化测试。
2. 为 `gateway-client` 和 `AppState` 补状态归并测试。
3. 为 Console 补组件测试，覆盖 unsupported/error/ready 三种态。
4. 为跳转链路补集成测试。
5. 为 i18n key 对称性增加脚本检查。
6. 为多会话并发、事件乱序、重连校正补 E2E 或集成测试。

阶段完成标准：

- 测试矩阵中的高优先级用例全部落地
- i18n 双语缺 key 能自动发现
- Agent Console 新增能力不回归原聊天主流程

### 10. 测试矩阵

#### 10.1 前端单元测试

| 场景 | 断言 |
|------|------|
| `capability_not_supported` | `ResourceState.unsupported = true`，不落入普通 error |
| 事件乱序 | 旧时间戳事件不会覆盖新快照 |
| 多会话 token 校正 | A/B 会话各自累计，不共享校正计数 |
| Skill 缓存 | `sessionId` 切换不会覆盖其他会话 skill bindings |
| Prompt/Memory 拆分 | memory 失败时 prompt 仍保持 ready |

#### 10.2 前端组件/集成测试

| 场景 | 断言 |
|------|------|
| unsupported 面板 | 显示“需要后端升级”而非危险错误 |
| memory 近似降级 | 显示近似标识与来源说明 |
| Settings 跳转 | `navigateTo()` 后打开对应 section 并定位条目 |
| 模型覆盖未支持 | 按钮置灰并有说明 |
| Prompt 脱敏 | `redacted = true` 时显示脱敏提示 |

#### 10.3 后端测试

| 场景 | 断言 |
|------|------|
| 协议序列化 | request/response 与事件 payload 可稳定序列化 |
| `session.runtime` 聚合 | 模型绑定、token、turn 元数据正确组装 |
| `session.model.override` | 设置、局部更新、reset 三种路径正确 |
| `session.memory.hits` | 未埋点时返回 unsupported；有命中时返回命中列表 |
| Prompt 预览 | 敏感字段被脱敏，history 截断策略生效 |

#### 10.4 E2E 测试

| 场景 | 断言 |
|------|------|
| 旧 Gateway 兼容 | Console 可打开，未支持能力平滑降级 |
| 多会话并发 | tools、skills、token、memory 不串会话 |
| 重连恢复 | WebSocket 重连后 token 与 runtime 回源校正 |
| 会话切换 | 切换到新会话时不会短暂展示旧会话 Prompt/Memory |
| 原流程回归 | 聊天、切会话、设置页基础功能仍可用 |

#### 10.5 i18n 与静态检查

新增脚本要求：

- 对比 `en.ts` 与 `zh.ts` 的 key 集合
- 报告单侧缺失 key
- 报告同 key 的插值参数不一致

该检查建议纳入前端测试或 CI lint 步骤，但不替代现有 Rust 侧 `cargo` 流程。

### 11. 风险与决策记录

#### 11.1 必须在 Plan 4 明确的决策

- `session.model.override` 是否返回完整 snapshot
- `sessions.token_usage` 是否独立存在，还是并入 `session.runtime`
- `gateway-client.ts` 是否在本轮拆文件，还是只做局部模块化
- `session.skill.bindings` 的 ID 是否与 `SkillItem.id` 同命名空间

#### 11.2 当前风险

- memory hits 后端埋点若延后，前端需要较长时间依赖近似搜索，解释性会受限
- `TurnContext` 到 PromptPreview 的映射如果没有稳定抽象，后续 Prompt 结构变更可能频繁破坏协议
- 如果继续允许字符串字面量事件扩散，Plan 5/6 会进一步放大维护成本
- 若不在 Plan 4 解决会话隔离，Plan 5 的 run/history 能力会直接建立在错误缓存模型上

### 12. 验收标准

- 所有 Plan 2/3 依赖的新增接口均有明确状态：`ready`、`partial`、`planned` 或对应降级方案。
- 前端能可靠区分“能力未支持”和“真实失败”。
- Console 相关状态全部按 `sessionId` 隔离，不再使用全局混合缓存。
- `navigateTo()` 到 Settings 的跳转链路闭环。
- EventBus 对 Console 新增链路不再依赖字符串字面量事件。
- 测试矩阵覆盖协议兼容、多会话隔离、事件乱序、降级路径和 i18n 对称性。

## 测试案例

- 协议兼容：旧 Gateway 返回 `capability_not_supported` 时，工具/技能/Prompt/Memory 面板分别进入 unsupported 状态，不互相拖垮。
- 会话隔离：A、B 两个会话连续触发 `SkillActivated` 和 token 更新后，各自 Console 只刷新本会话缓存。
- Prompt/Memory 分离：`session.memory.hits` 未支持时，Prompt 预览仍可正常显示，Memory 区域展示近似命中提示。
- token 校正：同一会话连续 3 轮后触发回源校正，其他会话的计数不受影响。
- Settings 跳转：从 Prompt 中点击 skill/tool/memory 跳转后，Settings 打开到正确 section，并高亮对应 `data-item-id`。
- 模型覆盖返回值：`session.model.override` 若仅返回成功标记，前端会立即补拉 `session.runtime`；若返回完整 snapshot，则不会额外发起第二次请求。
- skill ID 映射：运行态 `SkillBindingView.id` 无法直接映射 `SkillItem.id` 时，UI 不会跳到错误条目，而是退化为搜索模式或只打开技能 section。
- i18n 对称性：新增 `console.*` 与 `skills.*` key 后，`en.ts` / `zh.ts` 任一侧缺失都会使检查失败。
- 回归保护：新增协议和 EventBus 常量后，原有聊天消息发送、会话切换、设置页打开流程保持可用。
