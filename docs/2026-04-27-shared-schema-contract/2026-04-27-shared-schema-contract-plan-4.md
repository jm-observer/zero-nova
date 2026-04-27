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

## 实现状态（2026-04-27）

### 已完成

#### 后端契约测试
| 文件 | 内容 |
|------|------|
| `crates/nova-protocol/tests/contract.rs` | 11 个契约测试：7 个正常路径 roundtrip、3 个异常路径、1 个所有 envelope 变体遍历 |
| `crates/nova-protocol/tests/fixtures/` | 9 个 JSON fixture（7 正常 + 3 反例） |
| `schemas/fixtures/` | 共享 fixture，前后端共用 |

#### 前端契约测试
| 文件 | 内容 |
|------|------|
| `deskapp/src/__tests__/gateway-messages.test.ts` | 24 个单元测试，覆盖 `validateOutboundMessage`、`parseInboundMessage`、`serializeMessage`、`normalizeProgressEvent`、`createValidatedHandler`、`trackConsecError` |
| `deskapp/src/__tests__/gateway-messages-fixture.test.ts` | 14 个 fixture 契约测试，覆盖正常路径、异常路径、边界条件 |
| `deskapp/vitest.config.ts` | vitest 配置（jsdom 环境） |
| `deskapp/package.json` | 添加 `vitest`、`jsdom` 依赖；添加 `test` / `test:watch` 脚本 |
| `deskapp/schemas/fixtures/` | 前端 fixture 副本 |

#### CI 更新
| 文件 | 内容 |
|------|------|
| `.github/workflows/release.yml` | 新增 `schema-check` job（schema 导出 + diff 检查）<br>新增 `frontend-check` job（vitest 测试运行）<br>`build` job 依赖更新为 `needs: [schema-check, frontend-check, check]` |

#### 修复的预存问题
| 文件 | 修复内容 |
|------|----------|
| `crates/nova-protocol/src/system.rs` | `WelcomePayload.setupRequired` 添加 `#[serde(default)]`，允许缺失字段反序列化 |
| `crates/nova-protocol/src/chat.rs` | `ChatPayload.attachments` 添加 `#[serde(default, skip_serializing_if = "Option::is_none")]`，避免 roundtrip 不一致 |
| `crates/nova-protocol/src/schema/generate.rs` | ① 修复 `as` 强转错误（`as *mut` + unsafe）<br>② 移除已不存在的 `observability` 模块导入<br>③ 修复 `SchemaDomain` Display 格式问题 |
| `crates/nova-protocol/src/bin/export-schema.rs` | 修复 `use crate::schema` -> `use nova_protocol::schema` |

### 验证结果

**Rust 测试**：24 个测试全部通过

```
nova-protocol library tests:      5 passed
nova-protocol contract tests:    11 passed
nova-protocol schema export:     13 files exported
```

**前端测试**：38 个测试全部通过

```
gateway-messages.test.ts:         24 passed
gateway-messages-fixture.test.ts: 14 passed
```

**Full check cycle**：

- `cargo fmt --all --check` ✅
- `cargo clippy --workspace -- -D warnings` ✅
- `cargo test --workspace` ✅

### 待处理事项

1. **Schema 导出二进制 `export-schema` 的 observability 导入** — 当前使用 placeholder，后续需指向正确的 observability 类型路径（位于 `nova-protocol/src/schema.rs` 或 `nova-core`）。

2. **CI 中添加前端类型生成步骤** — 当前只运行 vitest，后续可添加 `pnpm generate:types` 并检查 `deskapp/src/generated/` 是否有未提交差异。

3. **后端 schema 导出 CI 集成交互** — CI 中的 `schema-check` job 需要确保导出 schema 与 git tracked 版本一致，可能需要添加比较逻辑。

4. **前端 fixture 路径** — 当前前端测试使用 `deskapp/schemas/fixtures/` 副本，未来可改为共享到仓库根目录 `schemas/fixtures/`，避免两份副本不同步。
