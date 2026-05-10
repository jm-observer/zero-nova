# Plan 1: 标题生成组件建模与配置

## 前置依赖
- 无

## 本次目标
- 新建 `SessionTitleService` 组件，明确标题生成职责归属
- 在 `ControlState` 中扩展标题生成状态字段
- 将标题生成参数提取为可配置项，放入 `AppConfig`

## 涉及文件
- `crates/nova-agent/src/conversation/control.rs` - 扩展 ControlState
- `crates/nova-agent/src/conversation/service.rs` - 会话创建逻辑
- `crates/nova-agent/src/config.rs` - 添加 SessionTitleConfig
- 可能新增：`crates/nova-agent/src/app/session_title_service.rs`

## 详细设计

### 1. 标题状态模型扩展

**在 ControlState 中新增字段：**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlState {
    pub active_agent: String,
    #[serde(default)]
    pub project_dir: Option<PathBuf>,
    #[serde(default)]
    pub model_override: SessionModelOverride,
    #[serde(default)]
    pub last_turn_snapshot: Option<LastTurnSnapshot>,
    #[serde(default)]
    pub skill_bindings: Vec<serde_json::Value>,
    #[serde(default)]
    pub system_prompt_base_override: Option<String>,
    #[serde(default)]
    pub system_prompt_state: SystemPromptState,
    #[serde(default)]
    pub token_counters: SessionTokenCounters,
    // === 新增：标题生成状态 ===
    #[serde(default)]
    pub title_generation: Option<TitleGenerationState>,
}
```

**新增 TitleGenerationState 定义：**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleGenerationState {
    /// 标题来源：default（后端默认）/ ai（AI生成）/ manual（手动修改）
    pub source: TitleSource,
    /// 生成状态：idle / pending / generating / succeeded / failed
    pub status: TitleStatus,
    /// 已尝试次数
    pub attempt_count: u8,
    /// 上次尝试时间
    pub last_attempt_at: i64,
    /// 上次成功时间
    pub last_success_at: Option<i64>,
    /// 上次错误信息
    pub last_error: Option<String>,
    /// 基于的用户消息数量
    pub based_on_user_message_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TitleSource {
    #[serde(rename = "default")]
    Default,
    #[serde(rename = "ai")]
    Ai,
    #[serde(rename = "manual")]
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TitleStatus {
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "generating")]
    Generating,
    #[serde(rename = "succeeded")]
    Succeeded,
    #[serde(rename = "failed")]
    Failed,
}
```

**默认值实现：**

```rust
impl Default for TitleGenerationState {
    fn default() -> Self {
        Self {
            source: TitleSource::Default,
            status: TitleStatus::Idle,
            attempt_count: 0,
            last_attempt_at: 0,
            last_success_at: None,
            last_error: None,
            based_on_user_message_count: 0,
        }
    }
}
```

**数据库兼容策略：**
- 数据库将在部署时删除并重建，无需兼容旧数据
- 不保留旧会话，也不提供旧 `runtime_control` JSON 的向后兼容分支
- 新建会话时 `title_generation` 字段默认为 `None`
- 反序列化时若为 `None`，则使用默认值（`Idle` 状态，`attempt_count = 0`）
- 新会话会正常触发标题生成逻辑

### 2. 配置模型扩展

**在 config.rs 中新增 SessionTitleConfig：**

```rust
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SessionTitleConfig {
    /// 首次尝试最小用户消息数
    #[serde(default = "default_min_messages_first_attempt")]
    pub min_user_messages_first_attempt: usize,
    /// 重试最小用户消息数
    #[serde(default = "default_min_messages_second_attempt")]
    pub min_user_messages_second_attempt: usize,
    /// 最大尝试次数
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u8,
    /// 最小有效字符数
    #[serde(default = "default_min_total_chars")]
    pub min_total_chars: usize,
    /// 标题最大长度（字符）
    #[serde(default = "default_max_title_length")]
    pub max_title_length: usize,
    /// 生成超时时间（毫秒）
    #[serde(default = "default_generation_timeout_ms")]
    pub generation_timeout_ms: u64,
    /// 可选：使用独立模型生成标题
    #[serde(default)]
    pub model_override: Option<crate::config::ModelRef>,
}

fn default_min_messages_first_attempt() -> usize { 2 }
fn default_min_messages_second_attempt() -> usize { 3 }
fn default_max_attempts() -> u8 { 2 }
fn default_min_total_chars() -> usize { 24 }
fn default_max_title_length() -> usize { 50 }
fn default_generation_timeout_ms() -> u64 { 5000 }
```

**在 AppConfig 中添加字段：**

```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    // ... 现有字段 ...
    /// 会话标题生成配置
    #[serde(default)]
    pub session_title: SessionTitleConfig,
}
```

### 3. SessionTitleService 组件设计

**组件职责：**
- 封装标题生成的完整生命周期
- 提供异步生成接口
- 管理标题生成状态转换
- 暴露配置访问接口

**接口定义：**

```rust
pub struct SessionTitleService {
    config: Arc<SessionTitleConfig>,
    llm_client: Arc<dyn LlmClient>,
}

impl SessionTitleService {
    pub fn new(config: Arc<SessionTitleConfig>, llm_client: Arc<dyn LlmClient>) -> Self { ... }
    
    /// 检查是否应该触发标题生成
    pub fn should_generate(&self, state: &TitleGenerationState, user_message_count: usize) -> bool { ... }
    
    /// 异步生成标题
    pub async fn generate(&self, messages: &[UserMessage]) -> Result<String, TitleGenerationError> { ... }
    
    /// 规范化标题长度
    pub fn normalize_title(&self, title: String) -> String { ... }
    
    /// 构建标题生成 prompt
    pub fn build_prompt(&self, messages: &[UserMessage]) -> String { ... }
}
```

**错误类型定义：**

```rust
#[derive(Debug, Clone)]
pub enum TitleGenerationError {
    NetworkError,      // 网络错误，应该重试
    Timeout,           // 超时，应该重试
    EmptyResponse,     // 模型返回空标题，不需要重试
    InvalidResponse,   // 模型返回格式错误，不需要重试
}
```

### 4. 默认标题策略

**后端创建会话时的默认标题：**

```rust
// 在 create_for_agent 中
let session_name = name.unwrap_or_else(|| {
    // 统一使用 "New Chat" 作为默认标题
    "New Chat".to_string()
});
```

**产品决策**：
- 统一使用 "New Chat" 作为默认标题，避免历史数据中 "Session {id_prefix}" 与新产品文案混杂
- 前端创建会话时不再传 title，由后端统一填默认值
- 手动创建会话时，UI 层字符串仅用于默认展示，不作为最终标题

**触发条件实现：**

```rust
impl SessionTitleService {
    pub fn should_generate(&self, state: &TitleGenerationState, user_message_count: usize) -> bool {
        // 已生成或正在生成中，不重复触发
        if state.status == TitleStatus::Succeeded || state.status == TitleStatus::Generating {
            return false;
        }
        
        // 超过最大尝试次数
        if state.attempt_count >= self.config.max_attempts {
            return false;
        }
        
        // 手动修改后不自动覆盖
        if state.source == TitleSource::Manual {
            return false;
        }
        
        // 首次尝试
        if state.attempt_count == 0 && user_message_count >= self.config.min_user_messages_first_attempt {
            return true;
        }
        
        // 重试
        if state.attempt_count == 1 
            && user_message_count >= self.config.min_user_messages_second_attempt 
            && state.last_error.is_some() {
            return true;
        }
        
        false
    }
}
```

## 测试案例
- 正常路径：
  - 新建会话时默认 `title_generation = None`，反序列化后转为 `Idle` 状态
  - 配置加载正常，所有字段有合理默认值
- 边界条件：
  - 数据库重建后新会话数据能正常反序列化
  - 配置中缺少可选字段时使用默认值
- 异常路径：
  - 配置中 `max_attempts = 0` 时不触发任何生成
