# Plan 4: Gateway 协议补齐与测试方案

## 前置依赖

- Plan 2: LLM 切换与 Token 统计
- Plan 3: Tool / Skill / Memory / Prompt 可观测面

## 本次目标

梳理 `deskapp` 为支持新增控制台能力所需的协议补充、事件同步和测试覆盖方案。

## 涉及文件

- `deskapp/src/gateway-client.ts`
- `deskapp/src/core/types.ts`
- `crates/nova-gateway-core/*`
- `crates/nova-protocol/*`
- 相关 Agent runtime / session runtime 模块
- `deskapp/e2e/tests/*`

## 详细设计

### 1. 新增请求接口

建议新增以下请求：

- `agent.inspect`
- `session.runtime`
- `session.prompt.preview`
- `session.tools.list`
- `session.memory.hits`
- `session.model.override`

返回对象都应为稳定的 ViewModel，不直接暴露内部领域结构。

建议字段级职责如下：

- `agent.inspect`
  - 返回当前 Agent 维度静态+半静态数据
  - 包含：模型绑定、技能绑定、默认 system prompt 摘要、工具统计
- `session.runtime`
  - 返回当前会话维度运行数据
  - 包含：session override、token 累计、最后一次运行时间、最近 turnId
- `session.tools.list`
  - 返回当前会话最终工具装配结果
- `session.memory.hits`
  - 返回指定轮次或最近一轮的命中记忆
- `session.prompt.preview`
  - 返回当前轮实际发送给模型的 Prompt 分段视图
- `session.model.override`
  - 修改会话级模型覆盖并返回最新 runtime snapshot

### 2. 新增事件

- `session.runtime.updated`
- `session.token.usage`
- `session.tools.updated`
- `session.memory.hit`

事件要求：

- 带 `sessionId`
- 带时间戳
- 允许前端做幂等覆盖更新

建议 payload 设计：

- `session.runtime.updated`
  - 用于模型绑定、最近 turn、最近运行时间变化
- `session.token.usage`
  - 用于本轮与累计 token 更新
- `session.tools.updated`
  - 用于工具绑定变更，例如 MCP 断开或客户端工具重注册
- `session.memory.hit`
  - 用于一轮完成后更新命中详情

### 2.1 协议兼容策略

- 旧 Gateway 未实现某接口时，应返回明确错误码，如 `capability_not_supported`。
- 前端收到该错误码后，将对应面板标记为“当前版本暂不支持”。
- 避免使用模糊字符串错误导致前端只能靠文案匹配。

### 3. `gateway-client.ts` 扩展方式

- 保持现有 `request(type, payload)` 风格，不引入第二套 transport。
- 为新接口增加语义方法，如：
  - `getAgentInspect`
  - `getSessionRuntime`
  - `previewSessionPrompt`
  - `listSessionTools`
  - `getSessionMemoryHits`
  - `overrideSessionModel`
- 为新增事件增加 `onXxx` 订阅函数，避免业务层直接监听裸消息类型。

建议新增的方法列表：

```ts
getAgentInspect(agentId: string, sessionId?: string)
getSessionRuntime(sessionId: string)
previewSessionPrompt(sessionId: string, turnId?: string)
listSessionTools(sessionId: string)
getSessionMemoryHits(sessionId: string, turnId?: string)
overrideSessionModel(payload: SessionModelOverrideRequest)
onSessionRuntimeUpdated(handler)
onSessionTokenUsage(handler)
onSessionToolsUpdated(handler)
onSessionMemoryHit(handler)
```

### 4. 状态同步策略

- 首次打开控制台：走一次主动请求拉取快照。
- 会话运行中：依赖事件增量更新。
- 会话切换后：丢弃旧会话 runtime cache，避免错绑。

建议前端同步顺序：

1. 进入会话后先请求 `session.runtime`
2. 打开控制台后再按标签懒加载其余视图
3. 流式聊天中订阅事件更新
4. 回复完成后触发一次轻量刷新，兜底修正事件缺失问题

这样可以避免完全依赖事件推送的脆弱性。

### 5. 测试策略

前端至少补齐以下测试层次：

- 单元测试
  - `gateway-client` 对新增消息类型的解析
  - `AppState` 对 runtime snapshot 的状态更新
- 组件测试
  - Agent Console 的空态、加载态、刷新态
  - 模型切换作用域提示
  - Prompt 脱敏视图渲染
- E2E 测试
  - 打开控制台查看模型与工具
  - 切换会话级模型覆盖
  - 查看 memory hits 与 prompt preview

后端也应补充：

- 协议序列化测试
- runtime snapshot 组装测试
- session override 继承/恢复测试
- token usage 聚合测试
- prompt preview 脱敏测试

### 6. 灰度实施顺序

建议按以下顺序落地：

1. 只读视图先行：概览、tool/skill/memory/prompt 查看
2. token 统计接入
3. 会话级模型覆盖
4. 如有需要，再评估 Prompt 编辑或 Skill 在线开关

### 7. 具体实施步骤

1. 在 `nova-protocol` 中定义新增消息 type 和 payload 结构。
2. 在 Gateway 路由层增加对应 request handler 和 event broadcaster。
3. 在 Agent / Session runtime 层补齐聚合视图组装逻辑。
4. 在 `deskapp/src/gateway-client.ts` 封装新请求和事件订阅。
5. 在 `deskapp/src/core/state.ts` 接入 runtime cache 与事件归并逻辑。
6. 在 `chat-view` 中接入控制台 UI 和按需加载。
7. 先交付只读 inspection 面板。
8. 再接入 token usage。
9. 最后接入 session model override。

### 8. 里程碑建议

- Milestone A
  - 控制台框架
  - `agent.inspect`
  - `session.runtime`
- Milestone B
  - `session.tools.list`
  - `session.memory.hits`
  - `session.prompt.preview`
- Milestone C
  - `session.token.usage`
  - token UI
- Milestone D
  - `session.model.override`
  - 会话级模型切换

### 9. 验收标准

- 旧版本 Gateway 下，控制台能安全降级。
- 新协议字段具备稳定命名，不泄露内部领域模型。
- 前后端在多会话并发时不会串 session 状态。
- 关键路径覆盖单元测试、组件测试和 E2E。

## 测试案例

- 协议兼容：旧 Gateway 不支持新接口时，前端显示能力不可用，不导致页面崩溃。
- 事件乱序：先收到 token 更新，再收到 runtime 更新时，前端最终状态仍正确。
- 多会话隔离：两个会话并发运行时，token 和 memory hit 不串到错误会话。
- 脱敏安全：Prompt preview 返回 `redacted=true` 时，前端明确展示“已脱敏”标记。
- E2E 回归：新增控制台后，原有聊天、sessions、agents 基础流程不回归失败。
