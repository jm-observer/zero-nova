# Plan 1: `TitleGenerator` trait + 注入点 + `FallbackTitleGenerator`

## 前置依赖

无。

## 任务目标

完成后可验证：

- `crates/nova-agent/src/conversation/title_generator.rs` 提供 `TitleGenerator` trait + `TitleGenerationError` 枚举 + `FallbackTitleGenerator` 实现。
- `SessionService` 持有 `Arc<dyn TitleGenerator>` 字段；构造函数默认装 `FallbackTitleGenerator`，新增 `set_title_generator(&mut self, ...)` setter 供外部覆盖。
- `service/title.rs::generate_title` 删除占位静态方法，改调注入的 generator，错误按 `TitleGenerationError::{Retryable, NonRetryable}` 映射为 `retryable:` / `non_retryable:` 前缀字符串（与 `set_failed` 协议一致）。
- `service/mod.rs::TITLE_GENERATION_TIMEOUT_MS` 从 `3_000` 改为 `15_000`，单一来源不散落。
- 全量修复循环通过。

## 执行范围

**必须修改**：

- `crates/nova-agent/src/conversation/title_generator.rs`（新增）
- `crates/nova-agent/src/conversation/mod.rs`（增 `pub mod title_generator;` 并导出符号）
- `crates/nova-agent/src/conversation/service/mod.rs`（`SessionService` 字段、`new` 默认装配、`set_title_generator` setter、`TITLE_GENERATION_TIMEOUT_MS = 15_000`）
- `crates/nova-agent/src/conversation/service/title.rs`（删 `generate_title` 静态方法；`run_title_generation` 改调 `self.title_generator.generate(&user_texts)`，错误映射 `TitleGenerationError` 到 `Failed` 状态机的 `retryable:` / `non_retryable:` 前缀字符串）
- `crates/nova-agent/src/conversation/service/tests.rs`（mock generator，下个 Plan 也会用，先在本 Plan 加一个 `MockTitleGenerator` 辅助类型）

**允许修改**：

- `crates/nova-agent/src/lib.rs`（如需 re-export `TitleGenerator` 给外部用户）

**禁止修改**：

- `service/write.rs`（触发链路保持不变）
- `service/persist.rs`、`control.rs`、`session.rs`（状态机和持久化路径不变）
- 现有调度门槛常量（除 `TITLE_GENERATION_TIMEOUT_MS`）

## Agent 执行步骤

1. 新增 `crates/nova-agent/src/conversation/title_generator.rs`：
   - 定义 `pub enum TitleGenerationError { Retryable(anyhow::Error), NonRetryable(anyhow::Error) }`，`impl Display + std::error::Error`。
   - 定义 `#[async_trait::async_trait] pub trait TitleGenerator: Send + Sync { async fn generate(&self, session_id: &str, user_texts: &[String]) -> Result<String, TitleGenerationError>; }`。`session_id` 入参是为 Plan 2 解析 active agent binding 留接口（本 Plan 的 fallback 实现忽略它）。
   - 定义 `pub struct FallbackTitleGenerator;`，`impl TitleGenerator`，逻辑：取 `user_texts.first()`，截前 40 chars，单行清理；空时 `Err(NonRetryable)`。
2. 在 `crates/nova-agent/src/conversation/mod.rs` 增 `pub mod title_generator;` 并 `pub use title_generator::{TitleGenerator, TitleGenerationError, FallbackTitleGenerator};`。
3. 修改 `service/mod.rs`：
   - `pub const TITLE_GENERATION_TIMEOUT_MS: u64 = 15_000;`
   - `SessionService` 新增字段 `title_generator: Arc<dyn TitleGenerator>`。
   - `SessionService::new(cache, repository)` 默认装 `Arc::new(FallbackTitleGenerator)`。
   - 新增 `pub fn set_title_generator(&mut self, generator: Arc<dyn TitleGenerator>)`（注意 SessionService 实现 Clone 时本字段也克隆 Arc，验证语义保留）。
4. 修改 `service/title.rs`：
   - 删除 `async fn generate_title(user_texts: &[String]) -> Result<String>`。
   - `run_title_generation` 中 `timeout(...).await` 包裹 `self.title_generator.generate(&session.id, &user_texts)`。
   - 错误映射：
     - `Ok(Ok(title))` → 走 normalize 流程（不变）
     - `Ok(Err(TitleGenerationError::Retryable(e)))` → `set_failed(format!("retryable: {e}"))`
     - `Ok(Err(TitleGenerationError::NonRetryable(e)))` → `set_failed(format!("non_retryable: {e}"))`
     - `Err(_) /* timeout */` → `set_failed(format!("retryable: timeout after {TITLE_GENERATION_TIMEOUT_MS}ms"))`
5. 修改 `service/tests.rs`：
   - 增加 `MockTitleGenerator` helper（用 `Mutex<Vec<Result<String, TitleGenerationError>>>` 控制每次返回值，按调用次序消费）。
   - 现有依赖默认 stub 输出的断言改为：构造 SessionService 后显式 `set_title_generator(Arc::new(FallbackTitleGenerator))`，断言取首条用户消息前 40 字符。

## 目标接口契约

```rust
// crates/nova-agent/src/conversation/title_generator.rs
use anyhow::Error as AnyError;
use async_trait::async_trait;

#[derive(Debug)]
pub enum TitleGenerationError {
    Retryable(AnyError),
    NonRetryable(AnyError),
}

impl std::fmt::Display for TitleGenerationError { /* ... */ }
impl std::error::Error for TitleGenerationError { /* ... */ }

#[async_trait]
pub trait TitleGenerator: Send + Sync {
    async fn generate(
        &self,
        session_id: &str,
        user_texts: &[String],
    ) -> Result<String, TitleGenerationError>;
}

pub struct FallbackTitleGenerator;

#[async_trait]
impl TitleGenerator for FallbackTitleGenerator {
    async fn generate(
        &self,
        _session_id: &str,
        user_texts: &[String],
    ) -> Result<String, TitleGenerationError> { /* 首条消息单行前 40 char；空则 NonRetryable */ }
}
```

`SessionService` 改动：

```rust
#[derive(Clone)]
pub struct SessionService {
    cache: Arc<SessionCache>,
    repository: SqliteSessionRepository,
    loading: Arc<RwLock<LoadingWaiters>>,
    title_generator: Arc<dyn TitleGenerator>,  // 新增
}

impl SessionService {
    pub fn new(cache: Arc<SessionCache>, repository: SqliteSessionRepository) -> Self { /* 默认 Fallback */ }
    pub fn set_title_generator(&mut self, generator: Arc<dyn TitleGenerator>) { /* */ }
}
```

## 行为规则

| 输入 | 处理路径 | 期望输出 |
|------|----------|----------|
| `FallbackTitleGenerator.generate(_, ["你好，帮我看一下 router 这块怎么写"])` | 截前 40 char + 单行清理 | `Ok("你好，帮我看一下 router 这块怎么写")`（≤40 char） |
| `FallbackTitleGenerator.generate(_, [])` | 空输入 | `Err(NonRetryable)` |
| `FallbackTitleGenerator.generate(_, ["   \n  "])` | trim 后为空 | `Err(NonRetryable)` |
| Mock 返回 `Err(Retryable)` 触发 title 调度 | `run_title_generation` 走错误映射 | `set_failed("retryable: ...")`, `attempt_count += 1`，下一次用户消息可重试 |
| Mock 返回 `Err(NonRetryable)` | 同上 | `set_failed("non_retryable: ...")`, `attempt_count += 1`，不可重试 |
| timeout 超过 15s | timeout 分支 | `set_failed("retryable: timeout after 15000ms")` |

## 禁止事项

- 禁止在本 Plan 内引入 LLM client（留给 Plan 2）。
- 禁止改触发链路 / 调度门槛 / `TitleSource` 枚举。
- 禁止把 `TitleGenerator` 写成同步 trait（流式 LLM 调用要异步）。
- 禁止把 `title_generator` 字段做成 `Option`——默认装 `FallbackTitleGenerator`，类型上无 None。

## 测试要求

文件：`crates/nova-agent/src/conversation/service/tests.rs`（沿用现有测试模块）

新增测试：

1. `fn fallback_title_generator_uses_first_user_message`：直接构造 `FallbackTitleGenerator`，断言返回首条消息前 40 char。
2. `fn fallback_title_generator_rejects_empty_input`：空输入 → `NonRetryable`。
3. `fn title_generation_uses_injected_generator`：构造 SessionService，`set_title_generator(MockTitleGenerator::with(Ok("自定义".into())))`，模拟两条用户消息触发，断言 `session.get_name().await == "自定义"`。
4. `fn title_generation_retryable_error_keeps_failed_state`：MockGenerator 返回 `Err(Retryable)`，断言 `title_state.status == Failed`，`attempt_count == 1`，下一条用户消息时仍能重试。
5. `fn title_generation_non_retryable_error_consumes_attempt`：返回 `Err(NonRetryable)`，断言 `attempt_count == 1`。

验证命令：

```bash
cargo test -p nova-agent conversation::service::title
cargo test -p nova-agent conversation::title_generator
```

## 完成条件

- [ ] `title_generator.rs` 已创建，trait/error/Fallback 实现完毕
- [ ] `SessionService.title_generator` 字段 + `set_title_generator` setter 完成
- [ ] `service/title.rs::run_title_generation` 已切换到 trait 调用
- [ ] `TITLE_GENERATION_TIMEOUT_MS = 15_000`
- [ ] 5 个新测试全部通过
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过
