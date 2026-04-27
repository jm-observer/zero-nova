# Plan 1: Schema 单一来源定义

## 前置依赖
- 无

## 本次目标
- 明确哪些协议类型必须进入共享 Schema。
- 定义 Schema 命名、分层和版本规则，确保可持续演进。
- 固化“后端协议模型是唯一事实来源”的约束。

## 涉及文件
- `crates/nova-protocol/src/lib.rs`
- `crates/nova-protocol/src/envelope.rs`
- `crates/nova-protocol/src/agent.rs`
- `crates/nova-protocol/src/chat.rs`
- `crates/nova-protocol/src/session.rs`
- `crates/nova-protocol/src/observability.rs`
- `crates/nova-protocol/src/system.rs`
- `docs/2026-04-27-shared-schema-contract/2026-04-27-shared-schema-contract.md`

## 详细设计
### 1. 共享范围
- 第一阶段纳入网关通信核心类型：
  - `GatewayMessage`
  - `MessageEnvelope`
  - 高频 request/response payload（chat/session/agent 相关）
- 第二阶段纳入 observability 扩展类型，降低首批接入复杂度。

### 2. Schema 组织结构
- 采用“按域拆分 + 根索引”的结构：
  - `schemas/root/gateway-message.schema.json`
  - `schemas/domains/chat/*.schema.json`
  - `schemas/domains/session/*.schema.json`
  - `schemas/domains/agent/*.schema.json`
  - `schemas/domains/system/*.schema.json`
- 根 schema 通过 `$ref` 关联子 schema，避免重复定义。

### 3. 命名与兼容规则
- Schema `$id` 使用稳定路径，不绑定本地绝对目录。
- 字段命名严格保持 serde 序列化结果（camelCase）。
- 向后兼容原则：
  - 允许新增可选字段。
  - 禁止删除字段、修改字段类型、修改枚举值语义（除非升级 major 版本）。

### 4. 设计约束
- 不允许前端手工新增“仅前端存在”的协议字段。
- 协议字段变更必须先改 `nova-protocol`，再重新生成 schema 与前端类型。

## 测试案例
- 正常路径：`GatewayMessage::Welcome` 可导出并被 schema 校验通过。
- 边界条件：可选字段缺失时校验通过；可选字段类型错误时校验失败。
- 异常场景：枚举值非法（如未知 `type`）时校验失败并返回明确错误位置。
