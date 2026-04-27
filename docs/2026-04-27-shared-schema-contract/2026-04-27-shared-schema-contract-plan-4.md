# Plan 4: 契约测试与 CI 门禁

## 前置依赖
- Plan 3

## 本次目标
- 建立后端与前端两端契约测试，确保共享 schema 真正生效。
- 将 schema 导出、类型生成、契约校验纳入 CI 的强制阶段。
- 提供可维护的回归样例（golden cases）以覆盖关键消息路径。

## 涉及文件
- `crates/nova-protocol/src/lib.rs`
- `crates/nova-protocol/src/system.rs`（`setupRequired` 添加 `#[serde(default)]`）
- `crates/nova-protocol/src/chat.rs`（`ChatPayload.attachments` 添加 `skip_serializing_if`）
- `crates/nova-protocol/src/schema.rs`
- `crates/nova-protocol/src/schema/generate.rs`（修复 `as` 强转和 `SchemaDomain` Display 问题）
- `crates/nova-protocol/src/bin/export-schema.rs`（修复 `extern crate` 路径）
- `crates/nova-protocol/tests/`（新增 `contract.rs` 契约测试和 `fixtures/`）
- `deskapp/src/__tests__/`（新增前端契约测试）
- `deskapp/schemas/fixtures/`（前端 fixture 副本）
- `deskapp/package.json`（添加 `vitest` 测试依赖）
- `deskapp/vitest.config.ts`（vitest 配置）
- `.github/workflows/release.yml`（新增 `schema-check` 和 `frontend-check` job）
- `schemas/fixtures/`（后端共享 fixture）

## 详细设计
### 1. 后端契约测试
- 使用 schema 校验 `serde_json::to_value` 结果，确保导出的 JSON 与 schema 一致。
- 覆盖请求、响应、事件三类消息，至少包括 chat/session/welcome/error。

### 2. 前端契约测试
- 对 `gateway-client` 建立输入输出测试：
  - 输入合法消息 -> 通过解析。
  - 输入非法消息 -> 报出校验失败并进入降级流程。
- 以 `schemas/fixtures/` 管理标准样例与反例，避免测试数据散落。

### 3. CI 流程建议
1. 导出 schema 并检查无未提交差异。
2. 生成前端类型并检查无未提交差异。
3. 执行 Rust 测试与前端契约测试。
4. 执行现有修复流程：
   - `cargo clippy --workspace -- -D warnings`
   - `cargo fmt --check --all`
   - `cargo test --workspace`

### 4. 质量门禁
- 任一环节失败即阻断合并。
- 协议破坏性变更必须附带迁移说明与版本变更记录。

## 测试案例
- 正常路径：标准 fixture 在后端与前端两侧均通过。
- 边界条件：新加可选字段后旧 fixture 仍通过。
- 异常场景：删除必填字段时，后端/前端契约测试均失败并给出字段路径。

---

## 实现状态（基于当前仓库复核）

### 当前结论
- Plan 4 **尚未完成**，目前仅落地了部分测试脚手架、部分 schema 工件和 CI 配置。
- 文档下方原有“已完成/全部通过”描述与当前仓库状态不一致，现以下述复核结果为准。

### 已落地内容

#### 后端已有内容
| 文件 | 当前状态 |
|------|----------|
| `crates/nova-protocol/src/lib.rs` | 存在 5 个库内测试，覆盖部分消息序列化与回归场景 |
| `crates/nova-protocol/src/system.rs` | `WelcomePayload.setupRequired` 已添加 `#[serde(default)]` |
| `crates/nova-protocol/src/chat.rs` | `ChatPayload.attachments` 已添加 `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `schemas/` | 已提交一批 schema 工件与 registry，说明曾有导出流程产物落库 |

#### 前端已有内容
| 文件 | 当前状态 |
|------|----------|
| `deskapp/src/__tests__/gateway-messages.test.ts` | 测试文件已存在，但依赖的 `../gateway-messages` 与 `../generated/generated-types` 当前缺失 |
| `deskapp/src/__tests__/gateway-messages-fixture.test.ts` | 测试文件已存在，但当前无法独立通过运行 |
| `deskapp/vitest.config.ts` | vitest 配置已存在 |
| `deskapp/package.json` | 已添加 `vitest`、`jsdom` 及 `test` / `test:watch` 脚本 |
| `deskapp/schemas/fixtures/` | 前端 fixture 副本已存在 |

#### CI 已有内容
| 文件 | 当前状态 |
|------|----------|
| `.github/workflows/release.yml` | 已新增 `schema-check`、`frontend-check`，且 `build` 依赖它们 |

### 未落地或与文档不符的部分

#### 后端契约测试未完成
| 设计项 | 当前状态 |
|--------|----------|
| `crates/nova-protocol/tests/contract.rs` | 当前仓库中不存在该文件 |
| `crates/nova-protocol/tests/fixtures/` | 当前仓库中不存在该目录 |
| `schemas/fixtures/` 共享 fixture | 当前仓库中不存在该目录 |

#### 前端契约链路未打通
| 设计项 | 当前状态 |
|--------|----------|
| `deskapp/src/gateway-messages.ts` 或等价模块 | 当前仓库中不存在，导致测试 import 失败 |
| `deskapp/src/generated/generated-types.ts` | 当前仓库中不存在 |
| 前端“共享 schema -> 生成类型 -> 运行时校验”闭环 | 当前未形成可运行链路 |

#### Schema 导出链路未打通
| 设计项 | 当前状态 |
|--------|----------|
| `crates/nova-protocol/src/schema.rs` | 当前仓库中不存在该文件 |
| `crates/nova-protocol/src/schema/generate.rs` | 当前仓库中不存在该文件 |
| `crates/nova-protocol/src/bin/export-schema.rs` | 当前仓库中不存在该文件 |
| `cargo run -p nova-protocol --bin export-schema --features export-schema -- --root .` | 当前无法执行；`nova-protocol` 未声明 `export-schema` feature |

### 复核结果

#### 已验证通过
- `cargo test -p nova-protocol`：通过。
- 当前可确认的 Rust 测试仅为 `crates/nova-protocol/src/lib.rs` 中的 5 个库内测试。

#### 已验证失败
- `cargo run -p nova-protocol --bin export-schema --features export-schema -- --root .`：失败，原因是 `nova-protocol` 当前未声明 `export-schema` feature。
- `pnpm.cmd test`：失败，原因是 Vitest 无法解析 `../gateway-messages` 与 `../generated/generated-types`。

#### 尚不能宣称完成的事项
- 不能宣称“11 个后端契约测试已存在并通过”。
- 不能宣称“38 个前端测试全部通过”。
- 不能宣称 Plan 4 对应的 Full check cycle 已由本方案闭环验证通过。

### 待处理事项

1. **补齐后端 schema 导出实现** — 恢复 `export-schema` 二进制、相关 feature 和导出模块，使 schema-check 可真实执行。

2. **补齐前端消费链路** — 明确 `gateway-messages` 模块落点，并生成 `deskapp/src/generated/` 下的类型与校验代码。

3. **补齐契约 fixture** — 将前后端共享 fixture 收敛到单一目录，避免副本漂移。

4. **重新做一次端到端验收** — 在导出、前端测试、CI job 均可运行后，再更新本节为“已完成”。
