# ADR: Title 生成器依赖反转 + LLM 接入

- 日期：2026-05-26
- 状态：已实施
- 关联：[docs/2026-05-26-session-auto-title-llm/](../2026-05-26-session-auto-title-llm/session-auto-title-llm.md)

## 背景

`SessionService::run_title_generation` 长期持有一个占位实现：把所有用户文本 `join(" ")` 后交给 `normalize_generated_title` 截前 40 字符——观感等同于「复读用户消息」，给宿主（zero）和前端的感觉是 title 功能没生效。Plan 3 of 2026-05-09 已设计 trait + LLM 接入，但状态停留在「待开始」未推进。

## 决策

### D-1：用 `TitleGenerator` trait 做依赖反转

`SessionService` 持有 `Arc<dyn TitleGenerator>`，默认装 `FallbackTitleGenerator`；宿主通过 `set_title_generator` 注入 LLM 实现。

**为什么不直接在 `SessionService` 持有 `Arc<dyn LlmClient>` + `ModelConfig`：**
- 把 `provider::*` 依赖灌进 `conversation/repository/` 这一层，破坏 nova-agent 的内部分层（conversation = 会话状态与持久化；provider = 模型适配）。
- 测试每次都要 mock LlmClient，门槛比 mock 一个 trait 高。
- `provider` 升级（如换 streaming 协议）会牵连 SessionService 的构造调用方。

**为什么不在 `AgentApplicationImpl` 层装配：**
- 远离 `SessionService` 自身的 `title_generator` 字段，注入路径要穿过 ConversationService，链条变长。
- 单测「装配后立即触发」需要把 AgentApplicationImpl 整个搭起来。

### D-2：复用当前 session active agent 的 binding

每次 title 生成时按 `session_id` 反查 `session.get_active_agent().await`，用 `AppConfig::resolve_agent_binding_by_id(&agent_id)` 拿 binding。

**备选 — 引入 `[title_generator]` 专用 binding 配置项：**
- 优点：可以指定小模型（haiku/gpt-4o-mini），更经济，避免和会话主链路抢配额。
- 缺点：扩配置、需要默认值、需要文档；本次范围之外。

**用户偏好[memory feedback_subagent_isolation_preference]：** 已记录"弱模型多能力倾向独立子 Agent 隔离"——未来如果出现 title 与主 turn 抢配额/限流问题，按此偏好升级为独立 binding。

### D-3：保留 `FallbackTitleGenerator`

默认装 fallback（取首条用户文本单行），不是 `Option<Arc<dyn TitleGenerator>>`。

**理由：**
- 单元测试无需启 wiremock 就能验证调度链路。
- bootstrap 早期路径（构造 SessionService 后、ConversationService 装配前）不会触发空生成器导致面板。
- 类型上无 None，调用方少一层 unwrap。

### D-4：超时从 3s 调到 15s

原 `TITLE_GENERATION_TIMEOUT_MS = 3_000` 在 LLM 调用场景下几乎一定触发 timeout retryable。改为 15_000，给 stream 充足完成时间，仍远低于用户感知的"卡住"阈值。

### D-5：错误分类策略 — 未识别错误一律 Retryable

`classify_error` 按字符串关键字（`unauthorized` / `invalid api key` / `forbidden` / `not found` / `404`）认定为 `NonRetryable`，其余一律 `Retryable`。

**理由：** 状态机有 `attempt_count` 兜底（最多 2 次），偏 Retryable 不会无限重试；偏 NonRetryable 会"一次失败就放弃"，更糟糕。

## 影响

- `crates/nova-agent/src/conversation/`：新增 `title_generator.rs` 模块；`SessionService` 字段 + setter；`service/title.rs` 调 trait；超时常量改 15s。
- `crates/nova-agent/src/app/conversation_service.rs`：`ConversationService::new` 装配 `LlmTitleGenerator` 并注入。
- `crates/nova-agent/tests/`：新增 `session_auto_title_e2e.rs`（wiremock + SSE）覆盖正常与 5xx 路径。

## 后续

- 若 title 调用与主 turn 抢配额 → 升级为 D-2 备选（独立 binding 配置项）。
- 若 prompt 措辞需要按用户语言自适应 → 在 `LlmTitleGenerator` 增加 system prompt 模板路径，从 config 读取。
- 若需要按 token 上限收敛 user_texts 拼接 → 在 `LlmTitleGenerator::generate` 内做尾部截断。
