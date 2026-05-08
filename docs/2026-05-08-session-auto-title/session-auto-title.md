# Session Auto Title

## 时间
- 创建日期：2026-05-08
- 最后更新：2026-05-08
- 更新说明：补充触发时机、标题生成 API、前端同步机制、幂等性保证等设计细节

## 项目现状
- 当前会话标题在创建时一次性确定：
  - 前端 `deskapp/src/services/chat-service.ts` 会在"首条消息发送前"直接截断用户输入，作为 `sessions.create` 的 `title`。
  - 后端 `crates/nova-agent/src/conversation/service.rs` 在 `create_for_agent` 中把标题固化到 `Session.name` 和 `sessions.title`。
- 现有模型中没有"标题生成状态"：
  - `sessions.title` 只有最终字符串，没有记录是否为默认标题、是否由 AI 生成、已经尝试过几次、上次失败时间等。
  - `ControlState` 适合承载这类轻量会话级运行态，且已有向后兼容反序列化路径。
- 现有网关/前端没有"会话标题已更新"的推送事件：
  - UI 主要依赖 `sessions.list`、`sessions.create.response` 获取标题。
  - 若后台异步改名，前端当前没有可靠的即时刷新机制。

## 整体目标
- 将"标题生成"从前端首条消息截断逻辑迁移到后台异步策略。
- 只有在收集到足够用户语义后才尝试生成标题，避免第一条或第二条消息信息不足。
- 标题生成失败不得影响聊天主流程，不阻塞用户发消息，也不回滚消息保存。
- 支持有限次数重试，优先在第 2 或第 3 条用户消息后触发，避免无意义频繁请求模型。
- 标题一旦生成成功，应立即同步到会话列表、聊天头部等前端视图。
- 标题生成成功后无论前端是否收到通知，后续不再重复生成（幂等保证）。
- 标题生成使用独立的轻量 prompt，不占用主对话模型配额。

## Plan 拆分
- Plan 1：会话标题状态建模与后端生成策略
  - 依赖：无
  - 顺序：1
  - 状态：待开始
- Plan 2：网关协议与前端同步链路
  - 依赖：Plan 1
  - 顺序：2
  - 状态：待开始
- Plan 3：测试补充与回归验证
  - 依赖：Plan 1、Plan 2
  - 顺序：3
  - 状态：待开始

## 风险与待定项
- 风险：标题生成若直接复用主对话模型，可能与当前 turn 争抢配额或拉高延迟，因此应采用异步后台任务，不挂在主响应路径上。
- 风险：如果没有专门的"session updated"事件，前端只能靠轮询或每轮刷新列表，体验和带宽都不理想。
- 风险：旧会话数据没有标题元信息，设计必须保证缺省字段能安全回退，不触发迁移失败。
- 风险：并发场景下，标题生成任务是否需要加锁避免重复生成？
- 风险：标题生成超时后如何处理？超时是否算作一次失败尝试？

## 推荐策略摘要
- 新建会话时统一使用通用占位标题，例如 `New Chat`，不再使用首条用户消息截断结果。
- 后端仅基于"用户消息文本"评估是否生成标题，建议门槛：
  - 用户消息数 `< 2`：不尝试。
  - 用户消息数 `>= 2` 且满足最小有效文本长度：首次尝试。
  - 首次失败后，在用户消息数 `>= 3` 时允许最后一次重试。
- 最大尝试次数建议为 `2`，避免每轮都重复请求模型。
- 标题生成成功后写回 `sessions.title`，并记录 `generated/confirmed` 状态，后续不再覆盖。

## 补充设计细节

### 1. 触发时机（明确为事件驱动）

**触发模式：事件驱动**

标题生成只在**用户消息到达时**触发检查，而不是每轮轮询。具体流程：

1. 用户发送消息 → 消息保存成功
2. 后端检查当前会话的 `title_generation` 状态：
   - 若 `title_status == Generated`：跳过，不重复生成
   - 若 `title_status == Failed` 且 `retry_count >= 2`：跳过
   - 若 `title_status == Pending` 且满足触发条件：启动异步标题生成任务
3. 标题生成任务完成后，更新 `title_generation` 状态

**触发条件细化**：
- 用户消息数 `< 2`：不触发
- 用户消息数 `>= 2` 且 `title_generation.attempt_count == 0`：首次尝试
- 用户消息数 `>= 3` 且 `title_generation.attempt_count == 1` 且 `title_generation.last_failure != null`：重试

### 2. 标题生成 API 设计

**API 签名**：
```rust
async fn generate_session_title(
    session_id: SessionId,
    messages: &[UserMessage],
) -> Result<String, TitleGenerationError>;
```

**输入**：
- 当前会话的所有用户消息文本（或最近 N 条）
- 建议只使用最近 3-5 条用户消息，避免过长上下文

**输出**：
- 成功：标题字符串（建议最大长度 50 字符）
- 失败：`TitleGenerationError` 枚举

**错误类型**：
```rust
enum TitleGenerationError {
    NetworkError,      // 网络错误，应该重试
    Timeout,           // 超时，应该重试
    EmptyResponse,     // 模型返回空标题，不需要重试
    InvalidResponse,   // 模型返回格式错误，不需要重试
}
```

**超时设置**：建议 5 秒超时，超时算作一次失败尝试。

**独立 Prompt**：
- 使用独立的轻量 prompt，例如：
  ```
  根据以下对话内容，生成一个简短的会话标题（10字以内）：
  {messages}
  ```
- 使用独立的模型配置或 endpoint，避免与主对话争抢配额

### 3. 数据结构定义

**ControlState 扩展**：
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleGenerationState {
    pub status: TitleStatus,          // Pending / Generating / Generated / Failed
    pub attempt_count: u32,           // 已尝试次数
    pub last_failure: Option<Instant>, // 上次失败时间
    pub generated_at: Option<Instant>, // 生成成功时间
    pub title: Option<String>,        // 生成的标题
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TitleStatus {
    Pending,      // 等待生成
    Generating,   // 正在生成（防止并发重复）
    Generated,    // 生成成功
    Failed,       // 生成失败（可重试）
}
```

**默认值**：
- 新建会话：`status = Pending, attempt_count = 0`
- 旧会话：`status = Generated, title = sessions.title`（直接复用现有标题）

### 4. 前端同步机制

**推荐方案：WebSocket 推送 + 前端轮询兜底**

1. **主路径**：标题生成成功后，通过 WebSocket 推送 `session_title_updated` 事件
   - 事件内容：`{ session_id, title, generated_at }`
   - 前端收到后更新对应会话的标题

2. **兜底路径**：前端在以下场景主动获取最新标题：
   - 会话列表加载时
   - 收到新消息但标题未更新时
   - 页面可见性变化时（visibilitychange）

3. **标题变更标记**：后端在消息响应中附带 `title_updated: bool` 标记，前端据此决定是否刷新标题

### 5. 幂等性保证

**确保不重复生成的机制**：
1. **状态机约束**：`TitleStatus::Generated` 状态下不再触发生成
2. **生成中锁**：`TitleStatus::Generating` 状态下，即使收到新消息也不重复启动任务
3. **生成任务去重**：使用 `session_id + attempt_count` 作为任务唯一标识，避免并发重复

**并发安全**：
- 标题生成任务启动前，将状态从 `Pending` 更新为 `Generating`（CAS 操作）
- 如果更新失败（已被其他任务抢占），则跳过本次生成

### 6. 降级策略

**降级路径**：
1. 标题生成连续失败 2 次后，停止尝试，保持占位标题
2. 如果标题生成完全不可用（模型服务故障），前端显示占位标题，不阻塞聊天
3. 标题生成超时不算作失败，不增加 `attempt_count`，等待下次用户消息再尝试

### 7. 旧数据兼容

**旧会话处理**：
- 旧会话的 `ControlState` 中 `title_generation` 字段默认为 `None`
- 读取时若为 `None`，则：
  - `status = Generated`
  - `title = sessions.title`（直接复用现有标题）
  - `attempt_count = 0`（不再重新生成）
- 不需要显式迁移逻辑，只需在读取时做 `None` 处理

### 8. 日志与监控

**关键日志点**：
- `INFO`：标题生成任务启动
- `INFO`：标题生成成功，新标题：{title}
- `WARN`：标题生成失败，重试次数：{attempt_count}，错误：{error}
- `ERROR`：标题生成连续失败，停止尝试
- `DEBUG`：标题生成跳过（原因：{reason}）

**监控指标**：
- 标题生成成功率
- 标题生成平均耗时
- 标题生成重试率
- 标题生成超时率
