# Plan 1：Rust 协议建模与 Schema 导出能力重建

## 前置依赖
- 无

## 本次目标
- 为 `crates/nova-protocol` 中参与协议传输的核心 DTO 增加可自动导出的 schema 能力。
- 将 `export-schema` 从“校验静态文件存在”改造为“从 Rust 类型生成 schema 工件”。
- 让 `GatewayMessage` / `MessageEnvelope` 成为前后端共同消费的真实根协议定义。

## 涉及文件
- `Cargo.toml`
- `crates/nova-protocol/Cargo.toml`
- `crates/nova-protocol/src/envelope.rs`
- `crates/nova-protocol/src/chat.rs`
- `crates/nova-protocol/src/session.rs`
- `crates/nova-protocol/src/agent.rs`
- `crates/nova-protocol/src/observability.rs`
- `crates/nova-protocol/src/system.rs`
- `crates/nova-protocol/src/config.rs`
- `crates/nova-protocol/src/schema.rs`
- `crates/nova-protocol/src/bin/export-schema.rs`

## 详细设计

### 1. DTO 导出原则
- 所有需要进入 WebSocket 协议边界的结构体、枚举、payload 都应同时具备：`Serialize`、`Deserialize`、`JsonSchema`。
- 仅在确实需要动态结构时才继续使用 `serde_json::Value`；同时在 schema 注释中明确该字段“开放对象”语义，避免前端误以为可强校验。
- 命名延续现有 `serde(rename_all = "camelCase")` 与 `serde(rename = "...")`，禁止在 schema 导出层做二次重命名，确保导出结果与运行时行为一致。

### 2. `GatewayMessage` / `MessageEnvelope` 根协议导出
- `GatewayMessage` 继续保持：
  - `id?: string`
  - `type: string`
  - `payload: object | primitive | omitted(仅 unit variant)`
- `MessageEnvelope` 保留现有 `#[serde(tag = "type", content = "payload")]` 语义。
- 导出时以 `GatewayMessage` 为根 schema，同时单独输出 `MessageEnvelope` schema，方便：
  - 后端测试直接验证完整消息；
  - 前端在“原始 socket 消息解析”和“只校验 payload”两类场景分别使用。

### 3. `export-schema` 重构
- 新增 schema 导出主流程：
  1. 收集根类型列表；
  2. 调用统一导出函数生成 `RootSchema`；
  3. 以稳定命名写入 `schemas/domains/**`；
  4. 生成 registry 与 snapshot；
  5. 同步 fixtures；
  6. 对导出结果执行最小自检（根类型均可序列化成合法 JSON）。
- 根类型至少包括：
  - `GatewayMessage`
  - `MessageEnvelope`
  - `ChatPayload`、`ChatCompletePayload`、`ProgressEvent`
  - `SessionCreateRequest`、`SessionCreateResponse`、`SessionIdPayload`
  - `AgentInspectRequest`、`WorkspaceRestoreRequest` 及相关 observability 响应 DTO
- `schema.rs` 中不再手工维护 `REQUIRED_SCHEMA_FILES` 常量清单，而是改为维护“导出根类型清单”；工件文件名由导出逻辑统一决定。

### 4. 文件命名与根类型映射
- 采用“类型语义名 + kebab-case + `.schema.json`”规则，例如：
  - `gateway-message.schema.json`
  - `message-envelope.schema.json`
  - `agent-inspect-request.schema.json`
  - `workspace-restore-request.schema.json`
- 文件落盘路径由域决定：
  - gateway 根消息放 `schemas/domains/gateway/`
  - session 放 `schemas/domains/session/`
  - observability 放 `schemas/domains/observability/`
- 所有 schema 顶层写入统一元信息：`$schema`、`title`、`description(可选)`、`$defs`。

### 5. 兼容策略
- 首轮不改变运行时协议形状，只导出“当前真实协议”。
- 因此：
  - `workspace.restore` 仍要求外层存在 `payload`，即使其内部字段全可选；
  - `agent.inspect` 仍要求 `sessionId` 和 `agentId`；
  - 前端必须跟随后端契约，不在 schema 层做“宽松兼容”。
- 若后续决定支持“无 payload 的命令消息”，应作为单独协议演进任务处理，而不是在本次导出任务中顺手放宽。

## 测试案例
- 正常路径：`GatewayMessage`、`MessageEnvelope`、`AgentInspectRequest`、`WorkspaceRestoreRequest` 均能成功导出 schema。
- 边界条件：包含 `Option<T>`、unit variant、嵌套 enum、`Vec<T>`、`HashMap<String, T>`（若存在）时导出结果结构正确。
- 异常场景：新增协议类型未加入根导出清单时，导出测试应失败并提示缺少注册。
- 回归场景：`agent.inspect` schema 中必须要求 `sessionId` 与 `agentId`；`workspace.restore` schema 必须要求 envelope 级 `payload`。

