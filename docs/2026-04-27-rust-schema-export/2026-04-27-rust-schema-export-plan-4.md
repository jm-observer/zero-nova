# Plan 4：契约测试与 CI 门禁

## 前置依赖
- Plan 3

## 本次目标
- 建立 Rust 导出、前端消费、fixture 一致性的测试闭环。
- 把 schema 导出和前端生成纳入自动检查，避免只改一端造成静默漂移。
- 将当前“手写 validator 通过、真实协议却失败”的问题转成可自动发现的回归测试。

## 涉及文件
- `.github/workflows/release.yml`
- `crates/nova-protocol/src/lib.rs`
- `crates/nova-protocol/src/schema.rs`
- `deskapp/src/__tests__/gateway-messages.test.ts`
- `deskapp/src/__tests__/gateway-messages-fixture.test.ts`
- `deskapp/scripts/generate-schemas.js`
- 可能新增的 Rust integration tests / 前端 contract tests 文件

## 详细设计

### 1. Rust 侧测试
- 新增 schema 导出测试，验证：
  - 根类型可导出；
  - 导出文件名稳定；
  - `GatewayMessage` schema 包含 `agent.inspect`、`workspace.restore` 等新消息分支；
  - fixture 可被对应 Rust DTO 成功反序列化。
- 新增反向约束测试：读取关键 schema 文件，断言其中 required 字段与 Rust 类型语义一致。

### 2. 前端测试
- 保留现有 fixture test，但底层从“手写 validateEnvelope / validateOutboundMessage”切换到“生成 validator”。
- 新增两类测试：
  - outbound contract test：对 `agent.inspect`、`workspace.restore` 构造正确和错误请求，验证发送前校验结果；
  - schema drift test：检测生成文件是否与当前仓库提交一致。
- 对 `GatewayClient` 的集成测试增加断言：
  - 错误 payload 不会进入真实 `ws.send`
  - 正确 payload 序列化后保留 `payload` 外层包装

### 3. CI 门禁
- Root Rust 检查前增加：
  - `cargo run -p nova-protocol --bin export-schema --features export-schema -- --root .`
- Frontend 检查保持：
  - `pnpm generate:schemas`
  - 生成后 `git diff --exit-code`
- 推荐顺序：
  1. 导出 Rust schema
  2. 检查 `schemas/` 无脏 diff
  3. 生成前端类型与 validator
  4. 检查 `deskapp/src/generated/` 无脏 diff
  5. 跑 Rust / 前端测试

### 4. 回归场景固化
- 将这次暴露的问题固化为显式测试：
  - `workspace.restore` 没有 `payload` 时失败；
  - `agent.inspect` 缺少 `sessionId` 时失败；
  - `agent.inspect` 缺少 `agentId` 时失败；
  - 正确的 `workspace.restore` / `agent.inspect` 请求样例均通过 Rust 和前端双端校验。

## 测试案例
- 正常路径：完整导出、生成、测试链路一次通过。
- 边界条件：仅新增可选字段时，前后端生成与测试仍稳定通过。
- 异常场景：修改 Rust DTO 后未重新导出 schema，CI 失败；修改 schema 后未重新生成前端产物，CI 失败。
- 回归场景：本次两类协议漂移错误都有对应 fixture 与自动化断言。

