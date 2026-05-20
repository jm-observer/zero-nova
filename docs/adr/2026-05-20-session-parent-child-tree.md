# ADR: 子 Session 独立化与父子树（v0.3.4）

- 日期：2026-05-20
- 状态：提议中（待 Plan 1-3 实施完成转 Accepted）
- 关联设计：[`docs/2026-05-20-session-parent-child-tree/`](../2026-05-20-session-parent-child-tree/session-parent-child-tree.md)
- 下游消费方：zero 仓「错误会话复盘」（`zero/docs/2026-05-20-错误会话复盘-session-tree/`）

## 上下文

zero 仓有「错误会话标记」能力（v0.3.3 实现）：用户反馈某次会话处理有误时，调 `session_flag` 工具把当前活跃 session 的 `Vec<SessionMessage>` 快照进 run-state，供离线复盘。

但**复盘范围严重不够**：

1. zero 侧 `record_message` 只写了入站用户原文 + 顶层 Agent 最终回复字符串两类内容
2. 子 Agent（OrchestrateTask 委派出的 skill 子 Agent）的活动**根本不进 zero 的 session_message 表**
3. nova 这边子 Agent 的 turn 也**没有落进任何 Session.history**——`AgentTool::run_subagent` 调 `runtime.run_turn_with_context` 时复用父 session_id（`crates/nova-agent/src/tool/builtin/agent.rs:661-664`），但 turn 结束后 `TurnResult.messages` 没人持久化，事实上丢失

→ 错误会话只能看到「用户说啥 / Agent 最后回啥」，子 Agent 内部 LLM 通讯、工具调用、再嵌套调用全部不可追溯，**无法定位错误根因**。

需要让"子 Agent"在 nova 数据层成为**真正的独立 Session**，与父 Session 双向锚定；新增 `get_session_tree` 让下游一次拉到完整树。

## 决策

**1. 子 Agent 数据层定位：独立 `conversation::Session`**

子 Agent 派生时新建 `Session`，与父通过 `(parent_session_id, parent_tool_use_id)` 反向锚定；子 turn 在子 session 上运行，全部消息（含 `ProviderHttpTrace`）写入子 Session.history。父 Session.history 维持现状（仅含父调子的 ToolUse + ToolResult 一对 ContentBlock，语义就是"父调子的接口边界"）。

**2. 父子关系 SQLite 持久化**

`sessions` 表加 `parent_session_id TEXT` + `parent_tool_use_id TEXT` 两列 + `parent_session_id` 索引；forward-only ALTER migration。

**3. 关联键：`parent_session_id` + `parent_tool_use_id` 双键**

`parent_tool_use_id` 必存：父 Agent 同一 turn 可能多次派生子 Agent，没有 tool_use_id 复盘 UI 无法把子树锚定到具体 ToolUse。

**4. `delete_session` 默认拒绝有子的 Session；提供显式 `delete_session_tree`**

避免错误会话的 trace 树被例行清理路径误删。

**5. 取消传播：父 CancellationToken 派生 child_token 给子 turn**

父 turn 取消 → 已派生但未完成的子 turn 跟着取消。

## 否决的备选

### 备选 A：在父 Session 的 ToolResult 上挂 `sub_messages: Vec<Message>`

**做法**：扩 `ContentBlock::ToolResult` 增 `sub_messages: Option<Vec<Message>>` 字段；`AgentTool::run_subagent` 把 `turn_result.messages` 整段塞进父 history 那条 ToolResult 的 `sub_messages`。嵌套通过子 Vec<Message> 里又出现 ToolResult.sub_messages 递归表达。

**优点**：
- 改动局限在 `ContentBlock::ToolResult` schema + `AgentTool` 单点回写
- 不引入子 Session 生命周期管理
- 数据结构上"子 trace 永远跟着父 ToolUse 走"，复盘 UI 直接遍历即可

**否决理由**：
- 把"逻辑独立的对话"伪装成"父消息的附属字段"，扭曲 `Message` schema 语义
- 阻碍未来子 Session 独立特性的演进（独立取消、独立持久化、独立 list、子 Session 续跑等）
- `ContentBlock` 是 LLM 上下文的载体，挂大体积附属数据会让 SQLite 序列化/反序列化体积膨胀，且需要在所有 ContentBlock 消费点做"跳过 sub_messages"特殊处理
- 嵌套递归字段（`ContentBlock` → `Message` → `ContentBlock` → ...）让 schema 自身循环引用，影响 derive、serde、跨语言绑定

### 备选 B：父子关系仅内存维护，不持久化

**做法**：`Session` 加字段，但 SQLite schema 不动；进程重启后父子关系丢失。

**优点**：
- 零 migration、零 SQL 改动
- 错误会话在 flag 时由 zero 侧 SessionFlagTool 当场 snapshot 整树进 run-state，进程重启不影响已 flag 的数据

**否决理由**：
- 子 Session 既然作为一等公民，没有理由比父 Session 持久化弱——这是逻辑上的双标
- 限制未来"按任意 session 查 trace 树"的能力（比如想看一个非 flag 状态会话的子 Agent 历史，重启就没法查）
- 进程重启路径上 cache 重建时父子链丢失，未来若要在重启后做 trace 关联会被迫追加"内存中重建关系"的旁路逻辑，复杂度反而高

### 备选 C：只存 `parent_session_id`，不存 `parent_tool_use_id`

**优点**：
- schema 只需 1 列

**否决理由**：
- 父 Agent 同一 turn 可能多次派生子 Agent（同一 turn 内多个 OrchestrateTask 调用、parallel stage）
- 复盘 UI 拿到一棵 children 列表但不知道每个 child 对应父 history 哪条 ToolUse，没法做"展开父 ToolUse 看里面这次调了什么"的交互
- 多省一列的收益（持久化体积、查询代价）相对于损失的可观测性完全不成比例

### 备选 D：`delete_session` 隐式级联删除

**优点**：
- 调用者无需关心父子关系

**否决理由**：
- 错误会话 trace 树是核心资产；如果某个例行清理路径不知道父子关系而调 `delete_session(parent)`，整棵 trace 静默丢失，且无任何报错提示
- 显式 API 让"删整棵树"成为有意识的决定；默认拒绝并报错让调用者必须显式选择 `delete_session_tree`

## 后果

### 正向

- 错误会话复盘从"用户说啥 / Agent 回啥"升级为"父子全树 + 每节点每次 LLM 调用的 raw HTTP req/resp"，能真正定位子 Agent 错调工具、LLM 给出错误回复等根因
- 子 Session 成为一等公民后，未来可扩展：独立续跑、独立取消、单子 Session 重放、子 Session 级别的 token 用量计费、跨子 Session 的失败重试策略等
- 与外部消费方（zero、未来其它）的边界清晰：只对外暴露 `get_session_tree` 等克隆数据的 API，不泄漏 nova 内部 Session/Cache 引用

### 负向

- nova 一次 patch 升级（v0.3.4）即引入 SQLite migration + AgentTool 关键路径改动，发布风险高于纯 add-only 设计
- 现有 `delete_session` 调用者拿到"拒绝"错误后必须显式判断是否走 `delete_session_tree`——是不向后兼容的语义变化（理由：默认级联删的风险大于二选一的迁移成本）
- 子 turn 持久化让每条子 Session.history 都通过 `append_message` 走完整 SQLite 写路径，子 Agent 量大时 IO 增加（实测后若有性能问题再考虑批量 write）

### 风险与缓解

| 风险 | 缓解 |
|------|------|
| migration 失败 | forward-only `ALTER TABLE ADD COLUMN` 是 SQLite 最安全的 schema 变更；测试覆盖"老库→新库"路径 |
| 子 Session 暴涨 | `delete_session_tree` 显式 API + 未来可加 TTL 策略；当前错误会话场景增量可控（每次错误才一棵树） |
| 嵌套退化 | `get_session_tree(_, max_depth=8)` 防退化；超过深度的子树截断并标记 `truncated: true` |
| 与 `docs/2026-05-20-skill-tool-listing-api` 同 tag 协调 | 两份设计代码无交叉，可合并到 v0.3.4 同 tag 发布；ADR 互不阻塞 |
| zero 侧 bump tag | 沿用既有 zero ↔ zero-nova 版本耦合流程（zero memory：`project_zero_nova_custom_utils_coupling`）；本次为兼容 patch + migration，与既有流程一致 |

## 跟进事项

- Plan 3 完成、tag v0.3.4 push 后，本 ADR 状态改为 Accepted 并补"实施后回顾"段（实际改动行数、修复流程耗时、发现的问题）
- 6 个月后回顾：子 Session 数据库膨胀是否需要 TTL/归档策略
- 未来若需要"子 Agent 失败重跑"，本次 X 方案的独立 Session 模型已经为之铺垫——直接以子 session_id 为单位续跑即可
