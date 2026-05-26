# Session Auto Title — LLM 生成器接入

## 时间

- 创建日期：2026-05-26
- 最后更新：2026-05-26
- 更新说明：把 2026-05-09 Plan 3 未落地的「LLM 标题生成器抽象 + 注入」补完

## 项目现状

- `conversation/service/title.rs::generate_title` 仍是占位实现：把所有用户文本 `join(" ")` 后返回，由 `normalize_generated_title` 截前 40 字符当标题——观感等同于「复读用户消息」，给宿主（zero）和前端的感觉是「title 功能没生效」。
- 调度框架已完备：
  - 触发点：`conversation/service/write.rs::append_message` 用户消息入库后 `maybe_schedule_title_generation`
  - 门槛：`TITLE_MIN_USER_MESSAGES_FIRST_ATTEMPT=2 / SECOND_ATTEMPT=3`，`TITLE_MIN_TOTAL_CHARS=24`，`TITLE_MAX_ATTEMPTS=2`，`TITLE_GENERATION_TIMEOUT_MS=3000`
  - 状态机：`TitleStatus::Pending → Succeeded/Failed`，`retryable:` 前缀触发下一次
  - 推送：`AgentApplicationImpl::start_turn` 在 turn 完成前后比对 `before_title/after_title`，变化时发 `SessionSummaryUpdated`
- 2026-05-09 `session-auto-title-fix` Plan 3 设计稿（已移入 `docs/old/`）明确了 trait 注入 + LLM 实现方向，状态停留在「待开始」未推进。

## 整体目标

把 title 生成从拼接占位换成「真正调用 LLM 拿一行 ≤40 字符摘要」，落地三条约束：

1. **依赖反转**：title.rs 不直接持 LLM client，通过注入的 `TitleGenerator` trait 调用。
2. **复用会话 binding**：用户已确认走「当前 session active agent 的 binding」，不引入新配置。
3. **失败不阻塞主链路**：LLM 失败（超时/网络/格式）走可重试错误，落入现有失败状态机；空响应等不可重试。

附带：保留一个 `FallbackTitleGenerator`（取首条用户文本前 N 字符，单行清理）作为没注入时的默认值，避免单测和早期初始化路径失语。

## Plan 拆分

| Plan | 主题 | 依赖 | 顺序 | 状态 |
|------|------|------|------|------|
| Plan 1 | `TitleGenerator` trait + `TitleGenerationError` + `FallbackTitleGenerator` + `SessionService` 注入点 | 无 | 1 | 已完成 |
| Plan 2 | `LlmTitleGenerator` 实现（复用 active agent binding 调 OpenAiCompatClient） | Plan 1 | 2 | 已完成 |
| Plan 3 | `ConversationService::new` 装配 `LlmTitleGenerator` 并注入 `SessionService` | Plan 1, 2 | 3 | 已完成 |
| Plan 4 | 测试覆盖（mock generator + 超时 + 集成 smoke）+ 修复循环 | Plan 1, 2, 3 | 4 | 已完成 |
| Plan 5 | zero-nova 发新 nova tag；zero 那边 `cargo update -p nova-agent` 升级并跑自身修复循环 | Plan 4 | 5 | 进行中 |

## 风险与待定项

- **风险 A — LLM client 解析时机**：`ConversationService::new` 时 active agent binding 未确定（按 session 才能拿）。**收敛方案**：`LlmTitleGenerator` 持有 `Arc<AppConfig> + Arc<SessionService>`（用于反查 session active agent），不在构造时绑死单一 binding。每次调用按 session 解析 binding 后即时构造 `OpenAiCompatClient`。
- **风险 B — title 调用与主 turn 同 binding 抢配额/限流**：复用 active agent binding 是用户明确选择，承担此风险；后续若出现问题，按用户的「弱模型多能力倾向独立子 Agent 隔离」偏好升级为「title 专用 binding」配置项（不在本次范围）。
- **风险 C — 超时仍为 3000ms**：LLM 走出整链 prompt+网络通常 > 3s，会大量进入 retryable。**收敛方案**：把 `TITLE_GENERATION_TIMEOUT_MS` 提到 15_000ms（在 Plan 1 完成同时改），保留单一来源 `service/mod.rs` 常量。
- **待定 — prompt 措辞与语言**：默认走「中文短摘要」prompt；若未来需要按用户输入语言自适应，再开。

## 设计影响记录

实施完成后在 `docs/adr/` 下新增 `2026-05-26-title-generator-injection.md`，记录：

1. 为什么用依赖反转 trait 而不是 SessionService 直接持 LlmClient（避免把 provider 依赖灌进 conversation/repository 层）。
2. 为什么复用 active agent binding（用户确认）以及保留的升级路径（独立 binding 配置）。
3. 为什么保留 `FallbackTitleGenerator`（单测、bootstrap 早期路径、LLM 失败兜底）。
