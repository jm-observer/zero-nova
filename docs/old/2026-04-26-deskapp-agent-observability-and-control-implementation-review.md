# DeskApp Agent 工作台实现评审记录

**时间**: 2026-04-26（创建）

**评审范围**:

- [总览文档](./2026-04-26-deskapp-agent-observability-and-control.md)
- [Plan 1: 运行态控制台与信息架构](./2026-04-26-deskapp-agent-observability-and-control-plan-1.md)
- [Plan 2: LLM 切换与 Token 统计](./2026-04-26-deskapp-agent-observability-and-control-plan-2.md)
- [Plan 3: Tool / Skill / Memory / Prompt 可观测面](./2026-04-26-deskapp-agent-observability-and-control-plan-3.md)

**评审对象**:

- [deskapp/src/core/state.ts](/D:/git/zero-nova/deskapp/src/core/state.ts)
- [deskapp/src/core/types.ts](/D:/git/zero-nova/deskapp/src/core/types.ts)
- [deskapp/src/ui/agent-console-view.ts](/D:/git/zero-nova/deskapp/src/ui/agent-console-view.ts)
- [deskapp/src/styles/main/agent-console.css](/D:/git/zero-nova/deskapp/src/styles/main/agent-console.css)
- [deskapp/src/i18n/en.ts](/D:/git/zero-nova/deskapp/src/i18n/en.ts)
- [deskapp/src/i18n/zh.ts](/D:/git/zero-nova/deskapp/src/i18n/zh.ts)

---

## 结论

本次实现已经补入了部分 Plan 2/3 的细节，例如：

- `PromptPreviewView.toolDescriptions` 与旧字段兼容
- memory 近似结果增加了显式警告文案
- artifacts panel 与 Agent Console 的互斥逻辑更加清晰

但仍存在若干关键偏差，主要集中在**会话隔离不足**、**降级路径未按设计落地**、**跳转链路不可用**三个方面。这些问题会直接影响 Agent Console 在多会话和后端能力未齐备场景下的稳定性与可用性。

---

## 1. Skill 绑定缓存未按会话隔离

**严重级别**: 高

**涉及代码**:

- [deskapp/src/core/state.ts](/D:/git/zero-nova/deskapp/src/core/state.ts:92)
- [deskapp/src/core/state.ts](/D:/git/zero-nova/deskapp/src/core/state.ts:186)
- [deskapp/src/core/state.ts](/D:/git/zero-nova/deskapp/src/core/state.ts:201)
- [deskapp/src/ui/agent-console-view.ts](/D:/git/zero-nova/deskapp/src/ui/agent-console-view.ts:351)

**问题描述**:

Plan 1 明确要求 Console 运行态缓存按会话维度保存，避免不同会话之间状态互相污染；但当前 `skillBindingStates` 仍然是全局 `Map<string, ResourceState<SkillBindingView>>`，key 只有 `skillId`，不包含 `sessionId`。同时，`loadSkillsData()` 在重新加载时会先清空整个 `skillBindingStates`，再写入当前会话的结果。

这意味着：

- A 会话和 B 会话的 Skill 绑定结果会互相覆盖
- 最近 3 个非当前会话缓存的 LRU 策略对 Skill 数据不生效
- 多会话切换后，Console 展示的 Skill 列表可能来自上一次加载的其他会话

**与设计不一致点**:

- 不符合 Plan 1 的会话级 `ResourceState` 缓存模型
- 不符合 Plan 3 中“当前会话运行绑定表”的语义

**建议修复方向**:

- 将 Skill 绑定缓存改为按 `sessionId` 维度保存
- 复用 Plan 1 已建立的 `Map<sessionId, ResourceState<T>>` 模式
- Skill 事件对本地状态的更新也应带上会话边界，避免跨会话污染

---

## 2. Prompt 与 Memory 加载被绑定为单一失败单元

**严重级别**: 中

**涉及代码**:

- [deskapp/src/ui/agent-console-view.ts](/D:/git/zero-nova/deskapp/src/ui/agent-console-view.ts:374)
- [deskapp/src/ui/agent-console-view.ts](/D:/git/zero-nova/deskapp/src/ui/agent-console-view.ts:395)
- [deskapp/src/ui/agent-console-view.ts](/D:/git/zero-nova/deskapp/src/ui/agent-console-view.ts:403)

**问题描述**:

当前 `loadPromptMemoryData()` 使用 `Promise.all()` 同时请求：

- `getSessionPromptPreview(sessionId)`
- `getSessionMemoryHits(sessionId)`

只要 `session.memory.hits` 失败，整个 `Promise.all()` 就会 reject，随后代码会把 `prompt` 和 `memory` 两个资源状态一起写成错误态。

这会导致：

- 即使 Prompt 预览接口可用，也无法展示 Prompt 内容
- 后端尚未实现 `session.memory.hits` 时，整个 `Prompt/Memory` 标签可用性被拖垮

**与设计不一致点**:

Plan 3 明确要求 memory 命中在后端未实现前采用近似方案降级，例如按需调用 `memory.search(lastUserMessage)` 并在 UI 中标注“近似结果”。当前实现虽然增加了“近似匹配结果”提示文案，但并没有真正实现近似数据来源；实际仍然硬依赖 `session.memory.hits`。

**建议修复方向**:

- 将 Prompt 与 Memory 的加载和失败处理拆开，避免一个接口失败拖垮另一个面板
- 在 `session.memory.hits` 未支持时，按 Plan 3 设计走近似搜索降级
- 为“接口暂未支持”和“真实加载失败”区分不同 UI 状态，避免误导用户

---

## 3. `navigateTo()` 跳转链路当前不可用

**严重级别**: 中

**涉及代码**:

- [deskapp/src/ui/agent-console-view.ts](/D:/git/zero-nova/deskapp/src/ui/agent-console-view.ts:1024)
- [deskapp/src/core/event-bus.ts](/D:/git/zero-nova/deskapp/src/core/event-bus.ts:68)
- [deskapp/src/ui/settings-view.ts](/D:/git/zero-nova/deskapp/src/ui/settings-view.ts:36)
- [deskapp/src/ui/agent-console-view.ts](/D:/git/zero-nova/deskapp/src/ui/agent-console-view.ts:729)
- [deskapp/src/ui/agent-console-view.ts](/D:/git/zero-nova/deskapp/src/ui/agent-console-view.ts:784)
- [deskapp/src/ui/agent-console-view.ts](/D:/git/zero-nova/deskapp/src/ui/agent-console-view.ts:872)

**问题描述**:

Plan 3 定义了跨标签和跳转到 Settings 的统一导航接口 `navigateTo()`，但当前实现还没有形成可工作的闭环：

1. 跳转到 Settings 时，`navigateTo()` 发出的是 `Events.SETTINGS_TOGGLE`
2. 该事件当前在 `EventBus` 中的注释语义仍是 `{ visible: boolean }`
3. 现有 `SettingsView` 并未监听 `settings:toggle`，而是监听 `view:toggle`
4. 标签内定位依赖 DOM 上存在 `data-item-id`
5. 当前渲染出来的 tool/skill/memory 列表项没有设置 `data-item-id`

这意味着：

- 跳转到 Settings 不会生效
- 同一 Console 内的滚动定位和高亮也不会生效
- 该方法目前更像“接口占位”，还不是可交付能力

**与设计不一致点**:

- 不满足 Plan 3 中定义的“从 Prompt 到 Skill/Tool/Memory/Settings 的行动链路”

**建议修复方向**:

- 统一 Settings 打开事件和 payload 协议
- 为可定位列表项补充稳定的 `data-item-id`
- 补上实际调用 `navigateTo()` 的交互入口，形成端到端闭环

---

## 4. Token 校正计数是全局计数，不是会话维度计数

**严重级别**: 中

**涉及代码**:

- [deskapp/src/core/state.ts](/D:/git/zero-nova/deskapp/src/core/state.ts:86)
- [deskapp/src/core/state.ts](/D:/git/zero-nova/deskapp/src/core/state.ts:146)

**问题描述**:

Plan 2 的补充设计要求前端在增量累加 token 时定期回源校正，当前实现增加了 `tokenAccumulationCount`，并在累计达到 3 次时回调 `getSessionTokenUsage(sessionId)`。

但这个计数器是**全局单值**，不是按 `sessionId` 区分的。这会引出两个问题：

- 多会话并行产生 token 更新时，A/B 会话会共用同一计数器
- 某个会话可能因为别的会话的消息累加而提前或延后触发校正

**与设计不一致点**:

- 不符合会话级 runtime/cache 管理的总体思路
- 会在多会话场景下产生不可预期的校正时机

**建议修复方向**:

- 将计数器改为按会话维度维护
- 或直接把“校正阈值”记录在对应会话的 token resource 元数据中

---

## 建议优先级

建议修复顺序如下：

1. 先修复 Skill 缓存的会话隔离问题
2. 再拆开 Prompt/Memory 的失败处理，并补齐 memory 近似降级
3. 然后打通 `navigateTo()` 的事件和 DOM 定位链路
4. 最后把 token 校正计数改为会话维度

---

## 关联文档

- [遗留问题与延后事项](./2026-04-26-deskapp-agent-observability-and-control-backlog.md)
- [总览文档](./2026-04-26-deskapp-agent-observability-and-control.md)
- [Plan 1](./2026-04-26-deskapp-agent-observability-and-control-plan-1.md)
- [Plan 2](./2026-04-26-deskapp-agent-observability-and-control-plan-2.md)
- [Plan 3](./2026-04-26-deskapp-agent-observability-and-control-plan-3.md)
