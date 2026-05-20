# Plan 2: AgentTool 子 Session 路径改造

## 前置依赖

Plan 1（Session 父子模型 + 持久化）。本 Plan 假设 `Session.parent_session_id` / `parent_tool_use_id` / `child_session_ids` 与 `SqliteSessionRepository::save_session(..., parent_*)` / `list_child_session_ids` 已就绪。

## 任务目标

改造 `AgentTool::run_subagent`：派生子 Agent 时**为其创建一个独立的 `conversation::Session`**（带 `parent_session_id` + `parent_tool_use_id`），子 turn 在子 Session 上运行；子 turn 完成后把 `TurnResult.messages`（含 `ProviderHttpTrace` metadata）追加进**子 Session.history**（不是父 Session）；同时把子 session_id `push_child` 进父 Session。

完成后：

- 父 Session.history 里那对 `ToolUse{name:"Agent",...}` + `ToolResult{output:子最终回复}` 保持现状（语义就是"父调子的接口"）
- 子 Session 物理存在，独立持有完整 Vec<Message>（含子 turn 每次 LLM 调用的 raw HTTP req/resp）
- 父子关系通过 SQLite 持久化（重启可恢复树形结构）
- 嵌套：子 Agent 内部又派生孙 Agent 自然递归（孙 Session 的 parent 是子 Session）
- 取消传播：父 turn 取消时，已派生但未完成的子 Session turn 跟着取消（通过 `CancellationToken` 父子关联）
- **本 Plan 不暴露对外** `get_session_tree` API（Plan 3 做）

## 执行范围

**必须修改**：

| 文件 | 改动 |
|------|------|
| `crates/nova-agent/src/tool/builtin/agent.rs` | `AgentToolServices` 增 `conversation_writer: Arc<ConversationWriteHandle>` 字段；`run_subagent` 内部：创建子 Session → 运行子 turn → 持久化 turn_result.messages 到子 Session → 父 Session push_child |
| `crates/nova-agent/src/app/conversation_service.rs` | 暴露最小 `ConversationWriteHandle`（trait 或 newtype 包装），仅暴露子 Agent 路径需要的两件事：`create_child_session(parent_id, parent_tool_use_id, agent_id, title) -> Result<String>` 与 `persist_subagent_turn(child_session_id, turn_result)`；这两个方法内部调用既有 `SessionWriteService::create_session` / `append_message`，保持与父 turn 的持久化语义一致 |
| `crates/nova-agent/src/app/application.rs` | `AgentApplicationImpl::new` 构造 `AgentToolServices` 时填充 `conversation_writer` |
| `crates/nova-agent/src/tool/context.rs`（`ToolContext` 定义处） | 确认 `tool_use_id` 字段已存在（现状已存在），无需新增；本 Plan 仅消费该字段 |

**允许修改**：

- `AgentToolServices` 构造点的测试 helper（补 mock writer）
- 既有 `AgentTool::run_subagent` 单测：补"子 Session 创建路径"的覆盖

**禁止修改**：

- `Session` struct（Plan 1 已定）
- SQLite schema（Plan 1 已定）
- `AgentApplicationImpl` 公开签名（Plan 3）
- `run_turn_with_context` / `execute_turn_loop` 内部逻辑（本 Plan 仅在其外围加包装）
- `OrchestratorEngine` 与 `SubAgentExecutor` trait 签名（本 Plan 不改 trait，只改 `AgentTool` 这个 impl）
- 父 Session.history 写入的内容/格式（父侧 ToolUse + ToolResult 维持现状）

## Agent 执行步骤

### 步骤 1：定义 `ConversationWriteHandle`（`app/conversation_service.rs`）

```rust
/// 给 AgentTool 子 Agent 路径用的最小写入句柄。封装"创建子 Session + 持久化 turn 结果"两个原子操作，
/// 不暴露 ConversationService 全量 API（避免子 Agent 路径偶然触发 start_turn 等高层路径）。
#[derive(Clone)]
pub struct ConversationWriteHandle {
    sessions: SessionWriteService,
}

impl ConversationWriteHandle {
    pub(crate) fn new(sessions: SessionWriteService) -> Self {
        Self { sessions }
    }

    /// 创建子 Session 并落盘父子关系。
    /// `parent_tool_use_id` 必填（None 表示这不是子 Session 派生场景，应改走顶层 create_session 而非本方法）。
    pub async fn create_child_session(
        &self,
        parent_session_id: &str,
        parent_tool_use_id: &str,
        agent_id: &str,
        title: Option<String>,
    ) -> Result<String> {
        let child_id = self
            .sessions
            .create_with_parent(
                title.unwrap_or_else(|| format!("subagent-{}", parent_tool_use_id)),
                agent_id.to_string(),
                Some(parent_session_id.to_string()),
                Some(parent_tool_use_id.to_string()),
            )
            .await?;
        // 父 Session 内存侧 push_child（持久化关系靠子行的 parent_session_id 列；父侧 children 是 load 时回填）
        if let Some(parent) = self.sessions.try_get_loaded(parent_session_id).await {
            parent.push_child(&child_id).await;
        }
        Ok(child_id)
    }

    /// 把子 turn 的 messages（含 ProviderHttpTrace metadata）追加到子 Session.history 并落 SQLite。
    /// 语义与 ConversationService::start_turn 第 548-572 行的循环等价。
    pub async fn persist_subagent_turn(
        &self,
        child_session_id: &str,
        turn_result: &TurnResult,
    ) -> Result<()> {
        for msg in &turn_result.messages {
            let metadata = if msg.role == Role::Assistant {
                turn_result.provider_request_body.as_ref()
                    .zip(turn_result.provider_response_body.as_ref())
                    .map(|(req, resp)| serde_json::json!({
                        "providerHttpTrace": {
                            "requestBody": req,
                            "responseBody": resp,
                            "format": "json",
                            "boundMessageId": "",
                            "capturedAt": chrono::Utc::now().timestamp_millis(),
                            "truncated": false
                        }
                    }))
            } else { None };
            self.sessions
                .append_message(child_session_id, msg.role.clone(), msg.content.clone(), metadata)
                .await?;
        }
        Ok(())
    }
}
```

> 若 `SessionWriteService` 暂无 `create_with_parent` / `try_get_loaded`，本步骤含义为：在 `SessionWriteService` 上新增这两个方法（薄包装，前者调 `repo.save_session(..., Some(parent), Some(tool_use_id))` + 写 cache；后者从 cache 读出 `Option<Arc<Session>>`）。

### 步骤 2：扩 `AgentToolServices`（`tool/builtin/agent.rs`）

```rust
#[derive(Clone)]
pub struct AgentToolServices {
    pub prompt_service: SubagentPromptService,
    pub runtime_builder: SubagentRuntimeBuilder,
    pub conversation_writer: Arc<ConversationWriteHandle>,   // === Plan 2 新增
}
```

`AgentApplicationImpl::new`（`app/application.rs`）构造 `AgentToolServices` 处补 `conversation_writer: Arc::new(ConversationWriteHandle::new(sessions.clone()))`。

### 步骤 3：改 `AgentTool::run_subagent`（`tool/builtin/agent.rs:455` 起）

定位 `run_subagent` 末尾「准备运行 turn」段（agent.rs:660-696），将其重构为：

```rust
// === 现状（删除）===
//   let session_id = context.as_ref().map(|c| c.session_id.clone())
//       .unwrap_or_else(|| "subagent".to_string());
//   ...
//   let result = runtime.run_turn_with_context(turn_ctx, user_message, &session_id, agent_id, ...).await?;

// === 新（Plan 2）===
let writer = services.conversation_writer.clone();

// (a) 取 parent 上下文（见总览「已收敛的待澄清点」#2）
let (parent_session_id, parent_tool_use_id) = match context.as_ref() {
    None => {
        // 没有 ToolContext —— CLI / 独立调用场景，回退老语义。
        log::warn!("[Agent] run_subagent invoked without ToolContext; falling back to flat session");
        return self.run_subagent_flat_fallback(/* ... */).await;
    }
    Some(ctx) if ctx.tool_use_id.is_empty() => {
        // ToolContext 存在但 tool_use_id 空 —— registry execute 路径必然填 tool_use_id，
        // 空值一定是上游 bug，不能 silent fallback 掩盖。
        anyhow::bail!("[Agent] ToolContext present but tool_use_id is empty (upstream bug)");
    }
    Some(ctx) => (ctx.session_id.clone(), ctx.tool_use_id.clone()),
};

// (b) 创建子 Session（带父子关系），自动持久化
let child_session_id = writer
    .create_child_session(&parent_session_id, &parent_tool_use_id, agent_id, None)
    .await?;

// (c) 派生子 CancellationToken：父取消 → 子取消
let child_cancel = context.as_ref()
    .and_then(|ctx| ctx.cancellation_token.clone())
    .map(|parent_tok| parent_tok.child_token())
    .or_else(|| Some(CancellationToken::new()));

// (d) 跑子 turn（在子 session_id 上）
let turn_ctx = runtime
    .prepare_turn(prompt, Arc::new(Vec::new()), system_prompt, &child_session_id)
    .await?;
let user_message = Message::new(
    Role::User,
    vec![ContentBlock::Text { text: prompt.to_string() }],
    chrono::Utc::now().timestamp_millis(),
);
let result = runtime
    .run_turn_with_context(turn_ctx, user_message, &child_session_id, agent_id, Some(environment), tx, child_cancel)
    .await?;

// (e) 持久化子 turn 全部消息到子 Session.history（含 ProviderHttpTrace）
writer.persist_subagent_turn(&child_session_id, &result).await?;

// (f) preload tool 激活继续走原逻辑（注意 session_id 用 child_session_id）
// 上面 step (b) 之后的 preload_tools 循环 session_id 改为 child_session_id
```

> 关键不变量：父 Session.history 的写入路径完全不动——父 LLM 在派发 ToolUse 时由 `ConversationService::start_turn` 现有逻辑写父 history；子 ToolResult 由父 turn loop 在收到 `AgentTool` 返回的 `output` 字符串后写回父 history。本 Plan 只在父子 ToolUse/ToolResult 之间**新增**一个子 Session 的 history 写入侧线。
>
> **嵌套链路（见总览「已收敛的待澄清点」#3）**：当子 Agent 内部再次派生孙 Agent 时，孙 Session 的 `parent_tool_use_id` 来自子 turn 自己生成的 ToolUse id —— 这由 runtime 现有的 `ToolContext.tool_use_id` 在 turn loop 分发工具时自动填入，**本 Plan 不需要额外维护**。孙 Session 走与子 Session 完全相同的 `create_child_session` 路径，因此 N 层递归自然成立。
>
> **复盘 trace 粒度限制（见总览「项目现状」段）**：`TurnResult.provider_request_body` / `provider_response_body` 是 turn 末次 LLM 调用的单一快照。`persist_subagent_turn` 把同一份 trace 贴到子 turn 的每条 Assistant Message —— 与父 turn 现有行为完全一致，**不引入新的不完整性**。zero 侧消费时应明白：每个 Session 的 history 内多条 Assistant Message 的 `metadata.providerHttpTrace` 是同一份末次 trace 的副本，并非逐轮 trace。

### 步骤 4：preload tool 激活路径修正

agent.rs:667-674 处 `for tool_name in &preload_tools { ... runtime.tools().resolve_deferred(&session_id, tool_name).await ... }`：把 `session_id` 改为 `child_session_id`。preload 工具是在子 Session 范围内激活，与父 Session 无关。

### 步骤 5：CancellationToken 父子链

`ToolContext.cancellation_token: Option<CancellationToken>` 已存在；本 Plan 在子 turn 执行时调 `parent_tok.child_token()` 派生子 token，保证父被取消时子也取消。`tokio_util::sync::CancellationToken` 已提供 `child_token()`，无需自实现。

### 步骤 6：fallback 路径（CLI / 独立调用）

`run_subagent_flat_fallback`（步骤 3 中提到）保留原 agent.rs:661-696 的"用 `subagent` 顶层 session_id"行为——仅 CLI 直接 `nova subagent ...` 或脚本工具调用时走，生产路径不触发。文档化此回退是为了避免破坏 nova-cli 行为。

## 目标数据结构 / 接口契约

```rust
// app/conversation_service.rs
#[derive(Clone)]
pub struct ConversationWriteHandle {
    sessions: SessionWriteService,
}
impl ConversationWriteHandle {
    pub async fn create_child_session(
        &self, parent_session_id: &str, parent_tool_use_id: &str,
        agent_id: &str, title: Option<String>
    ) -> Result<String>;
    pub async fn persist_subagent_turn(
        &self, child_session_id: &str, turn_result: &TurnResult
    ) -> Result<()>;
}

// tool/builtin/agent.rs
pub struct AgentToolServices {
    pub prompt_service: SubagentPromptService,
    pub runtime_builder: SubagentRuntimeBuilder,
    pub conversation_writer: Arc<ConversationWriteHandle>,
}
```

## 行为规则

| 输入场景 | 子 Session 创建 | 持久化结果 | 取消行为 |
|---------|---------------|----------|---------|
| 父 turn 内 LLM 派生 Agent 工具（ToolContext 含 `tool_use_id="toolu_xxx"`） | 新子 Session，`parent_session_id=父id`、`parent_tool_use_id="toolu_xxx"` | 子 turn `Vec<Message>` 全部落子 Session.history，Assistant Message 带 ProviderHttpTrace | 父 token 取消 → 子 token 跟着取消，子 turn 中止 |
| 子 Agent 内部再次派生孙 Agent（嵌套） | 孙 Session，`parent_session_id=子id`、`parent_tool_use_id=子 turn 里那条 ToolUse 的 id` | 孙 Session.history 独立 | 父→子→孙 三级 child_token 链 |
| 同一 `tool_use_id` 重复被 `run_subagent` 调用（异常） | 创建多个子 Session（不去重）；父 Session.child_session_ids 内存去重不影响 DB | 各自独立 history | 各自独立 token |
| `ToolContext` 缺失（`context == None`，CLI / 独立调用） | 不创建子 Session，回退老语义"flat subagent" | 子 turn messages **不被持久化**（与现状一致） | 老语义 |
| `ToolContext` 存在但 `tool_use_id` 为空 | 视为上游 bug，`anyhow::bail!` 立即报错（不走 fallback、不创建子 Session） | 父 turn 收到 ToolResult is_error=true，包含错误信息 | — |
| `create_child_session` SQLite 失败 | 整个 `run_subagent` 报错回传给父 turn（父侧 ToolResult `is_error=true`） | 父 history 仍可看到 ToolUse + ToolResult(error) | — |

## 禁止事项

- 不修改父 Session.history 写入路径（父侧 ToolUse + ToolResult 维持现状）
- 不让子 Session 的 history 写入污染父 Session（必须使用 child_session_id）
- 不在子 turn 完成前就 commit 父 history 的 ToolResult（父 turn loop 自己控制；本 Plan 不动）
- 不引入 `delete_session` cascade 或父子级联清理（Plan 3 引入显式 `delete_session_tree`）
- 不暴露 `get_session_tree` 等对外 API（Plan 3）
- 不动 git tag
- 不引入 multi-turn 子 Agent 语义（见总览「已收敛的待澄清点」#6——本设计前提是 single-turn 子 Agent，`parent_tool_use_id` 一次写定即指首次派生点）

## 测试要求

| 文件 | 测试用例 | 输入 | 断言 |
|------|---------|------|------|
| `tool/builtin/agent.rs`（`#[cfg(test)] mod`） | `run_subagent_creates_child_session_when_tool_context_present` | mock ConversationWriteHandle 捕获调用；调 `run_subagent` 带 `ToolContext{session_id:"p1", tool_use_id:"toolu_x"}` | `create_child_session("p1","toolu_x","sub-agent-id", None)` 被调一次；`persist_subagent_turn(child_id, _)` 被调一次 |
| 同上 | `run_subagent_flat_fallback_without_tool_context` | mock；调 `run_subagent` 不带 ToolContext（`context = None`） | `create_child_session` 未被调用；fallback 路径执行 |
| 同上 | `run_subagent_bails_when_tool_use_id_empty` | mock；调 `run_subagent` 带 `ToolContext{ session_id:"p1", tool_use_id:"" }` | 返回 `Err`，错误信息含 "tool_use_id is empty"；`create_child_session` 未被调用 |
| 同上 | `run_subagent_propagates_cancellation` | mock；父 CancellationToken 在子 turn 进行中 cancel | 子 turn 收到 cancel；返回前 child_token().is_cancelled() == true |
| `app/conversation_service.rs`（`#[cfg(test)] mod`） | `create_child_session_persists_parent_columns` | 调 `ConversationWriteHandle::create_child_session("p1","toolu_x","agent",None)` 后用 repo `load_session_meta` 读子行 | `parent_session_id == Some("p1")`、`parent_tool_use_id == Some("toolu_x")` |
| 同上 | `create_child_session_pushes_into_parent_memory` | 先 create 父 Session 进 cache，再 create_child_session；调 `parent.get_child_ids().await` | 包含子 id |
| 同上 | `persist_subagent_turn_attaches_provider_http_trace_to_assistant` | 构造 mock `TurnResult { messages: [Assistant], provider_request_body: Some(_), provider_response_body: Some(_) }` 调 `persist_subagent_turn`；再 `repo.load_session(child_id)` | Assistant Message 的 metadata.provider_http_trace 存在且 requestBody/responseBody 与输入等值 |
| `tool/builtin/agent.rs` 集成 | `nested_subagent_creates_grandchild_session_under_child` | 子 Agent prompt 触发再调 Agent 工具；端到端跑（用 mock provider） | DB 中存在三个 Session：p / child(parent=p) / grandchild(parent=child)；`list_child_session_ids(p)` 含 child；`list_child_session_ids(child)` 含 grandchild |

**验证命令**：

```bash
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test --workspace -p nova-agent tool::builtin::agent
cargo test --workspace -p nova-agent app::conversation_service
```

## 完成条件

- [ ] `ConversationWriteHandle` 实现，含 `create_child_session` 与 `persist_subagent_turn` 两个方法
- [ ] `AgentToolServices` 增 `conversation_writer` 字段；`AgentApplicationImpl::new` 构造路径补全
- [ ] `AgentTool::run_subagent` 改造：有 ToolContext 时创建子 Session、子 turn 在子 session_id 上跑、persist 子 messages；无 ToolContext 走 fallback
- [ ] preload_tools 激活使用 child_session_id
- [ ] CancellationToken 父子链可用
- [ ] 全 workspace `cargo clippy / fmt / test` 全绿
- [ ] nova-cli 既有"独立 subagent"路径行为不变（fallback 兜底）
- [ ] orchestration 集成测试（`tests/integration/orchestration.rs`）零回归
- [ ] 本 Plan 的总览状态在 `session-parent-child-tree.md` 改为「已完成」并 commit
