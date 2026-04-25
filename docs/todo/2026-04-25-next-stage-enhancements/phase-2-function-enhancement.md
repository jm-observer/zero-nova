# Phase 2: 功能增强 - 详细设计

> 日期：2026-04-25
> 范围：Skill 深化、MCP 扩展、向量搜索、多 Provider

---

## 背景

Phase 2 聚焦于深化已有功能并引入关键增强，确保系统从"能用"向"好用"演进。

---

## 任务清单

### 2.1 Skill System 深化

#### 2.1.1 Sticky 机制实现

**背景：** Skill 激活后需要保持上下文一致性，避免被其他工具打断。

**设计决策：**
- 使用 `AgentRuntime.sticky_skill: Option<String>` 跟踪当前粘滞 Skill
- Sticky 期间只允许:
  - 同一 Skill 的子操作
  - 明确的退出信号 `<skill_exit/>`
  - 用户中断（Stop/Cancel）
- 非 Sticky 模式 Skill 行为不变

**实现方案：**

```rust
// crates/nova-core/src/agent.rs

pub struct AgentRuntime {
    // ... 现有字段 ...
    pub sticky_skill: Option<String>,
}

impl AgentRuntime {
    /// 激活 Skill 并进入 Sticky 模式
    pub fn activate_sticky(&mut self, skill_id: &str) {
        self.sticky_skill = Some(skill_id.to_string());
    }

    /// 检查是否允许执行工具（Sticky 期间过滤）
    pub fn can_execute_tool(&self, tool_name: &str, skill_context: &str) -> bool {
        match &self.sticky_skill {
            Some(current) => {
                tool_name == current || skill_context.contains(current.as_str())
            }
            None => true,
        }
    }

    /// 处理 Skill 退出信号
    pub fn exit_sticky(&mut self) {
        self.sticky_skill = None;
    }
}
```

**Skill 退出信号处理：**
```rust
// 在 content block 中识别 `<skill_exit/>` 标签
if content.contains("<skill_exit/>") {
    runtime.exit_sticky();
    // 可选：记录 sticky 持续时间
}
```

**测试场景：**
1. Skill 激活 → 执行子操作 → 正常完成
2. Skill 激活 → 收到非 Sticky 工具 → 自动拒绝/推迟
3. Skill 激活 → 收到 `<skill_exit/>` → 正常退出
4. 用户中断 → Skill 清理 → 恢复原状

---

#### 2.1.2 LLM 分类路由（Phase 4a 升级）

**背景：** 当前 Skill 路由使用纯规则匹配（`skill.rs:537`），缺少语义理解。

**设计决策：**
- 添加 `use_llm_skill_router` 配置项
- LLM 路由回退到规则匹配
- 使用现有 `OpenAiCompatClient` 进行意图分类

**实现方案：**

```rust
// crates/nova-core/src/skill.rs

pub struct SkillRouter {
    llm_client: Option<Arc<dyn LlmClient>>,
    system_prompt: String,
}

impl SkillRouter {
    pub async fn route_skill(&self, user_message: &str) -> Option<String> {
        // 1. 构建路由提示词
        let prompt = format!(
            "Given this user message, determine which skill to activate:\n{}\n\nCurrent skills:\n{}",
            user_message,
            self.available_skills_as_text()
        );

        // 2. 调用 LLM 获取分类结果
        let result = self.llm_client
            .generate(&prompt)
            .await?;

        // 3. 解析 JSON 响应提取 skill_id
        parse_skill_selection(&result)
    }
}
```

**配置项：**
```toml
[gateway.skill_routing]
enabled = true
use_llm_classification = true
llm_model = "gpt-4"  # 或自定义模型
```

---

### 2.2 向量语义搜索

#### 2.2.1 SQLite 向量扩展集成

**设计决策：**
- 使用 `sqlite-vss` 扩展（C 绑定轻量级）
- 或纯 Rust `libsql-vss` 作为替代
- 向量维度：768（OpenAI ada-002 兼容）

**数据库变更：**
```sql
-- 新增向量表
CREATE VIRTUAL TABLE vectors USING vss(v1);

-- 消息向量关联
INSERT INTO vectors VALUES (?1, ?2);  -- message_id, vector_blob

-- 相似度搜索
SELECT message_id FROM vectors WHERE vss_distance(vector, ?, 'l2') < ?;
```

**实现方案：**

```rust
// crates/nova-conversation/src/vector.rs

pub struct VectorStore {
    db: rusqlite::Connection,
    dimension: usize,
}

impl VectorStore {
    pub fn search(&self, query_vector: &[f32], limit: usize) -> Result<Vec<(String, f32)>, String> {
        // SQL 查询返回 top-k 相似消息
    }

    pub fn insert(&self, message_id: &str, vector: &[f32]) -> Result<(), String> {
        // 插入向量到 VSS 表
    }
}
```

**向量生成：**
```rust
// crates/nova-core/src/vector_generator.rs

pub struct VectorGenerator {
    llm_client: Arc<dyn LlmClient>,
}

impl VectorGenerator {
    pub async fn generate_text_vector(&self, text: &str) -> Result<Vec<f32>, String> {
        // 调用 embedding API
        // 支持 OpenAI ada-002 或本地模型
    }
}
```

---

#### 2.2.2 Memory/Distillation 前端视图

**前端组件设计：**
```
src/ui/
├── memory-view.ts      # 记忆管理视图
└── distillation-view.ts # 记忆蒸馏视图
```

**API 扩展：**
```typescript
// gateway-client.ts

interface MemorySearchResult {
    message: Message;
    distance: number;
    score: number;
}

async memorySearch(query: string, limit: number): Promise<MemorySearchResult[]> {
    return this.request('memory.search', { query, limit });
}

async getMemoryGraph(sessionId: string): Promise<MemoryNode[]> {
    return this.request('memory.graph', { sessionId });
}
```

---

### 2.3 MCP 协议扩展

#### 2.3.1 完整通知流支持

**缺失特性：**
- `notifications/resources/update*`
- `notifications/sampling/createMessage`
- 工具变更通知

**实现方案：**

```rust
// crates/nova-core/src/mcp/notifications.rs

pub trait NotificationStream: Send {
    fn on_resource_update(&self, resource: String);
    fn on_tool_change(&self, tools: Vec<String>);
    fn send_sampling_request(
        &self,
        model: &str,
        messages: &[String],
    ) -> Result<Option<String>, String>;
}

pub struct DefaultNotificationStream {
    tx: tokio::sync::mpsc::Sender<JsonRpcNotification>,
}

impl NotificationStream for DefaultNotificationStream {
    fn on_resource_update(&self, resource: String) {
        let notification = JsonRpcNotification {
            method: "notifications/resources/update".to_string(),
            params: json!({ "resource": resource }),
        };
        let _ = self.tx.try_send(notification.into());
    }
}
```

---

#### 2.3.2 类型化 ServerCapabilities

**当前问题：** `ServerCapabilities` 使用 `Option<Value>`

```rust
// 当前
pub struct ServerCapabilities {
    pub tools: Option<Value>,
    pub resources: Option<Value>,
    pub prompts: Option<Value>,
}

// 改进
pub struct ServerCapabilities {
    pub tools: Option<ToolCapability>,
    pub resources: Option<ResourceCapability>,
    pub prompts: Option<PromptCapability>,
}

#[derive(Serialize, Deserialize)]
pub struct ToolCapability {
    pub list_changed: Option<bool>,
}

#[derive(Serialize, Deserialize)]
pub struct ResourceCapability {
    pub subscribe: Option<bool>,
}
```

---

### 2.4 多 Provider 支持

#### 2.4.1 LLM Provider 管理

**设计决策：**
- 支持按 Agent/Prompt 配置不同 LLM
- Provider 切换通过路由层实现
- 统一 Input/Output 归一化

**实现方案：**

```rust
// crates/nova-core/src/provider/mod.rs

pub enum ProviderRegistry {
    Active(String), // 当前活跃 provider
}

impl ProviderRegistry {
    pub fn get_client(&self, agent_id: &str) -> Result<Arc<dyn LlmClient>, String> {
        // 根据 agent 配置选择 provider
        // 回退到默认 provider
    }
}
```

**配置扩展：**
```toml
[[llm.providers]]
name = "anthropic"
base_url = "https://api.anthropic.com"
api_key = "${ANTHROPIC_API_KEY}"
model = "claude-sonnet-4-20250514"

[[llm.providers]]
name = "openai"
base_url = "https://api.openai.com/v1"
model = "gpt-4o"

[gateway.agents]
[[gateway.agents]]
id = "nova"
provider = "anthropic"  # 新增字段
```

---

## 测试计划

| 测试类型 | 范围 | 方法 |
|----------|------|------|
| 单元测试 | Skill Sticky/Routing | `#[tokio::test]` |
| 集成测试 | Vector Search | SQLite 集成 |
| E2E 测试 | MCP 通知流 | WebSocket 模拟 |
| 性能测试 | LLM 分类延迟 | Benchmark |

---

## 风险评估

1. **SQLite-VSS 编译依赖** - 需要 Vulkan SDK 或单独链接 libjson-c
2. **LLM 分类成本** - 每次中断时调用 LLM 会产生额外 token 费用
3. **向量存储扩展性** - 高频写入时需注意 VSS 索引刷新策略
