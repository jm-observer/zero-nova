# Plan 1: Provider 心跳模型与协议扩展

## 前置依赖
无

## 本次目标
1. 为 Provider 心跳定义统一的状态模型、快照结构和增量事件。
2. 扩展共享协议，让 DeskApp 可消费 Provider 状态与 `project_dir`。
3. 明确 schema 导出和前端类型更新路径，避免 Rust/TS 漂移。

## 涉及文件
- `crates/nova-protocol/src/observability.rs`
- `crates/nova-protocol/src/envelope.rs`
- `crates/nova-protocol/src/schema.rs`
- `crates/nova-protocol/src/lib.rs`
- `schemas/fixtures/*`（新增协议样例）
- `deskapp/src/generated/schema-types.ts`
- `deskapp/src/generated/schema-validators.ts`
- `deskapp/src/core/types.ts`

## 详细设计

### 1. Provider 健康结构
新增 `ProviderHealthSnapshot`：

- `provider: String`
- `scope: String`
- `status: String`
- `checked_at: i64`
- `latency_ms: Option<u64>`
- `message: Option<String>`

新增 `ProviderHealthSnapshotResponse`：

- `providers: Vec<ProviderHealthSnapshot>`
- `updated_at: i64`

新增 `ProviderHealthRequest`：

- 可为空请求，后端返回当前缓存

### 2. 新消息类型
在 `MessageEnvelope` 中增加：

- `ProviderHealth`
- `ProviderHealthResponse`
- `ProviderHealthUpdated`

命名建议与现有 observability 风格对齐：

- 请求：`provider.health`
- 响应：`provider.health.response`
- 推送：`provider.health.updated`

### 3. 扩展 `SessionRuntimeSnapshot`
为 `SessionRuntimeSnapshot` 增加：

- `project_dir: Option<String>`

可选增强字段：

- `project_dir_source: Option<String>`

本期只把 `project_dir` 设为必需改动项，`source` 作为后续可选增强，避免影响面扩大。

### 4. Schema 与前端类型同步
协议字段以 Rust 为 source of truth：

1. 修改 `nova-protocol` 结构体
2. 更新 schema fixture
3. 执行 `cargo test` 触发 schema 生成
4. 让 DeskApp 直接消费生成后的类型

前端手写类型如 `SessionRuntimeSnapshot` 也必须同步补齐 `projectDir?: string`，并尽量向生成类型靠拢。

## 测试案例
1. 正常路径：`provider.health.response` 返回多个 provider 状态，schema 校验通过。
2. 正常路径：`provider.health.updated` 仅携带状态快照和时间戳，前端可反序列化。
3. 正常路径：`session.runtime.response` 包含 `projectDir`，旧字段不受影响。
4. 兼容路径：旧 `welcome`、`session.runtime.updated` fixture 之外的消息仍保持 schema 通过。
5. 边界路径：`latencyMs`、`message`、`projectDir` 为空时仍能反序列化。
