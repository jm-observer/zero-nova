# 2026-04-27 Rust 导出 Schema 并接入前端 详细设计

## 时间
- 创建日期：2026-04-27
- 最后更新：2026-04-27

## 项目现状
- 协议 Rust 类型集中定义在 `crates/nova-protocol`，运行时 WebSocket 入站解析直接依赖 `serde` 反序列化。
- `schemas/` 目录当前是静态工件集合，`crates/nova-protocol/src/schema.rs` 只负责校验文件是否存在、同步 fixture 和生成 snapshot，并不从 Rust DTO 自动导出。
- 前端 `deskapp/scripts/generate-schemas.js` 当前只是把模板复制到 `deskapp/src/generated/generated-types.ts`，并未消费 Rust 协议定义，也未消费 `schemas/` 目录中的 JSON Schema。
- 前端消息校验逻辑主要依赖 `deskapp/src/generated/generated-types.ts` 中的手写 validator；测试覆盖也围绕这套手写逻辑展开。
- 当前已出现协议漂移：运行时支持了 `agent.inspect`、`workspace.restore` 等消息，但 `schemas/domains/gateway/message-envelope.schema.json` 未覆盖，前端发送的请求结构也与 Rust DTO 不一致。

## 整体目标
- 以 `crates/nova-protocol` 的 Rust DTO 作为协议单一事实来源（Single Source of Truth）。
- 从 Rust 自动导出 JSON Schema，覆盖 Gateway 入站/出站消息及相关 payload 定义。
- 前端直接消费导出的 Schema，生成类型与运行时校验逻辑，替换当前手写模板式生成方案。
- 建立后端、前端、Schema 工件三者联动的测试与 CI 门禁，防止协议再度漂移。

## Plan 拆分
- Plan 1：Rust 协议建模与 Schema 导出能力重建
  - 描述：为协议 DTO 建立可导出的 schema 元数据，重写 `export-schema` 产物生成链路。
  - 依赖：无。
- Plan 2：Schema 工件布局与版本化约束
  - 描述：统一导出目录、清单文件、fixture 组织方式，以及新增消息的纳入规则。
  - 依赖：Plan 1。
- Plan 3：前端消费链路改造
  - 描述：前端从导出的 JSON Schema 生成类型与校验器，替换手写 validator/模板。
  - 依赖：Plan 2。
- Plan 4：契约测试与 CI 门禁
  - 描述：补全 Rust、前端、工件一致性测试，并纳入现有发布检查流程。
  - 依赖：Plan 3。

执行顺序：Plan 1 -> Plan 2 -> Plan 3 -> Plan 4。

## 风险与待定项
- 风险 1：`MessageEnvelope` 使用 tagged enum + `payload` 包装，导出时需要保证 schema 与 `serde(tag, content)` 语义完全一致，否则前端生成的校验器会与运行时反序列化继续偏离。
- 风险 2：协议中存在 `serde_json::Value` 一类“开放结构”字段，导出 schema 后需要明确是保留宽松对象，还是补充更细粒度子类型，否则前端生成类型价值有限。
- 风险 3：前端若同时生成“类型 + 运行时校验器”，构建链路会更长，需要控制生成结果可缓存、可提交、可审查。
- 风险 4：一次性全量替换现有手写 validator 会影响较多测试；建议按“先接入主消息通道，再补边缘消息”的顺序渐进收敛。
- 待定项 1：Rust 侧 schema 导出建议采用 `schemars`；该 crate 已出现在 lockfile 中，但尚未作为 workspace 明确依赖声明，实施时需要显式纳入 `workspace.dependencies`。
- 待定项 2：前端运行时校验建议采用 `ajv`，类型生成建议优先评估 `json-schema-to-typescript`；如果不希望新增两条依赖链，也可评估仅保留运行时校验、类型通过现有 wrapper 暴露。
- 待定项 3：本次建议先覆盖 `GatewayMessage` / `MessageEnvelope` 及 observability、session、chat 相关请求响应；对 `serde_json::Value` 的 config 类动态 payload 暂保留宽松 schema，不在首批强类型收敛范围内。

