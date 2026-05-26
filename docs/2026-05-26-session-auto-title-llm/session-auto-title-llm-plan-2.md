# Plan 2: `LlmTitleGenerator` 实现

## 前置依赖

Plan 1（trait、错误、注入点已就绪）

## 任务目标

完成后可验证：

- `crates/nova-agent/src/conversation/title_generator.rs` 增加 `LlmTitleGenerator`，根据 `session_id` 解析当前 active agent 的 binding，调用 `OpenAiCompatClient` 获取一行 ≤40 字符标题。
- 即使 LLM 输出多行、含 markdown / 引号 / emoji，`normalize_generated_title` 之后能拿到稳定单行。
- 网络错误、500/429、解析失败映射到 `Retryable`；空响应、内容为空字符串映射到 `NonRetryable`。
- 全量修复循环通过。

## 执行范围

**必须修改**：

- `crates/nova-agent/src/conversation/title_generator.rs`（增 `LlmTitleGenerator` 结构、构造、`impl TitleGenerator`、私有 helper）
- `crates/nova-agent/src/conversation/mod.rs`（导出 `LlmTitleGenerator`）

**允许修改**：

- `crates/nova-agent/src/conversation/service/tests.rs`（追加 LlmTitleGenerator 的单元 smoke 测试，使用 `wiremock` 同 `tests/integration/context_headers.rs` 套路）

**禁止修改**：

- `OpenAiCompatClient` 本体
- `SessionService` 的 setter 形态
- title 调度链路

## Agent 执行步骤

1. 在 `title_generator.rs` 顶部 use 引入 `crate::config::AppConfig`、`crate::conversation::SessionService`、`crate::provider::openai_compat::OpenAiCompatClient`、`crate::provider::{LlmClient, ModelConfig, ProviderStreamEvent}`、`crate::message::{Message, Role, ContentBlock}`、`crate::network::build_provider_client`。
2. 定义：
   ```rust
   pub struct LlmTitleGenerator {
       config: Arc<AppConfig>,
       sessions: Arc<SessionService>,
   }
   ```
   与 `pub fn new(config: Arc<AppConfig>, sessions: Arc<SessionService>) -> Self`。
3. `impl TitleGenerator for LlmTitleGenerator`：
   1. `let session = self.sessions.get(session_id).await.map_err(NonRetryable)?.ok_or_else(|| NonRetryable(anyhow!("session not found: {session_id}")))?;`
   2. `let agent_id = session.get_active_agent().await;`
   3. `let binding = self.config.resolve_agent_binding_by_id(&agent_id).map_err(NonRetryable)?;`
   4. `let http_client = build_provider_client().map_err(Retryable)?;`
   5. `let client = OpenAiCompatClient::from_registry_with_http_client(self.config.providers.clone(), binding.provider_id.clone(), http_client);`
   6. 构造 `model_config: ModelConfig = binding.model_config.clone().into();`，**覆盖** `max_tokens = 80`，`temperature = Some(0.2)`，`thinking_budget = None`，`reasoning_effort = None`（title 任务不需要思考预算）。
   7. 构造消息：
      - system: `"你是一个会话标题生成器。读用户消息后输出一行不超过 20 个汉字（或 40 字符）的中文短摘要，仅输出标题文本本身，禁止使用引号、Markdown、emoji、句末标点。"`
      - user: `format!("用户消息：\n{}\n\n请只输出一行短标题。", user_texts.join("\n---\n"))`
   8. 准备 `ProviderRequestContext`：用一次性 `request_id = format!("title-gen-{}", session_id)`，其余字段保持空/默认。
   9. `let mut stream = client.stream(&messages, &[], &model_config, &request_context).await.map_err(classify_error)?;`
   10. 循环 `stream.next_event()`，累加 `TextDelta`；遇 `MessageComplete` 退出；遇错误 `map_err(classify_error)`。
   11. 累加结果 `trim` 后若为空 → `Err(NonRetryable(anyhow!("empty title")))`；否则返回 `Ok(buffer)`（normalize 由调用方完成）。
4. `classify_error` helper：根据 `anyhow::Error` 的 `to_string()` 关键字（`timeout`/`connection`/`5xx`/`429`）映射到 `Retryable`，否则 `NonRetryable`。保守起见，未知错误默认 `Retryable`（不卡死状态机）。
5. `tests.rs` 增加 `wiremock` 驱动的 smoke：
   - 启 mock server，stub `/chat/completions` 返回 SSE 单 chunk `data: {"choices":[{"delta":{"content":"路由设计"}}]}\n\ndata: [DONE]\n\n`。
   - 构造一个最小 `AppConfig`，往 `providers` 注入一条指向 mock_server.uri() 的 provider 配置。
   - 构造 SessionService、创建 session、`LlmTitleGenerator::new(config, sessions).generate(session_id, &user_texts)` 断言返回 `Ok("路由设计")`。

## 目标接口契约

```rust
pub struct LlmTitleGenerator { /* 见上 */ }

impl LlmTitleGenerator {
    pub fn new(config: Arc<AppConfig>, sessions: Arc<SessionService>) -> Self { ... }
}

#[async_trait]
impl TitleGenerator for LlmTitleGenerator {
    async fn generate(
        &self,
        session_id: &str,
        user_texts: &[String],
    ) -> Result<String, TitleGenerationError>;
}

// 私有 helper
fn classify_error(err: anyhow::Error) -> TitleGenerationError;
```

## 行为规则

| 场景 | 处理路径 | 期望输出 |
|------|----------|----------|
| 正常路径：mock 返回 `路由设计` | stream 累加 → trim 非空 | `Ok("路由设计")`（由调用方 normalize） |
| LLM 返回多行 `路由设计\n再补一句` | stream 累加 | `Ok("路由设计\n再补一句")`，normalize 取首行截 40 |
| mock 网络错误 / 5xx | `client.stream` 报错 | `Err(Retryable)` |
| mock 返回 `{"choices":[{"delta":{"content":""}}]}` 且 MessageComplete | trim 后为空 | `Err(NonRetryable("empty title"))` |
| session 不存在 | `sessions.get` 返回 None | `Err(NonRetryable("session not found ..."))` |
| binding 解析失败（agent_id 在 config 中找不到） | `resolve_agent_binding_by_id` Err | `Err(NonRetryable(原 err))` |

## 禁止事项

- 禁止在本 Plan 改触发点或调度门槛。
- 禁止把 LLM client 缓存为字段（每次调用即时构造，否则 config 热更新场景失效）。
- 禁止把 prompt 文案散落到多处；集中在 `LlmTitleGenerator` 私有常量。
- 禁止透传 tools / skill / agent_prompt 等重量级上下文——title 任务只需 user_texts。
- 禁止把 LlmTitleGenerator 的 system prompt 做成英文（与会话主语境对齐：中文）。

## 测试要求

文件：`crates/nova-agent/src/conversation/service/tests.rs`（或独立 `crates/nova-agent/tests/integration/title_generator.rs`，根据现有 wiremock 测试落点习惯选定）

新增测试：

1. `fn llm_title_generator_returns_streamed_title`：mock SSE 返回 `路由设计`，断言 `Ok("路由设计")`。
2. `fn llm_title_generator_maps_5xx_to_retryable`：mock 返回 500，断言 `Err(Retryable)`。
3. `fn llm_title_generator_maps_empty_response_to_non_retryable`：mock 返回空 delta 后 `[DONE]`，断言 `Err(NonRetryable)`。
4. `fn llm_title_generator_returns_non_retryable_when_session_missing`：直接调用未注册 session_id，断言 `Err(NonRetryable)`。

验证命令：

```bash
cargo test -p nova-agent llm_title_generator
```

## 完成条件

- [ ] `LlmTitleGenerator` 已实现并 export
- [ ] 4 个新测试通过
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过
