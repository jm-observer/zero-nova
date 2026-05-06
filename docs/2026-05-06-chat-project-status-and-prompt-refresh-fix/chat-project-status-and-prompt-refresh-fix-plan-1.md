# Plan 1: 状态模型收敛与事件语义拆分

| 章节 | 说明 |
|------|------|
| Plan 编号与标题 | Plan 1: 状态模型收敛与事件语义拆分 |
| 前置依赖 | 无 |
| 本次目标 | 建立连接态、运行态、Project 状态、Prompt 版本状态的清晰边界，消除事件复用导致的展示歧义。 |

## 涉及文件

1. `deskapp/src/core/event-bus.ts`
2. `deskapp/src/main.ts`
3. `deskapp/src/ui/titlebar.ts`
4. `deskapp/src/ui/chat-view.ts`
5. `deskapp/src/ui/agent-console-view.ts`
6. `deskapp/src/core/types.ts`（必要时补充前端状态类型）

## 详细设计

### 1. `gateway:status` 语义拆分

现状：`Events.GATEWAY_STATUS` 同时承载连接态与运行文案，调用方可能误用。

修复：

1. 保留 `Events.GATEWAY_STATUS` 仅用于连接态，payload 统一为 `{ connectionStatus }`。
2. 新增独立事件（如 `Events.RUNTIME_STATUS_TEXT`）承载“Agent Running (x/y)”等临时文案。
3. `ChatView` 迭代进度不再写入连接态事件，改发运行文案事件。
4. `TitleBarView` 内部明确两套状态：
   - 连接态：只读 `connectionStatus`
   - 运行文案：只读 `runtimeStatusText`（可选展示）

### 2. Project 状态单一来源

现状：`sessionProjectDirState` 与 runtime state 并存，存在双写竞态。

修复：

1. 定义 runtime state 为 `projectDir` 的唯一事实来源。
2. `sessionProjectDirState` 改为仅用于“渲染快照缓存”，并以 runtime 更新时间或请求序号做防回退保护。
3. 所有更新入口（session 切换、runtime updated、手动刷新）统一走同一更新函数，禁止分散 set/map。

### 3. Prompt 状态模型补强

现状：重载后只做“再拉一次”，缺少版本一致性判断。

修复：

1. 为 prompt 面板维护 `expectedPromptVersion`（由 reload 响应返回）与 `displayedPromptVersion`。
2. 拉取预览后比较版本：
   - 相等：标记“已对齐”
   - 不等：进入“待对齐/重试中”状态并触发补偿刷新
3. 将上述状态抽象为 UI 可展示字段，避免用户无感。

### 4. 诊断日志边界

1. 连接态、Project 刷新、Prompt 重载均记录轻量 debug 日志（仅版本/路径摘要，不打印完整 prompt）。
2. 日志只在关键状态切换处打点，避免噪音。

## 测试案例

1. 连接态事件与运行态事件并发触发时，标题栏连接状态不被覆盖。
2. 切换 session 后，Project 读取只走统一入口，状态不回退。
3. Prompt 重载返回新版本后，前端状态先进入“待对齐”，最终进入“已对齐”。
4. 压测场景：短时间重复触发 runtime.updated，最终显示最后一次版本与路径。

