# Plan 3: 移除 TitleGenerator trait

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 删除 `TitleGenerator` trait 和 `TitleGenerationError` 枚举，将标题生成逻辑内联到 `SessionService`。

**Architecture:** 当前 `RuleBasedTitleGenerator` 只是拼接用户文本，逻辑极简。直接内联为 `SessionService` 的私有方法。测试中的 mock 改为直接注入一个 `fn` 或使用条件逻辑。

**Tech Stack:** Rust

---

## 前置依赖

无（与 Plan 1/2 独立）

## 涉及文件

- `crates/nova-agent/src/conversation/title_generator.rs` — 删除整个文件
- `crates/nova-agent/src/conversation/mod.rs` — 移除 `pub mod title_generator`
- `crates/nova-agent/src/conversation/service/mod.rs` — 移除 trait 字段，内联逻辑
- `crates/nova-agent/src/conversation/service/title.rs` — 简化，移除 TitleGenerationError 处理
- `crates/nova-agent/src/conversation/service/tests.rs` — 移除 MockTitleGenerator，简化测试

## 详细设计

### SessionService 变更

```rust
// Before:
pub struct SessionService {
    ...
    title_generator: Arc<dyn TitleGenerator + Send + Sync>,
    ...
}

// After:
pub struct SessionService {
    ...
    // title_generator 字段移除
    ...
}
```

### 标题生成内联

在 `title.rs` 中，将 `self.title_generator.generate_title(&user_texts)` 替换为直接拼接：

```rust
fn generate_title(user_texts: &[String]) -> Option<String> {
    let joined = user_texts.join(" ");
    if joined.trim().is_empty() {
        return None;
    }
    Some(joined)
}
```

`run_title_generation` 方法简化：移除 `TitleGenerationError` 的 Retryable/NonRetryable 分支，直接处理 Option。

### 测试简化

测试不再需要 MockTitleGenerator。如果需要控制标题生成行为，可以通过控制输入的 user_texts 来实现。

## 测试案例

- `cargo test -p nova-agent` 全部通过
- 标题生成的正常/空输入场景仍被覆盖
