# zero-nova 集成 Plan 1 — Agent 运行时增强

## 文档说明

- 设计文档: docs/design/2026-04-25-nova-integration-design.md
- 影响项目: zero-nova (独立), zero (依赖)
- Phase: Phase 0 (前置条件), Phase 3 (可选增强)

---

## 一、改动范围

### 1.1 影响文件清单

| 文件 | 改动类型 | 说明 |
|------|---------|------|
| Cargo.toml | 修改 | Rust 2024 Edition 升级 |
| crates/nova-core/Cargo.toml | 修改 | reqwest 0.13 升级 |
| crates/nova-core/src/agent.rs | 修改 | 接口兼容调整 |
| crates/nova-core/src/provider/mod.rs | 修改 | LlmClient trait 开放 |
| crates/nova-core/src/event.rs | 修改 | AgentEvent 类型补充 |
| crates/nova-app/Cargo.toml | 修改 | 依赖 zero-nova crates |
| crates/nova-app/src/application.rs | 修改 | AgentApplication 接口 |
| crates/nova-conversation/Cargo.toml | 修改 | 会话管理增强 |

### 1.2 不改动文件

以下文件不改动，保持向后兼容：

- crates/nova-protocol/ - 协议层完整
- crates/nova-gateway-core/ - 网关核心
- crates/channel-*/ - 通道系统
- deskapp/ - Tauri 桌面应用

---

## 二、实施步骤

### Step 1: Rust 2024 Edition 升级

**文件**: Cargo.toml, crates/*/Cargo.toml



**校验命令**:


### Step 2: reqwest 0.13 升级

**文件**: Cargo.toml, crates/nova-core/Cargo.toml



**校验命令**:


### Step 3: 验证编译


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 1 test
test test_can_use_tool_callback ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 1 test
test test_basic_repl_interaction has been running for over 60 seconds
test test_basic_repl_interaction ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 382.33s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 4 tests
test gateway_health_endpoint_responds_ok ... ok
test gateway_accepts_message_and_emits_event ... ok
test gateway_rejects_invalid_attachment_kind ... ok
test gateway_shutdown_stops_server ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.56s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 2 tests
test session::tests::test_clear_all_sessions ... ok
test session::tests::test_update_sessions_empty ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

---
