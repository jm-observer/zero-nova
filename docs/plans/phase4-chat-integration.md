# Phase 4：会话控制层骨架

> 前置依赖：Phase 1-3
> 基线设计：`docs/conversation-control-plane-design.md`

## 1. 目标

这是从"普通 chat gateway"走向"可控 agent 系统"的第一阶段。
引入 **TurnRouter** 作为决策中枢，给 `Session` 增加控制层状态扩展位。

核心交付：
- 每轮用户输入经过 `TurnRouter` 分类后再分发执行
- 最小化 `PendingInteraction` 能工作（创建、解析、过期）
- `handle_chat` 改造为 `route_turn -> execute_turn` 的两阶段结构

## 2. 当前代码现状（Phase 3 完成后预期）

Phase 3 完成后，`Session` 和 `handle_chat` 已具备：
- 稳定的消息读写 API
- `chat_lock` 串行化
- `CancellationToken` 支持
- `run_turn` 返回 `TurnResult { messages, usage }`

但仍缺少：
- 每轮输入的意图分类（当前所有输入一律进入 chat）
- 挂起交互管理（系统无法"等待用户确认"）
- 控制层状态存储位（`Session` 上没有 workflow / pending 字段）

## 3. 详细设计 (Detailed Design)

### 3.1 Session 控制层扩展

在 `Session` 上增加 `ControlState`：

```rust
pub struct Session {
    // --- Phase 3 已有字段 ---
    pub id: String,
    pub name: String,
    pub history: RwLock<Vec<Message>>,
    pub created_at: i64,
    pub updated_at: AtomicI64,
    pub chat_lock: Mutex<()>,
    pub cancellation_token: RwLock<Option<CancellationToken>>,
    // --- Phase 4 新增 ---
    pub control: RwLock<ControlState>,
}

pub struct ControlState {
    pub active_agent: String,
    pub pending_interaction: Option<PendingInteraction>,
    pub workflow: Option<WorkflowState>,  // Phase 5 填充，此处预留
}

impl ControlState {
    pub fn new(default_agent: &str) -> Self {
        Self {
            active_agent: default_agent.to_string(),
            pending_interaction: None,
            workflow: None,
        }
    }
}
```

### 3.2 TurnRouter

`TurnRouter` 接收用户输入和当前 `ControlState`，返回分类结果：

```rust
pub enum TurnIntent {
    /// 存在挂起交互，用户本轮输入应作为回应处理
    ResolvePendingInteraction,
    /// 用户点名了某个 agent
    AddressAgent { agent_id: String },
    /// 存在活跃 workflow，用户输入应继续该流程
    ContinueWorkflow,
    /// 普通聊天 / 新任务
    ExecuteChat,
}

pub struct TurnRouter;

impl TurnRouter {
    /// 判断本轮用户输入的控制意图
    pub fn classify(
        input: &str,
        control: &ControlState,
        agent_registry: Option<&AgentRegistry>,
    ) -> TurnIntent {
        // 优先级 1：挂起交互
        if control.pending_interaction.is_some() {
            return TurnIntent::ResolvePendingInteraction;
        }

        // 优先级 2：agent 点名（Phase 5 实现，此处占位）
        if let Some(registry) = agent_registry {
            if let Some(agent_id) = registry.resolve_addressing(input) {
                return TurnIntent::AddressAgent { agent_id };
            }
        }

        // 优先级 3：workflow 延续（Phase 5 实现，此处占位）
        if control.workflow.is_some() {
            return TurnIntent::ContinueWorkflow;
        }

        // 优先级 4：普通聊天
        TurnIntent::ExecuteChat
    }
}
```

**Fast path 优化**：当 `ControlState` 无 pending、无 workflow、无 agent_registry 时，`classify` 直接返回 `ExecuteChat`，不做任何额外判断。

### 3.3 PendingInteraction

```rust
pub struct PendingInteraction {
    pub id: String,
    pub kind: InteractionKind,
    pub subject: String,             // 针对什么动作 (如: "写入 config.toml")
    pub prompt: String,              // 展示给用户的提示文本
    pub options: Vec<InteractionOption>,
    pub risk_level: RiskLevel,
    pub created_at: i64,
    pub ttl_seconds: u64,            // 过期时长，而非绝对 deadline
}

pub enum InteractionKind {
    /// 需要用户批准一个动作（如写文件、部署）
    Approve,
    /// 需要用户从多个选项中选择
    Select,
    /// 需要用户提供自由文本输入
    Input,
}

pub struct InteractionOption {
    pub id: String,
    pub label: String,
    pub aliases: Vec<String>,        // "好的" / "OK" / "继续" 等同义词
}

pub enum RiskLevel {
    Low,
    Medium,
    High,
}
```

#### 过期策略

| 条件 | 行为 |
|------|------|
| `now - created_at > ttl_seconds` | 自动清除 pending，下次用户输入按正常流程处理 |
| 用户输入被解析为明确拒绝 | 清除 pending，通知用户"操作已取消" |
| 用户输入无法归类（低置信度） | 保持 pending，提示用户重新回应 |

过期检查在 `TurnRouter::classify` 入口处执行：如果 pending 已过期，先清除再继续分类。

### 3.4 InteractionResolver

```rust
pub struct InteractionResolver;

pub struct ResolutionResult {
    pub intent: ResolutionIntent,
    pub selected_option_id: Option<String>,
    pub free_text: Option<String>,
}

pub enum ResolutionIntent {
    Approve,
    Reject,
    Select,
    ProvideInput,
    Unclear,  // 无法确定用户意图
}

impl InteractionResolver {
    /// 初版使用纯规则匹配，不调用 LLM
    pub fn resolve(
        input: &str,
        pending: &PendingInteraction,
    ) -> ResolutionResult {
        // 1. 对 Approve 类型：匹配肯定词表 ("好的", "OK", "继续", "同意", "是")
        //    和否定词表 ("不", "取消", "算了", "停")
        // 2. 对 Select 类型：匹配 option 的 id / label / aliases
        //    以及序号匹配 ("第一个", "1", "A")
        // 3. 对 Input 类型：直接返回 ProvideInput + free_text
        // 4. 匹配失败：返回 Unclear
    }
}
```

**为什么初版不用 LLM**：
- "OK" / "第二个" 这类高频回应用关键词表即可覆盖 90%+ 场景
- 避免每轮输入都产生额外 LLM 调用的延迟和成本
- 只有当规则匹配返回 `Unclear` 时，后续 Phase 可选择 fallback 到 LLM

### 3.5 handle_chat 改造

```rust
pub async fn handle_chat<C: LlmClient>(
    payload: ChatPayload,
    state: Arc<AppState<C>>,
    outbound_tx: ...,
    msg_id: String,
) {
    let session = state.sessions.get_or_create(payload.session_id).await;
    let _guard = session.chat_lock.lock().await;

    // Phase 4 新增：路由分类
    let control = session.control.read();
    let intent = TurnRouter::classify(&payload.input, &control, None);
    drop(control);

    match intent {
        TurnIntent::ResolvePendingInteraction => {
            handle_resolve_interaction(session, &payload.input, outbound_tx, msg_id).await;
        }
        TurnIntent::AddressAgent { .. } => {
            // Phase 5 实现
            send_error(outbound_tx, "NOT_IMPLEMENTED", "Agent switching not yet available");
        }
        TurnIntent::ContinueWorkflow => {
            // Phase 5 实现
            send_error(outbound_tx, "NOT_IMPLEMENTED", "Workflow not yet available");
        }
        TurnIntent::ExecuteChat => {
            // 原 handle_chat 核心逻辑：write user msg -> run_turn -> write result -> chat.complete
            execute_chat_turn(session, &payload, state, outbound_tx, msg_id).await;
        }
    }
}
```

### 3.6 PendingInteraction 协议消息

需要在 `MessageEnvelope` 中新增：

```rust
// 服务端推送：告知客户端当前有挂起交互
InteractionRequest(InteractionRequestPayload),  // type: "interaction.request"
// 服务端推送：交互已解决
InteractionResolved(InteractionResolvedPayload), // type: "interaction.resolved"
```

```rust
pub struct InteractionRequestPayload {
    pub session_id: String,
    pub interaction_id: String,
    pub kind: String,          // "approve" | "select" | "input"
    pub subject: String,
    pub prompt: String,
    pub options: Vec<InteractionOptionDTO>,
    pub risk_level: String,    // "low" | "medium" | "high"
}

pub struct InteractionResolvedPayload {
    pub session_id: String,
    pub interaction_id: String,
    pub result: String,        // "approved" | "rejected" | "selected" | "input" | "expired"
}
```

## 4. 本 phase 范围

### 4.1 要做

- 在 `Session` 上增加 `control: RwLock<ControlState>` 字段
- 实现 `TurnRouter::classify`（含 fast path）
- 实现 `PendingInteraction` 数据结构与过期检查
- 实现 `InteractionResolver`（纯规则匹配版）
- 改造 `handle_chat` 为 `classify -> match intent` 结构
- 新增 `interaction.request` / `interaction.resolved` 协议消息
- 补充测试

### 4.2 不做

- 不做完整的 SolutionWorkflow（Phase 5）
- 不做多 Agent 注册与切换（Phase 5）
- 不做 LLM-based InteractionResolver（后续按需加）
- 不做复杂的语义打分机制

## 5. 实施步骤

### Step 1：定义控制层数据结构

文件：
- `src/gateway/control.rs`（新建）

动作：
- 定义 `ControlState`、`PendingInteraction`、`InteractionKind`、`RiskLevel`
- 定义 `TurnIntent`
- 实现 `TurnRouter::classify`
- 实现 `InteractionResolver::resolve`

### Step 2：扩展 Session

文件：
- `src/gateway/session.rs`

动作：
- 增加 `control: RwLock<ControlState>` 字段
- 在 `Session::new` 中初始化 `ControlState`

### Step 3：新增协议消息

文件：
- `src/gateway/protocol.rs`

动作：
- 增加 `InteractionRequest` / `InteractionResolved` 枚举分支
- 增加对应 payload 结构体

### Step 4：改造 handle_chat

文件：
- `src/gateway/handlers/chat.rs`

动作：
- 入口增加 `TurnRouter::classify` 调用
- 拆分为 `handle_resolve_interaction` 和 `execute_chat_turn`
- `handle_resolve_interaction` 中：解析用户输入 -> 更新 ControlState -> 发送 `interaction.resolved`

### Step 5：补测试

文件：
- `src/gateway/control.rs`（内联测试）

## 6. 测试方案

### 6.1 TurnRouter 单元测试

| 测试用例 | 验证点 |
|---------|-------|
| `test_classify_no_state` | 无 pending / workflow / agent 时返回 `ExecuteChat` |
| `test_classify_with_pending` | 有 pending 时返回 `ResolvePendingInteraction` |
| `test_classify_pending_expired` | pending 已过期时清除并返回 `ExecuteChat` |
| `test_classify_pending_priority` | 同时有 pending 和 workflow 时 pending 优先 |

### 6.2 InteractionResolver 单元测试

| 测试用例 | 验证点 |
|---------|-------|
| `test_resolve_approve_ok` | "好的" / "OK" / "继续" 解析为 Approve |
| `test_resolve_reject` | "取消" / "不要" 解析为 Reject |
| `test_resolve_select_by_index` | "第二个" / "2" 解析为 Select + 正确 option_id |
| `test_resolve_select_by_alias` | option alias 匹配 |
| `test_resolve_input` | Input 类型直接返回 free_text |
| `test_resolve_unclear` | 无法匹配时返回 Unclear |

### 6.3 集成测试

| 测试用例 | 验证点 |
|---------|-------|
| `test_chat_with_no_control_state` | 行为与 Phase 3 完全一致（回归） |
| `test_pending_resolve_flow` | 手动注入 pending -> 发送确认 -> pending 被清除 |

### 6.4 回归要求

```powershell
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test --workspace
```

## 7. 完成定义

- [ ] `Session.control` 字段存在且可读写
- [ ] `TurnRouter::classify` 正确分类所有 `TurnIntent` 变体
- [ ] `PendingInteraction` 过期检查生效
- [ ] `InteractionResolver` 能解析 Approve / Reject / Select / Input
- [ ] `handle_chat` 入口走 `classify -> match` 结构
- [ ] `interaction.request` / `interaction.resolved` 协议消息定义完整
- [ ] 无 pending/workflow 时 handle_chat 行为与 Phase 3 完全一致（回归）
- [ ] 全部测试用例通过

## 8. 给下一阶段的交接信息

Phase 4 完成后：
- `Session` 上已有 `ControlState` 扩展位，Phase 5 直接填充 `workflow` 和 `AgentRegistry`
- `TurnRouter::classify` 的 `AddressAgent` 和 `ContinueWorkflow` 分支已预留，Phase 5 只需实现判断逻辑和执行逻辑
- `PendingInteraction` 基础设施已就绪，Phase 5 的 `SolutionWorkflow` 可以直接创建 pending 交互
- `InteractionResolver` 使用规则匹配，Phase 5/6 可按需升级为 LLM fallback
