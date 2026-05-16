# Plan 2: 移除 AgentApplication trait

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 移除 `AgentApplication` trait，gateway 层直接使用 `Arc<AgentApplicationImpl>`。

**Architecture:** Plan 1 完成后 `AgentApplicationImpl` 不再是泛型，因此不再需要 trait 做类型擦除。gateway 层可以直接引用具体类型。

**Tech Stack:** Rust, async_trait

---

## 前置依赖

Plan 1（AgentApplicationImpl 去泛型后才能移除 trait）

## 涉及文件

- `crates/nova-agent/src/app/application.rs` — 删除 trait 定义，保留 impl 块方法
- `crates/nova-agent/src/app/mod.rs` — 移除 AgentApplication re-export
- `crates/nova-agent-loader/src/bootstrap.rs` — 返回 `Arc<AgentApplicationImpl>` 而非 `Arc<dyn AgentApplication>`
- `crates/nova-gateway-core/src/lib.rs` — `Arc<dyn AgentApplication>` → `Arc<AgentApplicationImpl>`
- `crates/nova-gateway-core/src/router.rs` — `&dyn AgentApplication` → `&AgentApplicationImpl`
- `crates/nova-gateway-core/src/handlers/*.rs` — 所有 handler 参数类型替换
- `crates/nova-server/src/lib.rs` — 参数类型替换

## 详细设计

### 删除 trait，方法保留为 inherent impl

```rust
// Before:
#[async_trait]
pub trait AgentApplication: Send + Sync {
    async fn session_exists(&self, session_id: &str) -> Result<bool>;
    ...
}

#[async_trait]
impl AgentApplication for AgentApplicationImpl { ... }

// After:
impl AgentApplicationImpl {
    pub async fn session_exists(&self, session_id: &str) -> Result<bool> { ... }
    ...
}
```

### Gateway 层类型替换

```rust
// Before:
pub struct GatewayCore {
    app: Arc<dyn AgentApplication>,
}

// After:
pub struct GatewayCore {
    app: Arc<AgentApplicationImpl>,
}
```

Handler 函数签名批量替换 `&dyn AgentApplication` → `&AgentApplicationImpl`。

## 测试案例

- `cargo clippy --workspace -- -D warnings` 通过
- `cargo test --workspace` 全部通过
