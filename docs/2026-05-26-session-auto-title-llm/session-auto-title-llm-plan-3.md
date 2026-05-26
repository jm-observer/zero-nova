# Plan 3: `ConversationService` 装配并注入 `LlmTitleGenerator`

## 前置依赖

Plan 1（注入点）+ Plan 2（LlmTitleGenerator）

## 任务目标

完成后可验证：

- `ConversationService::new` 构造 `LlmTitleGenerator` 并通过 `SessionService::set_title_generator` 注入；不改造 `SessionService::new` 签名。
- `SessionService` 内部 `Arc<dyn TitleGenerator>` 字段在装配后即生效——下一次用户消息触发调度时就走 LLM 路径。
- 所有现有 `ConversationService::new` 调用点（lib bootstrap、tests）无破坏性改动。

## 执行范围

**必须修改**：

- `crates/nova-agent/src/app/conversation_service.rs`
  - `ConversationService::new` 在 `sessions` 被纳入 `self` 之前/之后，构造 `LlmTitleGenerator`，调用 `sessions.set_title_generator(Arc::new(generator))`。
  - 因 `SessionService` 不再是不可变值（要 `&mut` 调 setter），评估两种实现：
    - **方案 A（推荐）**：`new` 内先把 `sessions` 当本地 `mut` 用、注入后再 move 进 `self`。
    - **方案 B**：给 `SessionService` 加一个 `with_title_generator(self, ...) -> Self` 链式 API。
  - 选 A，简单不破坏既有调用。
- `crates/nova-agent/src/conversation/service/mod.rs`：保留 `set_title_generator` 的 `&mut self` 签名；Plan 1 已就绪。

**允许修改**：

- 旧测试中直接 `ConversationService::new(...)` 的代码——若未走 LLM 路径不需调整；若走，需 mock SessionService 的 title_generator 替换（用 Plan 1 的 `set_title_generator` 后注入）。

**禁止修改**：

- `AgentApplicationImpl::new` 签名
- `start_turn` 等 turn 主链路
- `LlmTitleGenerator` 行为（在 Plan 2 定型）

## Agent 执行步骤

1. 修改 `crates/nova-agent/src/app/conversation_service.rs::ConversationService::new`：
   ```rust
   pub fn new(
       agent: AgentRuntime,
       agent_registry: AgentRegistry,
       mut sessions: SessionService,
       config: Arc<AppConfig>,
       turn_prompt_service: TurnPromptService,
   ) -> Self {
       let sessions_arc = Arc::new(sessions.clone()); // 给 LlmTitleGenerator 引用回查
       let title_generator = Arc::new(crate::conversation::LlmTitleGenerator::new(
           config.clone(),
           sessions_arc.clone(),
       ));
       sessions.set_title_generator(title_generator);
       Self {
           agent,
           agent_registry,
           sessions,
           config: config.clone(),
           turn_prompt_service,
           prompt_providers: crate::prompt_provider::PromptProviderRegistry::new(),
       }
   }
   ```
   > 注：`SessionService::clone()` 复制 `Arc<SessionCache>` 等内部引用，sessions_arc 与 `self.sessions` 共享同一份缓存与仓库；title_generator 通过 sessions_arc 回查 active agent 是安全的（指向同一数据源）。
2. 验证 `SessionService` `#[derive(Clone)]` 已存在（Plan 1 必须保留）。
3. 检查全工程 `ConversationService::new` 调用点：用 ripgrep 搜 `ConversationService::new`，确认所有 caller 仍能编过（new 签名未变）。
4. 跑修复循环。

## 行为规则

| 场景 | 处理路径 | 期望输出 |
|------|----------|----------|
| `nova-server-ws` bootstrap 启动 | ConversationService::new 注入 LlmTitleGenerator | 后续 session 用户消息触发的 title 调度走 LLM 路径 |
| 现有单测构造 SessionService 但不构造 ConversationService | 默认仍是 FallbackTitleGenerator | 不破坏既有断言 |
| 测试需要 mock LLM | 测试代码自行调 `set_title_generator(Arc::new(MockTitleGenerator))` 覆盖 | 不绕回 LLM 路径 |

## 禁止事项

- 禁止在 `AgentApplicationImpl` 层级注入（远离 SessionService 的层，违反"依赖反转就近装配"）。
- 禁止让 `LlmTitleGenerator` 在构造时即解析 binding（必须每次按 session 解析）。
- 禁止把 `Arc<SessionService>` 暴露为 `pub` 字段——通过 `LlmTitleGenerator::new` 的入参传入即可。

## 测试要求

文件：`crates/nova-agent/src/app/conversation_service.rs` 内部 `mod tests`（已有 helper 套路）或 `crates/nova-agent/tests/integration/title_generator_wiring.rs`。

新增测试：

1. `fn conversation_service_new_installs_llm_title_generator`：构造最小 ConversationService（mock AgentRuntime + mock provider），创建 session，发两条用户消息，断言 `session.get_name()` 返回 mock LLM 的输出（不再是 user_texts 拼接）。
2. `fn conversation_service_does_not_call_llm_below_threshold`：发一条用户消息（< 24 char），断言 LLM 调用次数为 0，session name 仍为空。

验证命令：

```bash
cargo test -p nova-agent conversation_service
```

## 完成条件

- [ ] `ConversationService::new` 注入 `LlmTitleGenerator` 完成
- [ ] 所有 `ConversationService::new` caller 仍能编译
- [ ] 新增 2 个测试通过
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过
