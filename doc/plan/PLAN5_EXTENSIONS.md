# Plan 5: OpenAI 兼容后端 + Skill 机制

## 目标

扩展 LLM provider 覆盖面（OpenAI / XAI / DashScope），引入 Skill 高阶编排机制。

## 前置

Plan 4 完成。

## 范围

| # | 文件 | 内容 |
|---|------|------|
| 1 | `src/provider/openai_compat.rs` | OpenAI 兼容 LlmClient 实现 |
| 2 | `src/provider/mod.rs` 补充 | ProviderClient 枚举增加 OpenAi / Xai 变体 |
| 3 | `src/skill.rs` | Skill trait + SkillAsToolWrapper |
| 4 | `src/lib.rs` 补充 | 导出 Skill 相关类型 |
| 5 | `src/bin/nova_cli.rs` 补充 | `--model grok-3` 等非 Anthropic 模型支持 |

## 详细设计

### 1. OpenAI 兼容后端

#### 1.1 OpenAiCompatClient

```rust
pub struct OpenAiCompatConfig {
    pub name: String,           // "openai" / "xai" / "dashscope"
    pub base_url: String,
    pub api_key_env: String,    // 环境变量名
    pub default_model: String,
}

impl OpenAiCompatConfig {
    pub fn openai() -> Self;
    pub fn xai() -> Self;
    pub fn dashscope() -> Self;
}

pub struct OpenAiCompatClient {
    http: reqwest::Client,
    api_key: String,
    config: OpenAiCompatConfig,
}

impl OpenAiCompatClient {
    pub fn from_env(config: OpenAiCompatConfig) -> Result<Self>;
    pub fn base_url(&self) -> &str;
}

impl LlmClient for OpenAiCompatClient {
    async fn stream(
        &self,
        messages: &[Message],
        system: &str,
        tools: &[ToolDefinition],
        config: &ModelConfig,
    ) -> Result<Box<dyn StreamReceiver>>;
}
```

#### 1.2 OpenAI wire format 差异处理

OpenAI 的 SSE 格式与 Anthropic 不同，需要单独的解析逻辑：

| 方面 | Anthropic | OpenAI |
|------|-----------|--------|
| 端点 | `/v1/messages` | `/v1/chat/completions` |
| system | 顶层 `system` 字段 | `messages[0].role = "system"` |
| tool_use | `content_block_start` + `input_json_delta` | `tool_calls` 在 delta 中 |
| 结束标志 | `message_stop` event | `data: [DONE]` |
| tool_result | `role: "user"` + `tool_result` block | `role: "tool"` + `tool_call_id` |

SSE parser 复用 `SseParser` 的 frame 分割逻辑，但 JSON 反序列化为 OpenAI 格式的结构体，再转换为统一的 `ProviderStreamEvent`。

#### 1.3 ProviderClient 扩展

```rust
pub enum ProviderClient {
    Anthropic(AnthropicClient),
    OpenAi(OpenAiCompatClient),
    Xai(OpenAiCompatClient),
}

impl ProviderClient {
    /// 根据模型名称自动检测 provider
    pub fn from_model(model: &str) -> Result<Self> {
        match detect_provider(model) {
            Provider::Anthropic => Ok(Self::Anthropic(AnthropicClient::from_env()?)),
            Provider::Xai => Ok(Self::Xai(OpenAiCompatClient::from_env(OpenAiCompatConfig::xai())?)),
            Provider::OpenAi => Ok(Self::OpenAi(OpenAiCompatClient::from_env(OpenAiCompatConfig::openai())?)),
        }
    }
}

impl LlmClient for ProviderClient {
    // 委托到内部 client
}
```

**模型 → provider 检测规则**（参考 claw-code）：
- `claude-*` / `opus` / `sonnet` / `haiku` → Anthropic
- `grok-*` → Xai
- `gpt-*` / `o1-*` / `o3-*` / `o4-*` → OpenAi
- `qwen-*` → OpenAi (DashScope)

### 2. Skill 机制

#### 2.1 Skill trait

```rust
/// Skill: 高阶能力抽象。
/// 与 Tool 的区别：
/// - Tool 是单次原子操作，由 LLM 决定何时调用
/// - Skill 是多步编排，内部可自行调用多个工具
///
/// 对 LLM 来说 Skill 就是一个 Tool（通过 SkillAsToolWrapper 包装）。
/// LLM 调用 Skill 时只提供高层输入（如 "research topic X"），
/// Skill 内部自行编排多个工具调用完成任务。
#[async_trait]
pub trait Skill: Send + Sync {
    /// Skill 名称（作为 Tool name 暴露给 LLM）
    fn name(&self) -> &str;

    /// Skill 描述
    fn description(&self) -> &str;

    /// Skill 的输入参数 schema
    fn input_schema(&self) -> serde_json::Value;

    /// 执行 Skill。
    /// - input: LLM 提供的输入参数
    /// - tools: 可用工具集的引用，Skill 可以调用任意工具
    /// - event_tx: 可选的事件推送通道，用于中间状态汇报
    async fn execute(
        &self,
        input: serde_json::Value,
        tools: &ToolRegistry,
        event_tx: Option<mpsc::Sender<AgentEvent>>,
    ) -> Result<String>;
}
```

#### 2.2 SkillAsToolWrapper

```rust
/// 将 Skill 包装为 Tool，对 LLM 透明
pub struct SkillAsToolWrapper {
    skill: Box<dyn Skill>,
    tools: Arc<ToolRegistry>,   // 共享 tool registry 的引用
    event_tx: Option<mpsc::Sender<AgentEvent>>,
}

#[async_trait]
impl Tool for SkillAsToolWrapper {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.skill.name().to_string(),
            description: Some(self.skill.description().to_string()),
            input_schema: self.skill.input_schema(),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput> {
        match self.skill.execute(input, &self.tools, self.event_tx.clone()).await {
            Ok(output) => Ok(ToolOutput { content: output, is_error: false }),
            Err(e) => Ok(ToolOutput { content: format!("Skill error: {e}"), is_error: true }),
        }
    }
}
```

#### 2.3 示例 Skill：DeepResearch

```rust
/// 深度研究 Skill：搜索 → 抓取多个来源 → 综合整理
pub struct DeepResearchSkill;

#[async_trait]
impl Skill for DeepResearchSkill {
    fn name(&self) -> &str { "deep_research" }

    fn description(&self) -> &str {
        "Conduct in-depth research on a topic by searching the web, \
         fetching multiple sources, and synthesizing findings."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "topic": { "type": "string", "description": "Research topic" },
                "depth": { "type": "integer", "description": "Number of sources to examine (default 3)" }
            },
            "required": ["topic"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        tools: &ToolRegistry,
        event_tx: Option<mpsc::Sender<AgentEvent>>,
    ) -> Result<String> {
        let topic = input["topic"].as_str().unwrap_or("unknown");
        let depth = input["depth"].as_u64().unwrap_or(3) as usize;

        // Step 1: 搜索
        let search_result = tools.execute("web_search", json!({
            "query": topic,
            "count": depth * 2
        })).await?;

        // Step 2: 从搜索结果中提取 URL 并抓取
        let urls = extract_urls(&search_result.content);
        let mut contents = Vec::new();
        for url in urls.into_iter().take(depth) {
            let fetch_result = tools.execute("web_fetch", json!({ "url": url })).await?;
            if !fetch_result.is_error {
                contents.push(fetch_result.content);
            }
        }

        // Step 3: 返回综合内容（由调用 Skill 的 LLM 来做最终综合）
        Ok(format!(
            "Research on \"{topic}\":\n\n\
             Search results:\n{}\n\n\
             Fetched content from {} sources:\n{}",
            search_result.content,
            contents.len(),
            contents.join("\n---\n")
        ))
    }
}
```

#### 2.4 注册 Skill

```rust
// AgentRuntime 新增方法
impl<C: LlmClient> AgentRuntime<C> {
    /// 注册一个 Skill（内部包装为 Tool）
    pub fn register_skill(&mut self, skill: Box<dyn Skill>) {
        let wrapper = SkillAsToolWrapper {
            skill,
            tools: Arc::clone(&self.tools_shared),
            event_tx: None,
        };
        self.tools.register(Box::new(wrapper));
    }
}
```

### 3. nova-cli 补充

```rust
// --model 参数现在支持自动 provider 检测
let client = ProviderClient::from_model(&cli.model)?;

// 或手动指定 base-url
if let Some(url) = &cli.base_url {
    // 使用 OpenAiCompatClient with custom base URL
}
```

使用示例：

```bash
# Anthropic
nova-cli --model claude-sonnet-4-20250514 chat

# OpenAI
nova-cli --model gpt-4o chat

# XAI
nova-cli --model grok-3 chat

# 自定义 endpoint
nova-cli --model my-model --base-url http://localhost:11434/v1 chat
```

## 验证方式

1. OpenAI 兼容后端：用 OpenAI API key 验证 `gpt-4o` 端到端对话
2. Provider 自动检测：验证各模型名称路由到正确的 provider
3. Skill 机制：
   - 注册 DeepResearchSkill
   - LLM 调用 `deep_research` 工具
   - Skill 内部正确编排 web_search → web_fetch 调用链
   - 最终结果返回给 LLM 做综合

## 交付物

- `ProviderClient::from_model("grok-3")` 可用
- `agent.register_skill(Box::new(DeepResearchSkill))` 可用
- nova-cli `--model gpt-4o` / `--model grok-3` 正常工作
