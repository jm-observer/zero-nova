# 子 Session 独立化与父子树（供 zero 错误会话复盘消费）

## 时间

- 创建时间：2026-05-20
- 最后更新：2026-05-20（设计稿，事实核实完成、待澄清点已收敛，待实施）

## 背景与触发方

下游消费方为 zero 仓库的「错误会话复盘」能力（设计稿见 zero 仓 `docs/2026-05-20-错误会话复盘-session-tree/错误会话复盘-session-tree.md`）。该能力要求：当用户反馈某次会话处理有误时，能够离线复盘**父 Agent 与子 Agent 全过程的 LLM 通讯**（含工具调用 input/output 与每次 LLM 调用的 raw HTTP request/response），而不是仅仅看到顶层用户消息和最终回复文本。

zero 是 nova-agent 的 git tag 依赖（`zero/Cargo.toml`：`nova-agent = { git = "...zero-nova.git", tag = "v0.3.3" }`），消费方拿不到 nova 进程内的子 Agent 内部状态——必须由 **nova-agent 对外暴露子 Session 实体与父子树查询 API**。本设计落地这一层。

> 一旦本设计实施完成、发出新 nova tag（v0.3.5），zero 侧才能继续推进其 `错误会话复盘-session-tree` 的 Plan 1。
>
> 注：listing-api 设计（`docs/2026-05-20-skill-tool-listing-api/`）走 v0.3.4 单独发版；本设计走 v0.3.5。详见下方「listing-api 设计共存」段。

## 项目现状（代码勘察）

| 关注点 | 现状 | 证据 |
|--------|------|------|
| `conversation::Session` 字段 | 仅 `id / name / history(Vec<Message>) / control / created_at / updated_at / chat_lock / cancellation_token / title_state`，**无任何父子字段** | `crates/nova-agent/src/conversation/session.rs:9-21` |
| `Message.metadata` | `Option<MessageMetadata>` 已可挂 `ProviderHttpTrace`（raw request/response body） | `crates/nova-agent/src/message.rs:37-62` |
| `service::write::append_message` | 写 `session.history` 时已把 metadata 一并落进 SQLite repository | `crates/nova-agent/src/conversation/service/write.rs:60-106` |
| `AgentTool::run_subagent`（子 Agent 派生路径） | **直接复用父 session_id**：`session_id = context.session_id`；`runtime.run_turn_with_context(&session_id, ...)` | `crates/nova-agent/src/tool/builtin/agent.rs:661-696` |
| 子 turn 返回 | `TurnResult.messages: Vec<Message>` 在内存里产生，但**未回写到任何 Session.history**，turn 结束后丢失 | `crates/nova-agent/src/agent/runtime.rs:297-318` + 调用现场 |
| `AgentApplicationImpl` 对外 API | 有 `create_session` / `list_sessions` / `delete_session` 等，**无父子关系、无树查询** | `crates/nova-agent/src/app/application.rs` |
| `OrchestratorEngine` 派生层 | `SubAgentExecutor::execute_agent(req, ctx) -> SubAgentOutput { output: String, ... }`——**输出层面不暴露子 session_id**，orchestrator 自定 `agent_id="sub_{...}"` 是逻辑标识、非 conversation Session id | `crates/nova-agent/src/orchestrator/mod.rs:37-51, 366-377` |
| `TurnResult` 携带 raw HTTP body | `TurnResult { messages, usage, provider_request_body: Option<Value>, provider_response_body: Option<Value> }`——两 body 字段**已存在**；现有父 turn 路径（`ConversationService::start_turn:548-572`）把它们贴到每条 Assistant Message 的 `metadata.providerHttpTrace` | `crates/nova-agent/src/agent/runtime.rs:29-35`、`crates/nova-agent/src/app/conversation_service.rs:548-572` |

**结论**：当前架构里"子 Agent"在数据层**根本不是独立 Session**，而是父 Session 上的一次匿名 turn。父 history 里只能看到一对 `ToolUse{name:"Agent",...}` + `ToolResult{output:子最终回复}`，子内部的 LLM 通讯、再嵌套调用一律不可追溯。这是错误会话复盘能力的硬阻塞。

**复盘粒度的现状限制（设计前提，下游需知悉）**：`TurnResult` 的 `provider_request_body` / `provider_response_body` 是 **`Option<Value>` 单值**，仅携带 turn **末次 LLM 调用** 的 raw body。一个 turn 内若发生多轮 LLM（工具循环），中间轮次的 raw body 在 turn 结束时丢失；conversation_service:548-572 现有逻辑会把同一份"末次 trace"**重复贴到每条 Assistant Message**。本设计承诺的"完整复盘 LLM 通讯"在此基础上等价为**复盘每个子 Session 末次 LLM 通讯**。若未来要求 turn 内逐轮 trace，需要先扩 `TurnResult` 为 `Vec<TraceEntry>`，该工作不在本设计 v0.3.5 范围内。

## 整体目标

让"子 Agent"在 nova 数据层成为**真正独立的 `conversation::Session`**，与父 Session 通过 `(parent_session_id, parent_tool_use_id)` 反向锚定；新增对外 `get_session_tree` API，让外部消费方可拉到一棵以任意 Session 为根的完整父子树（每节点带 `Vec<Message>` + 元数据）。

最终对外契约（v0.3.5）：

```rust
pub struct Session {
    // 现有字段保留 ...
    pub parent_session_id: Option<String>,    // 一次写定
    pub parent_tool_use_id: Option<String>,   // 一次写定；定位到父 history 哪一条 ToolUse 派生出本 Session
    pub child_session_ids: RwLock<Vec<String>>, // append-only
}

impl AgentApplicationImpl {
    /// 列出指定 Session 的所有直接子 Session（轻量，不拉 history）
    pub async fn list_child_sessions(&self, parent_id: &str) -> Result<Vec<SessionSummary>>;

    /// 深度优先返回以 root_id 为根的完整子 Session 树（含每节点 history）
    /// max_depth 防退化（建议默认 8，0 表示仅根）
    pub async fn get_session_tree(&self, root_id: &str, max_depth: usize) -> Result<SessionTree>;
}

pub struct SessionTree {
    pub summary: SessionSummary,
    pub parent_tool_use_id: Option<String>,   // 复盘 UI 借此把本子树锚定到父 history 的某条 ToolUse
    pub history: Vec<Message>,                 // 含 ProviderHttpTrace
    pub children: Vec<SessionTree>,
}
```

复盘消费方算法（zero 侧）：

```text
walk(tree):
  for msg in tree.history:
    for block in msg.content:
      if block is ToolUse:
        child = find child in tree.children where child.parent_tool_use_id == block.id
        if child: render_inline(walk(child))
```

→ 父 history 是树根；子 Session 完整内嵌在对应 ToolUse 处；递归直到深度上限。

## 核心取舍

| 决策 | 取值 | 否决备选 + 理由 |
|------|------|----------------|
| **子 Agent 数据层定位** | 独立 `conversation::Session` | **否决：在父 Session 那条 `ToolResult` 上挂 `sub_messages: Vec<Message>`**——把"逻辑独立的对话"伪装成"父消息的附属字段"，扭曲 Message schema 语义；嵌套表达需要在 ContentBlock 上长出递归字段，污染面持续扩大；阻碍未来子 Session 独立取消 / 持久化 / list 的演进 |
| **父子关系持久化** | SQLite 持久化（schema 加列 + migration） | **否决：仅内存维护**——子 Session 既然作为一等公民，没有理由比父 Session 持久化弱；进程重启后非 flag 的会话父子关系丢失，限制了未来"按 session 查 trace 树"的能力（即便错误会话本身在 flag 时已 snapshot 进 zero run-state） |
| **关联键** | `parent_session_id` + `parent_tool_use_id` 都存 | **否决：只存 `parent_session_id`**——父 Agent 同一 turn 可能多次派生子 Agent，没 tool_use_id 复盘 UI 无法把子树锚定到具体哪条 ToolUse |
| **`delete_session` 默认行为** | 拒绝删除有子的 Session，提供显式 `delete_session_tree` 级联 API | **否决：默认级联删**——错误会话复盘场景下父 Session 可能被例行清理路径误删导致整棵 trace 丢失 |
| **取消传播** | 父 turn 取消 → 已派生但未完成的子 Session turn 跟着取消 | 通过 `CancellationToken` 父子关联 |

## 已收敛的待澄清点（实施时强制遵循）

以下决策是本设计稿评审后明确收敛的细节，Plan 1/2/3 实施时必须按此执行，**禁止再回到二选一状态**：

| # | 问题 | 决策 | 适用 Plan |
|---|------|------|-----------|
| 1 | `copy_session` 副本是否继承父子关系 | **不继承**（副本视为独立根 Session）。前提：`copy_session` 是"用户克隆为新对话"语义，不是内部 fork。如果未来出现需要保留父子关系的内部复制场景，必须在该场景另起 API、不要复用 `copy_session` | Plan 1 |
| 2 | `run_subagent` fallback 判定条件 | 判定 `context` **整体为 `None`** 即走 fallback；`Some(ctx)` 但 `tool_use_id.is_empty()` 视为**异常**并 `anyhow::bail!`，不走 fallback。理由：registry execute 路径必然带 tool_use_id，空 tool_use_id 一定是上游 bug，silent fallback 会掩盖 | Plan 2 |
| 3 | 嵌套场景下孙 Session 的 `parent_tool_use_id` 来源 | **依赖现有 `ToolContext.tool_use_id` 由 turn loop 自动推进**——子 turn 执行嵌套 Agent 调用时，runtime 已在分发工具时填入子 turn 自己生成的 ToolUse id。本设计**不需要额外维护**此链路。Plan 2 必须显式注释这点 | Plan 2 |
| 4 | `delete_session_tree(不存在的 root_id)` 返回值 | 返回 **`Ok(0)`**（与 `list_child_session_ids` 查不到不报错的语义一致）。实现上：DFS 收集前先检查 root 是否存在，不存在直接返回 `Ok(0)`，不进入 cache.remove 路径 | Plan 3 |
| 5 | `get_session_tree` 递归 IO 顺序 | **串行 await 子树**（实现简单、正确性优先）。已知性能限制：8 层 × 4 子 ≈ 65k Session 的极端场景慢。代码注释必须标注"已知串行；如未来下游高频调用则改 `futures::try_join_all`"。zero 侧调用频次为错误标记触发、非热路径，可接受 | Plan 3 |
| 6 | 子 Agent 是否 multi-turn | **本设计前提：子 Agent 是 single-turn**（一次 prompt → final_assistant_msg → 子 Session 关闭，不再续 turn）。`parent_tool_use_id` 一次写定、永远指向首次派生点。若未来子 Agent 需要 multi-turn 对话，应**在同一子 Session 内累积 history**，而非每轮重新派生新 Session。此约束在 ADR 中显式记录 | Plan 2 + ADR |
| 7 | SQL 字符串示例（Plan 1 步骤 4）末尾缺收尾 `)` | 文档示例的手写疏漏。实施时按真实 SQL 语法补完整（`VALUES (?, ?, ?, ?, ?, ?, ?, ?)` 与 `ON CONFLICT` 子句完整闭合） | Plan 1 |

## Plan 拆分

| 顺序 | Plan | 依赖 | 状态 |
|------|------|------|------|
| 1 | [Session 父子模型 + 持久化](session-parent-child-tree-plan-1.md) | 无 | ✅ 已完成 |
| 2 | [AgentTool 子 Session 路径改造](session-parent-child-tree-plan-2.md) | Plan 1 | ⏳ 待评审 |
| 3 | [`get_session_tree` API + v0.3.5 tag](session-parent-child-tree-plan-3.md) | Plan 1, Plan 2 | ⏳ 待评审 |

三个 Plan 严格顺序执行，每完成一个走 CLAUDE.md 修复流程（clippy + fmt + test 全绿）。Plan 3 完成后升 git tag `v0.3.5` 并推 origin。

## 需要更新的 docs/design 与设计影响记录

- 待实施完成后更新 `docs/design/system-overview.md`：在「Session 模型」段补"父子树"小节
- 待实施完成后更新 `docs/design/nova-agent-engine-boundaries.md`：在「对外 API」段补 `get_session_tree` / `list_child_sessions` 两条
- 新增 `docs/adr/2026-05-20-session-parent-child-tree.md`：记录核心取舍表四项决策的否决理由（已在本总览枚举，ADR 做更详细的"上下文 + 后果"补充）
- 标注关联：下游 zero 仓 `docs/2026-05-20-错误会话复盘-session-tree/` 系列文档

## 风险与待定项

- **SQLite migration**：本次 schema 加列（`parent_session_id`、`parent_tool_use_id`），需写 forward-only migration。旧库该两列 NULL，对应"无父" Session，行为与现状等价（零回归）
- **版本协调**：本设计要求发新 nova tag（v0.3.5）；zero 仓须随后同步改 `Cargo.toml` 的 `nova-agent` / `nova-agent-loader` tag。两侧版本耦合属于已知流程（zero 侧记忆 `project_zero_nova_custom_utils_coupling`）。本次为 add-only + 一次 SQLite migration，符合 patch 升级语义
- **listing-api 设计共存（决策已更新）**：另有 `docs/2026-05-20-skill-tool-listing-api/` 已实施完成、code 已 commit（待发版）。该设计与本设计在代码层无交叉。**决策：拆开两个 tag**——listing-api 单独发 v0.3.4（已完成、可立即发），本设计走 v0.3.5。理由：①两份设计耦合更轻、节奏更灵活；②zero 侧两个消费方（catalog-view、错误会话复盘）无依赖关系，可分别解除阻塞；③本设计含 SQLite migration + 子 Agent 派生路径重构，实施周期显著长于 listing-api，把 catalog-view 卡在同 tag 不合理
- **递归深度上限**：`get_session_tree(root_id, max_depth)` 默认 max_depth=8 防止子 Agent 递归调用导致树退化；超过深度的子树在响应里截断并标记 `truncated: true`
- **history 体积**：含 `ProviderHttpTrace` 的 Message 单条可达几十 KB；一棵完整 tree 在极端场景下可能 MB 级。`get_session_tree` 为按需调用（错误会话标记触发），不在常规热路径上，可接受。如未来纳入常规监控再考虑流式/分页
- **chat_lock 隔离**：子 Session 有独立 `chat_lock: Mutex<()>`——这是 X 方案相对 sub_messages 内嵌方案的天然收益（父子并发清晰、子 Session 可独立取消/续跑）
- **跨平台**：纯 Rust + rusqlite，windows + linux 双目标编译走修复流程验证
