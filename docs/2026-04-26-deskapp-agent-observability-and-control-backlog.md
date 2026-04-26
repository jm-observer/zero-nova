# DeskApp Agent 工作台 — 遗留问题与延后事项

**时间**: 2026-04-26（创建）

**来源**: Plan 1-3 设计评审过程中识别的问题，需在 Plan 4-6 设计阶段一并处理。

---

## 1. gateway-client.ts 拆分

**优先级**: 中 | **建议归入**: Plan 4

**问题描述**: `gateway-client.ts` 已膨胀至 1215 行。Plan 1-3 新增了 `getAgentInspect`、`getSessionRuntime`、`getSessionTokenUsage`、`getSessionTools`、`getSessionMemoryHits`、`getSessionPromptPreview`、`setSessionModelOverride`、`resetSessionModelOverride`、`getSessionSkillBindings`、`onToolUnlocked`、`onSkillActivated`、`onSkillSwitched`、`onSkillExited` 等大量方法。Plan 4-6 还将继续添加 `run.control`、`permission.*`、`audit.*`、`diagnostics.*`、`workspace.*` 等接口。

**建议方案**:

- 方案 A：按职责域拆分为独立模块（如 `gateway-console-api.ts`、`gateway-evolution-api.ts`），通过 mixin 或组合模式注入 `GatewayClient`
- 方案 B：保持单文件但提取内部 section 注释和 region 折叠标记
- 在 Plan 4 中做决策，因为 Plan 4 还会新增一批接口，是合适的重构时机

---

## 2. 后端接口就绪状态矩阵

**优先级**: 高 | **建议归入**: Plan 4

**问题描述**: Plan 1-3 设计文档中提出的新增 Gateway 接口（`agent.inspect`、`session.runtime`、`session.tools.list`、`session.prompt.preview`、`session.memory.hits`、`session.model.override`、`session.skill.bindings`、`sessions.token_usage` 等）后端实现状态不明。

**需要在 Plan 4 中完成**:

1. 与后端对齐，逐一确认每个接口的实现状态（已实现 / 开发中 / 待启动）
2. 为"待启动"的接口定义前端统一降级策略：
   - `ResourceState.error` 写入 `'接口暂未支持'`
   - UI 展示友好提示（如"该功能需要后端升级"）+ 重试按钮
   - 降级 UI 不应与"加载失败"混淆，建议使用 `ResourceState` 扩展一个 `unsupported?: boolean` 字段或使用特定 error code
3. 定义接口版本协商机制（可选），使前端能在连接时感知后端支持的接口范围

---

## 3. EventBus 事件命名规范统一

**优先级**: 中 | **建议归入**: Plan 4

**问题描述**: 当前 EventBus 存在两套命名风格并存的问题：

- **常量风格**（在 `Events` 对象中定义）：`Events.SESSION_SELECTED`、`Events.CONSOLE_TOGGLED`、`Events.PROGRESS_UPDATE` 等
- **字符串字面量风格**（散落在代码中）：`'token'`、`'chat:complete'`、`'tool:start'`、`'tool:result'`、`'chat:error'`、`'system:log'`、`'chat:iteration'`、`'message:send'`、`'session:loading_changed'`、`'agents:updated'`、`'attachments:updated'`、`'mcp:updated'` 等

这导致事件查找不便、类型安全缺失、新开发者难以确定该用哪种风格。

**建议方案**:

1. 在 Plan 4 中制定规范：所有新增事件必须在 `Events` 常量中注册
2. 逐步将字符串字面量事件迁移到 `Events` 常量中（可作为 Plan 4 的附带任务或独立 Plan）
3. 考虑为 EventBus 增加泛型事件映射，提供编译期类型安全：

```ts
interface EventMap {
    [Events.SESSION_SELECTED]: { sessionId: string };
    [Events.CONSOLE_DATA_UPDATED]: { key: string };
    // ...
}
```

---

## 4. SkillBindingView 与 SkillItem 的 ID 命名空间

**优先级**: 低 | **建议归入**: Plan 4 或 Plan 6

**问题描述**: `SkillItem`（Settings 中的技能安装列表）和 `SkillBindingView`（Console 中的运行态绑定列表）都有 `id` 字段。Plan 3 设计中提到"从 Console 点击'查看来源'时跳转到 Settings 的技能列表并定位到对应 SkillItem"，这要求两者的 `id` 必须一致。

**需要确认**:

- 后端 `evolution.skills.list` 返回的 `SkillItem.id` 与 `session.skill.bindings` 返回的 `SkillBindingView.id` 是否使用同一 ID 体系
- 如果不一致，`SkillBindingView` 需要增加 `skillItemId?: string` 字段用于关联
- 运行时动态注入的技能（`source: 'runtime'`）可能没有对应的 `SkillItem`，跳转逻辑应做空值处理

---

## 5. i18n 双语对称性检查

**优先级**: 低 | **建议归入**: Plan 4（测试矩阵部分）

**问题描述**: 已发现 `zh.ts` 中有 `chat.error_iteration_limit` 但 `en.ts` 中没有。Plan 1-3 新增了大量 `console.*`、`tools.*`、`memory.*`、`skills.*` 相关 i18n key，如果不做自动化检查，很容易遗漏。

**建议方案**:

- 在 Plan 4 的测试矩阵中加入 i18n 对称性检查脚本
- 检查逻辑：对比 `en.ts` 和 `zh.ts` 的所有 key，报告只在一侧存在的 key
- 可作为 `cargo fmt` 检查周期的一部分，或作为 CI 中的独立 lint 步骤

---

## 6. `setSessionModelOverride` 返回值确认

**优先级**: 中 | **建议归入**: Plan 4（协议补齐部分）

**问题描述**: Plan 2 设计 `session.model.override` 接口时，期望返回完整的 `SessionRuntimeSnapshot`。但后端可能只返回简单的 `{ success: true }`。文档中已增加了 fallback 说明（操作成功后额外调用 `getSessionRuntime()` 刷新），但需要在 Plan 4 的协议设计中与后端明确约定返回值格式。

**影响**:

- 如果后端能返回完整 snapshot，前端一次请求即可完成更新（更优）
- 如果后端只返回操作结果，前端需要两次请求（override + getRuntime），会有短暂的数据不一致窗口

---

## 关联文档

- [总览文档](./2026-04-26-deskapp-agent-observability-and-control.md)
- [Plan 1: 运行态控制台与信息架构](./2026-04-26-deskapp-agent-observability-and-control-plan-1.md)
- [Plan 2: LLM 切换与 Token 统计](./2026-04-26-deskapp-agent-observability-and-control-plan-2.md)
- [Plan 3: Tool / Skill / Memory / Prompt 可观测面](./2026-04-26-deskapp-agent-observability-and-control-plan-3.md)
