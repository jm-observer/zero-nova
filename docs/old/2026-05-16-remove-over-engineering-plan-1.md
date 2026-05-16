# Plan 1: 消除 LlmClient 泛型感染

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `AgentRuntime<C: LlmClient>` 改为 `AgentRuntime`（内部持有 `Box<dyn LlmClient>`），消除泛型在 ConversationService 和 AgentApplicationImpl 上的传播。

**Architecture:** AgentRuntime 内部存储 `Box<dyn LlmClient>` 而非泛型参数 C。由于 LLM 调用是网络 IO，动态分发开销可忽略。所有下游类型（ConversationService, AgentApplicationImpl）不再需要泛型参数。

**Tech Stack:** Rust, async_trait, tokio

---

## 前置依赖

无

## 涉及文件

- `crates/nova-agent/src/provider/mod.rs` — 确保 LlmClient trait 是 object-safe
- `crates/nova-agent/src/agent/runtime.rs` — AgentRuntime 去泛型
- `crates/nova-agent/src/agent/runtime/tool_exec.rs` — impl 块去泛型
- `crates/nova-agent/src/agent/runtime/diagnostics.rs` — impl 块去泛型
- `crates/nova-agent/src/agent/mod.rs` — re-export 不变
- `crates/nova-agent/src/app/conversation_service.rs` — ConversationService 去泛型
- `crates/nova-agent/src/app/application.rs` — AgentApplicationImpl 去泛型
- `crates/nova-agent/src/app/mod.rs` — 移除 LlmClient re-export
- `crates/nova-agent/src/lib.rs` — 更新 re-export
- `crates/nova-agent-loader/src/bootstrap.rs` — BuiltAgentRuntime 去泛型，build_application 简化
- `crates/nova-agent-loader/src/lib.rs` — 更新 re-export
- `crates/nova-cli/src/main.rs` — 函数签名去泛型
- `crates/nova-agent/src/tool/builtin/agent.rs` — SubagentRuntimeBuilder 返回类型去泛型
- `crates/nova-agent/tests/integration/` — 测试中 mock client 改为 Box::new(...)

## 详细设计

### 核心变更：AgentRuntime

```rust
// Before:
pub struct AgentRuntime<C: LlmClient> {
    client: C,
    ...
}

// After:
pub struct AgentRuntime {
    client: Box<dyn LlmClient>,
    ...
}
```

构造函数改为接受 `impl LlmClient + 'static`：

```rust
impl AgentRuntime {
    pub fn new(client: impl LlmClient + 'static, tools: ToolRegistry, config: AgentConfig) -> Self {
        Self {
            client: Box::new(client),
            tools,
            config,
            ...
        }
    }
}
```

### 下游类型去泛型

```rust
// Before:
pub struct ConversationService<C: LlmClient> {
    pub agent: AgentRuntime<C>,
    ...
}

// After:
pub struct ConversationService {
    pub agent: AgentRuntime,
    ...
}
```

```rust
// Before:
pub struct AgentApplicationImpl<C: LlmClient> { ... }
impl<C: LlmClient + 'static> AgentApplication for AgentApplicationImpl<C> { ... }

// After:
pub struct AgentApplicationImpl { ... }
impl AgentApplication for AgentApplicationImpl { ... }
```

### nova-cli 去泛型

```rust
// Before:
async fn run_repl(agent: &mut AgentRuntime<impl LlmClient>, ...) -> Result<()>

// After:
async fn run_repl(agent: &mut AgentRuntime, ...) -> Result<()>
```

### nova-agent-loader 去泛型

```rust
// Before:
pub struct BuiltAgentRuntime {
    pub runtime: AgentRuntime<OpenAiCompatClient>,
    ...
}

// After:
pub struct BuiltAgentRuntime {
    pub runtime: AgentRuntime,
    ...
}
```

### 测试 mock 适配

测试中的 mock client 不需要改变实现，只需要在构造 AgentRuntime 时传入即可（`AgentRuntime::new(NoopClient, ...)` 会自动 box）。

## 测试案例

- `cargo clippy --workspace -- -D warnings` 通过
- `cargo test --workspace` 全部通过
- 所有集成测试中的 mock client 正常工作
