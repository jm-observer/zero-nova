# zero-nova 集成设计 — Agent 运行时核心

## 1. 概述

### 1.1 定位

zero-nova 作为 **Agent 运行时核心**，独立运行并提供完整的 Agent 能力栈。zero 依赖 zero-nova，专注通道层和消息路由层。

### 1.2 核心能力

| 能力 | 模块 | 说明 |
|------|------|------|
| Agent 运行态 | `nova-core/agent.rs` | AgentRuntime<C: LlmClient>，支持 iteration 循环 |
| 工具系统 | `nova-core/tool.rs` | ToolRegistry + ToolContext + ToolSearch |
| 技能系统 | `nova-core/skill.rs` | SkillRegistry + CapabilityPolicy |
| LLM 客户端 | `nova-core/provider/` | LlmClient trait + SSE 流式接收 |
| 会话管理 | `nova-conversation/` | SessionService + SqliteSessionRepository |
| 应用层 | `nova-app/application.rs` | AgentApplication + AgentApplicationImpl |
| 协议层 | `nova-protocol/` | GatewayMessage + MessageEnvelope |
| 网关核心 | `nova-gateway-core/` | GatewayHandler + dispatch |

---

## 2. 架构设计

### 2.1 当前架构

```
┌─────────────────────────────────────────────────────────────┐
│                    用户客户端                                 │
│  (Tauri Desktop / WebSocket / stdio)                        │
└──────────────┬──────────────────────────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────────────────────────┐
│              nova-gateway-core (GatewayHandler)               │
│  on_connect/on_disconnect/on_message                          │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  dispatch(MessageEnvelope) → handler module             │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────┬─────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                  nova-app (AgentApplication)                  │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  AgentApplicationImpl                                    │ │
│  │  ┌─────────────────────────────────────────────────────┐ │ │
│  │  │  AgentRuntime<C> (nova-core)                        │ │ │
│  │  │  LlmClient + ToolRegistry + SkillRegistry           │ │ │
│  │  └─────────────────────────────────────────────────────┘ │ │
│  │  ┌─────────────────────────────────────────────────────┐ │ │
│  │  │  ConversationService (nova-conversation)            │ │ │
│  │  │  SessionService + SessionCache + SessionRepository   │ │ │
│  │  └─────────────────────────────────────────────────────┘ │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 与 zero 的关系

```
                    zero (外部消费者)
┌──────────────────────────────────────────────────────────────────┐
│                                                                  │
│  1. 创建 C: LlmClient (OpenAiCompatClient 或 AnthropicClient)    │
│  2. 创建 ToolRegistry + 注册工具 (Bash/Read/Write/Edit/...)       │
│  3. 创建 AgentConfig (从 AppConfig 获取)                         │
│  4. AgentRuntime::new(client, tools, config)                     │
│  5. 可选: 设置 task_store, skill_registry                        │
│  6. 调用 agent.run_turn(history, input, event_tx, cancel_token)  │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

---

## 3. 核心接口设计

### 3.1 AgentRuntime 接口 (nova-core/src/agent.rs)

```rust
pub struct AgentRuntime<C: LlmClient> {
    client: C,                          // LLM 客户端
    tools: ToolRegistry,                // 工具注册中心
    config: AgentConfig,                // 运行时配置
    pub task_store: Option<Arc<Mutex<TaskStore>>>,
    pub skill_registry: Option<Arc<SkillRegistry>>,
    pub read_files: Arc<Mutex<HashSet<String>>>,
}

// 核心接口
impl<C: LlmClient> AgentRuntime<C> {
    pub fn new(client: C, tools: ToolRegistry, config: AgentConfig) -> Self
    pub async fn run_turn(<history>, <input>, <event_tx>, <cancel_token>) -> TurnResult
    pub async fn prepare_turn(<input>, <Arc<Vec<Message>>>) -> TurnContext
    pub async fn run_turn_with_context(<TurnContext>, <Message>, <event_tx>) -> TurnResult
    pub fn set_tools(&mut self, tools: ToolRegistry)
    pub fn register_tool(&mut self, tool: Box<dyn Tool>)
    pub fn tools(&self) -> &ToolRegistry
}
```

### 3.2 LlmClient 接口 (nova-core/src/provider/mod.rs)

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn stream(&self, <messages>, <tools>, <config>) -> Result<Box<dyn StreamReceiver>>;
}

#[async_trait]
pub trait StreamReceiver: Send {
    async fn next_event(&mut self) -> Result<Option<ProviderStreamEvent>>;
}
```

### 3.3 AgentEvent 类型 (nova-core/src/event.rs)

```rust
pub enum AgentEvent {
    TextDelta(String),
    ThinkingDelta(String),
    ToolStart { id, name, input },
    ToolEnd { id, name, output, is_error },
    LogDelta { id, name, log, stream },
    TurnComplete { new_messages, usage },
    IterationLimitReached { iterations },
    Iteration { current, total },
    Error(String),
    SystemLog(String),
    AssistantMessage { content },
    AgentSwitched { agent_id, agent_name, description },
    // Task 相关 (Plan 4)
    TaskCreated { id, subject },
    TaskStatusChanged { id, subject, status, active_form },
    BackgroundTaskComplete { name, result },
    // Skill 相关
    SkillLoaded { skill_name },
    SkillActivated { skill_id, skill_name, sticky, reason },
    SkillSwitched { from_skill, to_skill },
    SkillExited { skill_id },
    // Tool 解锁
    ToolUnlocked { tool_name },
}
```

---

## 4. Agent 运行循环

### 4.1 run_turn 流程

```
AgentRuntime::run_turn()
    │
    ├── 1. 追加用户输入到 all_messages
    │
    ├── 2. for iteration 0..max_iterations:
    │       │
    │       ├── 2.1 检查 cancellation_token
    │       │
    │       ├── 2.2 发送 AgentEvent::Iteration
    │       │
    │       ├── 2.3 获取 tool_definitions
    │       │
    │       ├── 2.4 调用 client.stream() 获取 SSE 流
    │       │       │
    │       │       └── 遍历流事件:
    │       │               ├── TextDelta/ThinkingDelta → 发送 AgentEvent
    │       │               ├── ToolUseStart/ToolUseInputDelta → 累积 tool_calls
    │       │               └── MessageComplete → 记录 usage
    │       │
    │       ├── 2.5 构建 assistant message (含 Thinking/Text/ToolUse)
    │       │
    │       ├── 2.6 处理 MaxTokens 自动续写
    │       │
    │       ├── 2.7 如果无 tool_calls → completed_naturally = true, break
    │       │
    │       └── 2.8 并行执行所有 tool calls (FuturesUnordered + timeout)
    │               │
    │               └── 按 call_idx 排序结果 → 追加 tool_result message
    │
    └── 3. 如非 naturally 完成 → 发送 IterationLimitReached + TurnComplete
```

### 4.2 Tool 执行流程

```
ToolRegistry::execute("<tool_name>", <input>, <context>)
    │
    ├── 1. 特殊工具 ToolSearch
    │       └── 按需加载 deferred tools
    │
    └── 2. 名称规范化 (bash→Bash, read_file→Read, ...)
            │
            └── 查找并执行: self.lock_tools().find(tool).execute(input, context)
```

---

## 5. 会话管理设计

### 5.1 Session 结构 (nova-conversation/src/session.rs)

```rust
pub struct Session {
    pub control: RwLock<ControlState>,           // active_agent: String
    pub id: String,
    pub name: String,
    pub history: RwLock<Vec<Message>>,
    pub created_at: i64,
    pub updated_at: AtomicI64,
    pub chat_lock: Mutex<()>,                   // 并发控制
    pub cancellation_token: RwLock<Option<CancellationToken>>,
}
```

### 5.2 SessionService 架构

```
┌─────────────────────────────────────────────────────────────┐
│                    SessionService                           │
├─────────────────────────────────────────────────────────────┤
│  cache: Arc<SessionCache>        ← 内存读穿缓存             │
│  repository: SqliteSessionRepository  ← SQLite 持久化       │
│  loading: Arc<RwLock<HashMap>>   ← 并发加载门控             │
└─────────────────────────────┬───────────────────────────────┘
                              │
         ┌──────────────────────┴──────────────────────┐
         │                                             │
   ┌─────▼─────┐                                ┌─────▼─────┐
   │ SessionCache│                               │Sqlite      │
   │ (启动加载)  │                               │Repository │
   └─────────────┘                                └───────────┘
```

---

## 6. Agent 运行配置

### 6.1 配置文件格式 (config.toml)

```toml
# LLM 服务配置
[llm]
api_key = "your-key"
base_url = "http://192.168.0.68:8082/v1"
model = "Huihui-Qwen3.6-35B-A3B-Claude-4.6"
max_tokens = 8192
temperature = 0.7
top_p = 1.0
thinking_budget = 4096

# 网关配置
[gateway]
host = "127.0.0.1"
port = 18801
max_iterations = 30
tool_timeout_secs = 3600

[gateway.router]
use_llm_classification = false

# Agent 注册
[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "默认通用助手"
aliases = ["小助手", "助手"]

[[gateway.agents]]
id = "openclaw"
display_name = "OpenClaw"
description = "资深架构师"
aliases = ["oc", "claw", "架构师"]

# 历史记录裁剪
[gateway.trimmer]
max_history_tokens = 50000
preserve_recent = 10
preserve_tool_pairs = true

# 工具配置
[tool]
skills_dir = "{workspace}/.nova/skills"
prompts_dir = "{workspace}/prompts"
data_dir = "{workspace}/.nova/data"
```

### 6.2 AgentConfig 结构

```rust
pub struct AgentConfig {
    pub max_iterations: usize,      // 默认 30
    pub model_config: ModelConfig,  // 模型配置
    pub tool_timeout: Duration,     // 默认 120s
    pub max_tokens: usize,          // 默认 4096
}
```

---

## 7. 数据流全景

```
                    ┌───────────────────────────────────────────────────────────┐
                    │                     外部请求                               │
                    │              (HTTP / WS / stdio)                         │
                    └────────────────────────┬──────────────────────────────────┘
                                             │
                                             ▼
              ┌────────────────────────────────────────────────────────────────────┐
              │                            GatewayHandler                          │
              │                     dispatch(MessageEnvelope)                      │
              └────────────────────────────────┬───────────────────────────────────┘
                                               │
                                               ▼
              ┌────────────────────────────────────────────────────────────────────┐
              │                         AgentApplicationImpl                       │
              │    ┌───────────────────────────────────────────────────────────┐   │
              │    │  start_turn(session_id, input, event_tx)                  │   │
              │    │    └─► AgentRuntime::run_turn()                          │   │
              │    └───────────────────────────────────────────────────────────┘   │
              │          │                                                         │
              │          ▼ (AgentEvent 流)                                         │
              │    ┌───────────────────────────────────────────────────────────┐   │
              │    │  AgentRuntime::run_turn()                                │   │
              │    │    ┌───────────────────────────────────────────────────┐ │   │
              │    │    │ LlmClient::stream() → ProviderStreamEvent(流)    │ │   │
              │    │    └───────────────────────────────────────────────────┘ │   │
              │    │         │                                                   │   │
              │    │         ▼ (TextDelta/ThinkingDelta etc)                   │   │
              │    │  ┌──────────────────────────────────────────────────────┐  │   │
              │    │  │ AgentEvent → AppEvent → GatewayMessage              │  │   │
              │    │  └──────────────────────────────────────────────────────┘  │   │
              │    └───────────────────────────────────────────────────────────┘   │
              │          │                                                         │
              │          ▼ (BridgeEvent/MessageCompleted 等)                       │
              └──────────┼─────────────────────────────────────────────────────────┘
                         │
                         ▼
              ┌────────────────────────────────────────────────────────────────────┐
              │                          返回给用户                                │
              │              (WebSocket / HTTP Response / GUI)                     │
              └────────────────────────────────────────────────────────────────────┘
```

---

## 8. 与 zero 的集成接口

### 8.1 Agent 启动接口

```rust
// zero 通过此接口获取零-nova 的 Agent 运行时
pub fn create_agent_runtime(config: AppConfig) -> anyhow::Result<AgentRuntime>
```

### 8.2 消息发送接口

```rust
// 零发送消息给 Agent
async fn send_message_to_agent<NovaAgentBridge>

# Agent 运行时接口
async fn AgentRuntime::run_turn(
    history: &[Message],
    input: &str,
    event_tx: mpsc::Sender<AgentEvent>,
    cancel_token: CancellationToken
) -> TurnResult
```

### 8.3 事件映射

```rust
// zero-nova 内部事件
AgentEvent::TurnComplete { new_messages, usage }

// zero-nova → zero 转换
impl From<nova_core::event::AgentEvent> for AppEvent {
    fn from(event: AgentEvent) -> Self {
        match event {
            AgentEvent::TextDelta(text) => AppEvent::Token(text),
            AgentEvent::ToolStart { id, name, input } => AppEvent::ToolStart { id, name, input },
            AgentEvent::ToolEnd { id, name, output, is_error } => AppEvent::ToolEnd { id, name, output, is_error },
            // ...
        }
    }
}
```

---

## 9. 实施 checklist

- [ ] 确认 Rust edition 升级为 2024
- [ ] 确认 reqwest 版本统一为 0.13
- [ ] 确认 nova-core 是独立 crate，不依赖外部.Tauri 相关 API
- [ ] 确认 LlmClient trait 是开放的标准化接口
- [ ] 确认 AgentRuntime.new() 可以接收外部构建的 LlmClient
- [ ] 确认 SessionService 可以外部注入配置
- [ ] 确认 AppEvent → Anchor 转换可以覆盖零需要的全部事件

