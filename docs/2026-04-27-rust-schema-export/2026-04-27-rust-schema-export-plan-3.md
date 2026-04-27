# Plan 3：前端消费链路改造

## 前置依赖
- Plan 2

## 本次目标
- 让前端直接消费 Rust 导出的 JSON Schema，而不是继续维护手写的类型与 validator 模板。
- 将“类型生成”和“运行时校验”分层，既保证 IDE/编译期体验，也保证 socket 数据在运行时能被约束。
- 修复当前 `agent.inspect` / `workspace.restore` 一类消息发送时的契约漂移问题。

## 涉及文件
- `deskapp/package.json`
- `deskapp/scripts/generate-schemas.js`
- `deskapp/src/generated/generated-types.ts`
- `deskapp/src/gateway-client.ts`
- `deskapp/src/core/types.ts`
- `deskapp/src/**` 中构造 `agent.inspect`、`workspace.restore` 请求的调用点

## 详细设计

### 1. 前端生成目标
- 生成结果拆分为两层：
  - `generated/schema-types.ts`：由 JSON Schema 生成的 TypeScript 类型声明。
  - `generated/schema-validators.ts`：基于 JSON Schema 编译得到的运行时校验封装。
- `generated/generated-types.ts` 不再手写 validator，而是退化为轻量聚合层，统一导出：
  - 类型别名
  - 校验器调用函数
  - 少量前端特有的 normalize helper（不属于协议契约本身）

### 2. `generate:schemas` 新流程
- 脚本执行顺序建议如下：
  1. 校验 `../schemas/registry.json` 是否存在；
  2. 读取 `frontend = true` 的 schema 列表；
  3. 生成 TypeScript 类型文件；
  4. 生成 AJV validator 封装；
  5. 生成入口 barrel 文件；
  6. 若输出与现有文件不同则覆写。
- 脚本应保持幂等；连续运行不应产生无关 diff。

### 3. 消息发送/接收分层
- 入站：
  - `parseInboundMessage` 先做 JSON parse；
  - 再用 `GatewayMessage` schema 校验完整 envelope；
  - 通过后再根据 `type` 分发到 normalize 逻辑。
- 出站：
  - 所有 `sendRequest(type, payload)` 路径在发送前按消息类型查找对应 request schema validator；
  - 校验失败时在前端本地直接拒绝发送，并给出明确日志或开发期告警。
- 对 `message-only` / `payload-required` 这种前端自定义 envelope 模式，迁移后不再由手写枚举控制，而改为由 schema 本身决定某类型是否需要 payload。

### 4. `core/types.ts` 角色调整
- 现有手写协议 DTO 逐步迁移为从 `generated/schema-types.ts` re-export。
- 只保留以下前端本地视图模型：
  - normalize 后的 UI event 视图
  - 非协议层辅助枚举
  - 与协议解耦的展示态结构
- 目标是让 `GatewayClient` 构造请求时直接依赖生成后的协议类型，例如：
  - `AgentInspectRequest`
  - `WorkspaceRestoreRequest`

### 5. 当前已知问题的直接修复策略
- `workspace.restore`：前端构造请求时必须发送 `payload: {}` 或 `payload: { userId: ... }`。
- `agent.inspect`：前端构造请求时必须传 `payload: { sessionId, agentId }`；当前的 `payload: { runtime: true }` 应被移除或迁移到新的独立消息类型中。
- 若 UI 真实需求只是“查看某 session 的 runtime”，应改为发送 `session.runtime`，而不是复用 `agent.inspect`。

### 6. 依赖建议
- 推荐方案：
  - 运行时校验：`ajv`
  - 类型生成：`json-schema-to-typescript`
- 取舍说明：
  - 该组合直接消费 JSON Schema，最贴合“Rust 导出 -> 前端使用”的链路；
  - 相比继续手写 validator，可显著降低协议维护成本；
  - 相比 `zod`，无需二次维护 schema 源。

## 测试案例
- 正常路径：前端可根据导出 schema 成功生成类型和 validator；`agent.inspect`、`workspace.restore` 发送前校验通过。
- 边界条件：可选字段、省略字段、附加字段存在时，校验行为与 Rust `serde` 保持一致。
- 异常场景：缺少 `payload`、缺少 `sessionId` / `agentId`、字段类型错误时，本地发送前校验失败。
- 回归场景：旧 fixtures 仍可通过解析；新增 observability fixtures 也能通过完整入站校验。

