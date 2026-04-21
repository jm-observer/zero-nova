# Plan 2：Skill 路由与 Prompt 组装

> 前置依赖：Plan 1（Skill 定义与加载）| 预期产出：`src/skill/router.rs`，`src/prompt.rs` 改造，`AgentRuntime` 接口调整

## 1. 目标

实现基于 **LLM 意图分类** 的 skill 路由，以及基于路由结果动态组装 system prompt 和过滤工具集。

## 2. Skill 路由设计

### 2.1 核心思路

使用一个**小模型（如 Haiku）**作为分类器，在主模型调用之前，先判断用户消息应该匹配哪个 skill。

所有叶子 skill 被**扁平化**为 slug + description 列表传给分类器，不管目录结构有多深。slug 是相对于 `skills/` 根目录的路径（如 `coding/debug/frontend`）。

```
用户消息
   │
   ▼
┌──────────────────────────────────────────┐
│  小模型分类器                               │
│                                            │
│  输入：                                     │
│    system: 分类指令                         │
│    user: 用户消息                           │
│    + 所有叶子 skill 的 slug/desc（扁平列表） │
│                                            │
│  输出：                                     │
│    JSON { "skill": "coding/debug/frontend" }│
│    或   { "skill": null }                   │
└──────────────┬─────────────────────────────┘
               │
               ▼
         按 slug 查找 skill
         加载 prompt → 进入主模型
```

### 2.2 分类器 Prompt 设计

```
你是一个意图分类器。根据用户的消息，判断应该激活哪个 skill。

可用的 skills：
{{#each skills}}
- slug: "{{this.slug}}" — {{this.description}}
{{/each}}

规则：
1. 如果用户消息明确匹配某个 skill 的场景，返回该 skill 的 slug
2. 如果用户消息不匹配任何 skill（如闲聊、通用问答），返回 null
3. 只返回 JSON，不要解释

输出格式（严格 JSON）：
{"skill": "<slug>"}  或  {"skill": null}
```

**实际发送给分类器的请求示例（假设有多层级 skill）：**

```json
{
  "model": "claude-3-5-haiku-latest",
  "max_tokens": 128,
  "system": "你是一个意图分类器。根据用户的消息，判断应该激活哪个 skill。\n\n可用的 skills：\n- slug: \"coding/commit\" — 帮助用户进行代码提交，包括查看变更、生成 commit message、执行 git 操作等\n- slug: \"coding/review\" — 审查代码质量，发现潜在问题并给出改进建议\n- slug: \"coding/debug/frontend\" — 调试前端问题，包括 CSS 布局、JS 报错、React 组件渲染等\n- slug: \"coding/debug/backend\" — 调试后端问题，包括 API 错误、数据库查询、性能瓶颈等\n- slug: \"writing/blog\" — 撰写博客文章\n- slug: \"writing/docs\" — 编写技术文档\n- slug: \"tech-consult\" — 回答技术选型、架构设计等技术咨询类问题\n\n规则：\n1. 如果用户消息明确匹配某个 skill 的场景，返回该 skill 的 slug\n2. 如果用户消息不匹配任何 skill（如闲聊、通用问答），返回 null\n3. 只返回 JSON，不要解释\n\n输出格式（严格 JSON）：\n{\"skill\": \"<slug>\"}  或  {\"skill\": null}",
  "messages": [
    { "role": "user", "content": "前端页面的按钮点击没反应，帮我看看" }
  ]
}
```

**预期输出：**
```json
{"skill": "coding/debug/frontend"}
```

**其他示例：**

| 用户输入 | 预期输出 |
|---------|---------|
| "帮我把代码提交一下" | `{"skill": "coding/commit"}` |
| "这段 API 响应好慢，帮我看看" | `{"skill": "coding/debug/backend"}` |
| "帮我写个博客介绍这个功能" | `{"skill": "writing/blog"}` |
| "今天天气怎么样" | `{"skill": null}` |

### 2.3 `src/skill/router.rs`

```rust
use super::definition::SkillDefinition;
use super::registry::SkillRegistry;
use crate::provider::{LlmClient, ModelConfig};
use anyhow::Result;
use serde::Deserialize;

/// 分类器的 JSON 输出结构
#[derive(Debug, Deserialize)]
struct ClassifierOutput {
    skill: Option<String>,
}

/// 路由结果
#[derive(Debug)]
pub struct RouteResult<'a> {
    /// 匹配到的 skill，None 表示无匹配
    pub skill: Option<&'a SkillDefinition>,
}

/// 基于 LLM 的 Skill 路由器
pub struct SkillRouter {
    /// 分类器使用的 system prompt（启动时根据已加载 skill 生成）
    classifier_prompt: String,
    /// 分类器模型配置
    classifier_config: ModelConfig,
}

impl SkillRouter {
    /// 根据已加载的叶子 skill 列表构建分类器 prompt
    ///
    /// 所有 skill 被扁平化为 slug + description 列表，
    /// slug 可能包含路径（如 "coding/debug/frontend"）。
    pub fn new(skills: &[SkillDefinition], classifier_config: ModelConfig) -> Self {
        let skills_desc: String = skills
            .iter()
            .map(|s| format!("- slug: \"{}\" — {}", s.slug, s.description))
            .collect::<Vec<_>>()
            .join("\n");

        let classifier_prompt = format!(
            r#"你是一个意图分类器。根据用户的消息，判断应该激活哪个 skill。

可用的 skills：
{skills_desc}

规则：
1. 如果用户消息明确匹配某个 skill 的场景，返回该 skill 的完整 slug
2. 如果用户消息不匹配任何 skill（如闲聊、通用问答），返回 null
3. 只返回 JSON，不要解释

输出格式（严格 JSON）：
{{"skill": "<slug>"}}  或  {{"skill": null}}"#
        );

        Self {
            classifier_prompt,
            classifier_config,
        }
    }

    /// 对用户输入进行 skill 路由
    ///
    /// 调用小模型分类器，解析 JSON 输出，返回匹配的 skill。
    /// 分类失败（网络错误、JSON 解析失败等）时静默降级为无匹配。
    pub async fn route<'a>(
        &self,
        input: &str,
        registry: &'a SkillRegistry,
        classifier_client: &dyn LlmClient,
    ) -> RouteResult<'a> {
        match self.classify(input, classifier_client).await {
            Ok(Some(slug)) => {
                let skill = registry.find_by_slug(&slug);
                if skill.is_none() {
                    log::warn!(
                        "Classifier returned unknown skill slug: {}",
                        slug
                    );
                }
                RouteResult { skill }
            }
            Ok(None) => {
                log::debug!("Classifier: no skill matched");
                RouteResult { skill: None }
            }
            Err(e) => {
                log::warn!(
                    "Skill classification failed, falling back to no-skill: {}",
                    e
                );
                RouteResult { skill: None }
            }
        }
    }

    /// 内部：调用分类器 LLM 并解析结果
    async fn classify(
        &self,
        input: &str,
        client: &dyn LlmClient,
    ) -> Result<Option<String>> {
        let messages = vec![crate::message::Message {
            role: crate::message::Role::User,
            content: vec![crate::message::ContentBlock::Text {
                text: input.to_string(),
            }],
        }];

        // 调用小模型，不传 tools
        let mut receiver = client
            .stream(&messages, &self.classifier_prompt, &[], &self.classifier_config)
            .await?;

        // 收集完整响应文本
        let mut response_text = String::new();
        while let Some(event) = receiver.next_event().await? {
            if let crate::provider::ProviderStreamEvent::TextDelta(delta) = event {
                response_text.push_str(&delta);
            }
        }

        // 解析 JSON
        let trimmed = response_text.trim();
        // 容错：提取 JSON 部分（LLM 可能附带额外文字）
        let json_str = extract_json(trimmed).unwrap_or(trimmed);

        let output: ClassifierOutput = serde_json::from_str(json_str)
            .map_err(|e| anyhow::anyhow!(
                "Failed to parse classifier output: {}. Raw: {}",
                e, trimmed
            ))?;

        Ok(output.skill)
    }
}

/// 从文本中提取第一个 JSON 对象
fn extract_json(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')? + 1;
    if start < end {
        Some(&text[start..end])
    } else {
        None
    }
}
```

### 2.4 路由边界情况与容错

| 场景 | 处理 |
|------|------|
| 分类器返回不存在的 slug | 记录 warn 日志，降级为无 skill |
| 分类器返回非 JSON 格式 | 尝试提取 JSON 子串，失败则降级为无 skill |
| 分类器网络超时/错误 | catch 错误，降级为无 skill，不阻断主流程 |
| 分类器返回 `{"skill": null}` | 正常，使用 base prompt |
| skill 列表为空 | 跳过分类器调用，直接返回无 skill |

### 2.5 性能特征

| 指标 | 预期值 |
|------|--------|
| 分类器调用延迟 | 200-500ms（Haiku 级别小模型） |
| 分类器 token 消耗 | 输入约 200-500 tokens（取决于 skill 数量），输出约 10-20 tokens |
| 主流程影响 | 每次用户消息增加一次 LLM 调用 |

### 2.6 跳过分类的优化

以下场景可以跳过分类器调用：

```rust
/// 判断是否需要调用分类器
fn should_classify(input: &str, registry: &SkillRegistry) -> bool {
    // 1. 没有加载任何 skill，直接跳过
    if registry.skills().is_empty() {
        return false;
    }

    // 2. 输入过短（如 "好"、"ok"），不太可能触发 skill 切换
    // 但不做这个优化，因为短输入也可能是确认类操作
    // 留给分类器判断

    true
}
```

## 3. SystemPromptBuilder 改造

### 3.1 当前问题

现有 `SystemPromptBuilder::new()` 使用 `include_str!("../prompts/default.md")` 在**编译时**嵌入 prompt。skill 系统需要运行时动态加载基础 prompt。

### 3.2 改造方案

```rust
// src/prompt.rs

pub struct SystemPromptBuilder {
    sections: Vec<String>,
}

impl SystemPromptBuilder {
    /// 从指定的基础 prompt 内容创建 builder
    pub fn from_base(base_prompt: &str) -> Self {
        let mut builder = Self { sections: Vec::new() };
        let trimmed = base_prompt.trim();
        if !trimmed.is_empty() {
            builder.sections.push(trimmed.to_string());
        }
        builder
    }

    /// 保持向后兼容：使用编译时嵌入的 default.md
    pub fn new() -> Self {
        let default_content = include_str!("../prompts/default.md");
        Self::from_base(default_content)
    }

    /// 注入 skill 的行为提示词
    pub fn with_skill(mut self, skill: &SkillDefinition) -> Self {
        let skill_section = format!(
            "# Active Skill: {}\n\n{}\n\n{}",
            skill.name,
            skill.description,
            skill.prompt
        );
        self.sections.push(skill_section);
        self
    }

    // ... 其他现有方法保持不变 ...
}
```

### 3.3 Prompt 组装流程

```rust
/// 根据 skill 路由结果构建完整的 system prompt
fn build_system_prompt_for_turn(
    base_prompt: &str,
    skill: Option<&SkillDefinition>,
) -> String {
    let mut builder = SystemPromptBuilder::from_base(base_prompt);

    if let Some(skill) = skill {
        builder = builder.with_skill(skill);
    }

    builder.build()
}
```

**注意：不再默认调用 `with_tools()`。** 工具描述通过 Anthropic API 的 `tools` 参数传递，不需要在 system prompt 中重复。

## 4. 工具过滤

### 4.1 ToolRegistry 新增方法

```rust
// src/tool.rs

impl ToolRegistry {
    /// 返回被指定名称过滤后的工具定义列表
    pub fn tool_definitions_filtered(
        &self,
        allowed_names: &[String],
    ) -> Vec<crate::provider::types::ToolDefinition> {
        if allowed_names.is_empty() {
            // 空白名单 = 不限制，返回全部
            return self.tool_definitions();
        }

        self.tools
            .iter()
            .filter(|t| {
                let name = t.definition().name;
                allowed_names.iter().any(|a| a == &name)
            })
            .map(|t| {
                let d = t.definition();
                crate::provider::types::ToolDefinition {
                    name: d.name,
                    description: d.description,
                    input_schema: d.input_schema,
                }
            })
            .collect()
    }
}
```

## 5. AgentRuntime 接口调整

### 5.1 核心改动：`run_turn()` 接受动态参数

当前 `run_turn()` 使用 `self.system_prompt` 和 `self.tools.tool_definitions()`。需要改为支持每轮动态传入。

**方案：引入 `TurnContext` 结构体**

```rust
/// 单轮对话的动态上下文
pub struct TurnContext {
    /// 本轮使用的 system prompt（已组装完成）
    pub system_prompt: String,
    /// 本轮可用的工具定义（已过滤）
    pub tool_definitions: Vec<crate::provider::types::ToolDefinition>,
    /// 本轮使用的历史消息（可能经过摘要压缩）
    pub history: Vec<Message>,
    /// 活跃的 skill 名称（用于日志和事件通知）
    pub active_skill: Option<String>,
}
```

```rust
impl<C: LlmClient> AgentRuntime<C> {
    /// 新签名：接受 TurnContext
    pub async fn run_turn(
        &self,
        ctx: &TurnContext,
        user_input: &str,
        event_tx: mpsc::Sender<crate::event::AgentEvent>,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<Vec<Message>> {
        let mut all_messages = ctx.history.clone();
        all_messages.push(Message { /* user_input */ });

        // ...
        let mut receiver = self
            .client
            .stream(
                &all_messages,
                &ctx.system_prompt,        // ← 动态 prompt
                &ctx.tool_definitions[..], // ← 过滤后的工具
                &self.config.model_config,
            )
            .await?;
        // ...
    }
}
```

### 5.2 向后兼容

保留旧签名作为便捷方法，内部构造 `TurnContext`：

```rust
impl<C: LlmClient> AgentRuntime<C> {
    /// 便捷方法：使用默认的 system prompt 和全部工具
    pub async fn run_turn_simple(
        &self,
        history: &[Message],
        user_input: &str,
        event_tx: mpsc::Sender<crate::event::AgentEvent>,
        cancellation_token: Option<CancellationToken>,
    ) -> Result<Vec<Message>> {
        let ctx = TurnContext {
            system_prompt: self.system_prompt.clone(),
            tool_definitions: self.tools.tool_definitions(),
            history: history.to_vec(),
            active_skill: None,
        };
        self.run_turn(&ctx, user_input, event_tx, cancellation_token).await
    }
}
```

## 6. 分类器 LLM Client 设计

### 6.1 复用 vs 独立

分类器和主模型都走 `LlmClient` trait，但需要不同的 `ModelConfig`（不同模型、不同 max_tokens）。

**方案：复用同一个 client 实例，使用不同 config 调用。**

现有的 `LlmClient::stream()` 已经接受 `&ModelConfig` 参数，不需要创建第二个 client。只需要准备一份分类器专用的 `ModelConfig`：

```rust
// 在 start_server() 中
let classifier_config = ModelConfig {
    model: config.skill.classifier.model.clone(),
    max_tokens: config.skill.classifier.max_tokens,
    temperature: Some(0.0),  // 分类任务用确定性输出
    top_p: None,
};

let skill_router = SkillRouter::new(
    skill_registry.skills(),
    classifier_config,
);
```

### 6.2 配置

```toml
[skill.classifier]
model = "claude-3-5-haiku-latest"   # 小模型
max_tokens = 128                     # 分类输出很短
```

如果 `[skill.classifier]` 未配置，使用以下默认值：
- `model`: 取主模型配置的值（降级使用主模型做分类）
- `max_tokens`: 128

## 7. handle_chat 集成

### 7.1 改造后的 `handle_chat()`

```rust
// src/gateway/router.rs handle_chat() 关键改动

async fn handle_chat<C: LlmClient>(
    payload: ChatPayload,
    state: Arc<AppState<C>>,
    outbound_tx: mpsc::UnboundedSender<GatewayMessage>,
    request_id: String,
) {
    // ... session 获取逻辑不变 ...

    // ======== 新增：Skill 路由（LLM 分类） ========
    let route_result = state.skill_router.route(
        &payload.input,
        &state.skills,
        &state.classifier_client,  // 复用同一个 LlmClient，不同 config
    ).await;

    if let Some(skill) = route_result.skill {
        log::info!("Skill classified: {}", skill.name);
    }

    // ======== 新增：检测 skill 切换 ========
    let previous_skill = session.active_skill.read().unwrap().clone();
    let current_skill_slug = route_result.skill.map(|s| s.slug.clone());
    let skill_switched = previous_skill != current_skill_slug;

    // ======== 新增：组装 system prompt ========
    let system_prompt = build_system_prompt_for_turn(
        &state.base_prompt,
        route_result.skill,
    );

    // ======== 新增：过滤工具集 ========
    let tool_defs = match route_result.skill {
        Some(skill) => state.agent.tools()
            .tool_definitions_filtered(&skill.tool_constraint.allowed),
        None => state.agent.tools().tool_definitions(),
    };

    // ======== 新增：处理历史消息（Plan 3 内容） ========
    let history = prepare_history(&session, skill_switched);

    // ======== 构造 TurnContext ========
    let ctx = TurnContext {
        system_prompt,
        tool_definitions: tool_defs,
        history,
        active_skill: current_skill_slug.clone(),
    };

    // ======== 调用 agent ========
    match state.agent.run_turn(&ctx, &payload.input, event_tx, None).await {
        Ok(new_messages) => {
            // 更新活跃 skill
            *session.active_skill.write().unwrap() = current_skill_slug;
            // 更新历史
            // ...
        }
        Err(e) => { /* ... */ }
    }
}
```

## 8. Session 扩展

```rust
// src/gateway/session.rs

pub struct Session {
    pub id: String,
    pub name: String,
    pub history: RwLock<Vec<Message>>,
    pub created_at: i64,
    pub chat_lock: Mutex<()>,
    pub active_skill: RwLock<Option<String>>,  // 新增：当前活跃 skill slug
}
```

## 9. AppState 扩展

```rust
// src/gateway/router.rs

pub struct AppState<C: LlmClient> {
    pub agent: AgentRuntime<C>,
    pub sessions: SessionStore,
    pub skills: SkillRegistry,         // 新增
    pub skill_router: SkillRouter,     // 新增
    pub base_prompt: String,           // 新增：base.md 内容
}
```

**注意：** `SkillRouter` 内部持有分类器 prompt 和 config，调用时需要传入 `LlmClient`。由于 `AgentRuntime` 已经持有了 `client`，可以通过 `AppState` 中暴露 client 引用，或让 `SkillRouter` 也持有 client 引用。

具体实现时建议将 client 从 `AgentRuntime` 中独立出来，作为 `AppState` 的顶层字段：

```rust
pub struct AppState<C: LlmClient> {
    pub client: Arc<C>,               // LLM client（分类器和主模型共用）
    pub agent: AgentRuntime<C>,
    pub sessions: SessionStore,
    pub skills: SkillRegistry,
    pub skill_router: SkillRouter,
    pub base_prompt: String,
}
```

## 10. 实施步骤

| 步骤 | 操作 | 涉及文件 |
|------|------|----------|
| 1 | 实现 `src/skill/router.rs`（LLM 分类器） | `src/skill/router.rs`, `src/skill/mod.rs` |
| 2 | 改造 `SystemPromptBuilder`，新增 `from_base()` 和 `with_skill()` | `src/prompt.rs` |
| 3 | `ToolRegistry` 新增 `tool_definitions_filtered()` | `src/tool.rs` |
| 4 | 定义 `TurnContext`，改造 `AgentRuntime::run_turn()` | `src/agent.rs` |
| 5 | `Session` 新增 `active_skill` 字段 | `src/gateway/session.rs` |
| 6 | `AppState` 扩展，`client` 独立为顶层字段 | `src/gateway/router.rs` |
| 7 | `config.rs` 新增 `ClassifierConfig` | `src/config.rs` |
| 8 | 改造 `handle_chat()` 集成路由逻辑 | `src/gateway/router.rs` |
| 9 | 改造 `start_server()` 初始化 router 和 base prompt | `src/gateway/mod.rs` |

## 11. 验证标准

- [ ] 输入 `帮我把改动提交一下` 能通过分类器匹配到 commit skill
- [ ] 输入 `今天天气怎么样` 分类器返回 null，使用 base prompt
- [ ] 分类器超时或出错时，静默降级为无 skill，不影响主流程
- [ ] skill 匹配后，system prompt 包含 base + skill prompt 内容
- [ ] skill 的工具白名单生效，LLM 只看到允许的工具
- [ ] 空白名单的 skill 可以使用全部工具
- [ ] 不同消息可以切换不同 skill，prompt 正确替换
- [ ] skill 列表为空时跳过分类器调用
- [ ] 分类器返回不存在的 slug 时降级为无 skill
