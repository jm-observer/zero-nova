# Session Auto Title (修订版)

## 时间
- 创建日期：2026-05-09
- 最后更新：2026-05-09
- 更新说明：基于代码调研重新评估，明确标题生成组件归属和配置方式；修复字段命名、超时语义、兼容策略等冲突；数据库不再需要兼容（部署时删掉重建）

## 项目现状

### 1. 当前标题生成位置

**前端主导（chat-service.ts）：**
- 文件：`deskapp/src/services/chat-service.ts`
- 第 82-89 行：当发送消息且没有当前会话时，前端直接截取用户输入作为标题：
  ```typescript
  if (!this.state.currentSessionId) {
      const title = text.length > 20 ? text.substring(0, 20) + '...' : text;
      const session = await this.client.createSession({ title, agentId });
  }
  ```
- 第 46-59 行：手动创建会话时使用 `payload?.title || 'New Chat'`

**后端被动接受（application.rs）：**
- 文件：`crates/nova-agent/src/app/application.rs`
- 第 238-286 行：`create_session` 接受 `title: Option<String>`，传递给 `create_for_agent`
- 返回 `AppSession` 时 `title: Some(name)`，其中 `name` 来自 `session.name`

**后端会话创建（service.rs）：**
- 文件：`crates/nova-agent/src/conversation/service.rs`
- 第 58-67 行：`create_for_agent` 使用传入的 title 或默认值 `"Session {id_prefix}"`
- 标题在创建时**一次性确定**，没有后续生成机制

### 2. 现有数据模型

**ControlState（control.rs）：**
- 当前**没有**"标题生成状态"字段
- 包含：active_agent, project_dir, model_override, last_turn_snapshot, skill_bindings, system_prompt_base_override, system_prompt_state, token_counters
- 适合承载轻量会话级运行态；本方案采用数据库重建，不要求旧数据反序列化兼容

**数据库（sqlite_manager.rs）：**
- `sessions.title` 为 `TEXT NOT NULL` 字段
- 没有单独的标题生成元信息表

### 3. 现有事件机制

**GatewayClient（gateway-client.ts）：**
- 已有多种事件类型：token, complete, tool_start, tool_result, iteration, system_log, orchestration_plan 等
- **没有**专门的 "session_title_updated" 事件
- 前端主要依赖 `sessions.list`、`sessions.create.response` 获取标题

### 4. 配置现状

**当前配置方式：**
- 标题生成逻辑**硬编码在前端**（chat-service.ts 第 84 行）
- 后端 `create_for_agent` 使用固定默认值 `"Session {id_prefix}"`
- 没有集中配置项控制：
  - 触发时机（消息数阈值）
  - 最大重试次数
  - 标题长度限制
  - 生成超时时间

## 整体目标

### 核心变更
1. **标题生成职责迁移**：从前端首条消息截断逻辑迁移到**后端异步策略**
2. **明确生成组件**：在后端新增 `SessionTitleService` 组件负责标题生成
3. **配置驱动**：将触发条件、重试策略、超时等参数提取为可配置项

### 设计目标
- 只有在收集到足够用户语义后才尝试生成标题，避免第一条或第二条消息信息不足
- 标题生成失败不得影响聊天主流程，不阻塞用户发消息，也不回滚消息保存
- 支持有限次数重试，优先在第 2 或第 3 条用户消息后触发
- 标题生成使用独立的轻量 prompt，不占用主对话模型配额
- 标题一旦生成成功，应立即同步到会话列表、聊天头部等前端视图

## Plan 拆分

### Plan 1：标题生成组件建模与配置
- **依赖**：无
- **顺序**：1
- **状态**：待开始
- **描述**：新建 `SessionTitleService` 组件，定义标题生成状态模型，提取配置项

### Plan 2：后端触发策略与生成逻辑
- **依赖**：Plan 1
- **顺序**：2
- **状态**：待开始
- **描述**：实现事件驱动的标题生成触发机制，集成异步生成执行

### Plan 3：网关协议与前端同步链路
- **依赖**：Plan 2
- **顺序**：3
- **状态**：待开始
- **描述**：新增标题更新事件，清理前端现有标题策略

### Plan 4：测试补充与回归验证
- **依赖**：Plan 1、Plan 2、Plan 3
- **顺序**：4
- **状态**：待开始
- **描述**：为自动标题需求补足后端单测、集成测试与前端状态测试

## 风险与待定项

### 风险
1. **模型选择**：标题生成应使用独立模型还是复用主对话模型？独立模型避免争抢配额但增加配置复杂度
2. **配置管理**：标题生成参数应放在 `AppConfig` 还是单独的配置模块？
3. **数据库重建窗口**：部署时删除并重建数据库，需明确会话数据清理与回填策略
4. **并发场景**：标题生成任务是否需要加锁避免重复生成？
5. **超时处理**：标题生成超时后如何处理？超时算作一次失败尝试（增加 attempt_count）

### 待定项
1. 标题生成的具体 prompt 模板
2. 标题生成是否支持手动触发（用户主动请求重新生成）
3. 前端是否需要显示"标题生成中"状态

## 推荐策略摘要

### 标题生成组件归属
- **生成组件**：后端 `SessionTitleService`（新建）
- **触发组件**：后端 `ConversationService`（在用户消息到达时检查）
- **同步组件**：前端 `GatewayClient` + `AppState`（接收标题更新事件）

### 配置建议
```rust
pub struct SessionTitleConfig {
    pub min_user_messages_first_attempt: usize,  // 首次尝试最小消息数
    pub min_user_messages_second_attempt: usize, // 重试最小消息数
    pub max_attempts: u8,                        // 最大尝试次数
    pub min_total_chars: usize,                  // 最小有效字符数
    pub max_title_length: usize,                 // 标题最大长度
    pub generation_timeout_ms: u64,              // 生成超时时间
    pub model_override: Option<ModelRef>,        // 可选：使用独立模型
}
```

### 触发条件细化
- 用户消息数 `< 2`：不触发
- 用户消息数 `>= 2` 且 `title_generation.attempt_count == 0`：首次尝试
- 用户消息数 `>= 3` 且 `title_generation.attempt_count == 1` 且 `title_generation.last_error.is_some()`：重试

### 前端清理
- `ChatService.sendMessage` 在"当前无 session"时创建会话，**不再**执行标题截断逻辑
- 改为创建占位标题 session，例如直接不传 `title`，由后端统一填默认值（"New Chat"）
