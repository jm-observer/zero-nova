# Plan 3: 协议暴露、UI 展示与成本估算扩展位

## 前置依赖
Plan 1、Plan 2

## 本次目标
把 token 统计从"后端内部数字"提升为前后端都能消费的 observability 能力。具体包括：统一协议命名、扩展 session 总览和轮次明细接口、利用已有的服务端推送事件、前端展示具体 token 消耗和 provider 原始数据、以及可选的成本估算展示。

## 涉及文件
- `crates/nova-protocol/src/observability.rs` — 协议结构体定义
- `crates/nova-protocol/src/envelope.rs` — 消息 envelope 定义（新增 detail 接口、统一命名）
- `crates/nova-protocol/src/schema.rs` — schema 导出
- `crates/nova-gateway-core/src/handlers/sessions.rs` — handler 实现
- `crates/nova-gateway-core/src/router.rs` — 路由新增
- `crates/nova-agent/src/app/agent_workspace_service.rs` — 业务方法实现（总览、明细、RunRecord usage 填充）
- `crates/nova-agent/src/app/conversation_service.rs` — turn 完成后推送 usage 更新事件
- `deskapp/src/core/types.ts` — 前端 TypeScript 类型
- `deskapp/src/gateway-client.ts` — 前端 gateway 客户端（订阅事件、废弃客户端拼凑逻辑）
- `deskapp/src/ui/agent-console-view.ts` — 前端 token 展示 UI

## 详细设计

### 1. 协议命名统一

当前不一致的状态：
- 请求：`sessions.token_usage`（下划线，`envelope.rs:191`）
- 更新事件：`sessions.token.usage`（点号，`envelope.rs:195`）

统一为点号风格：

| 协议消息 | 用途 | 方向 |
|---------|------|------|
| `session.token.usage` | 查询 session token 总览 | 请求 → 响应 |
| `session.token.usage.response` | session token 总览响应 | 服务端 → 客户端 |
| `session.token.usage.updated` | turn 完成后主动推送 usage 更新 | 服务端 → 客户端 |
| `session.token.usage.detail` | 查询按轮 token 明细 | 请求 → 响应 |
| `session.token.usage.detail.response` | 按轮 token 明细响应 | 服务端 → 客户端 |

废弃旧命名 `sessions.token_usage` 和 `sessions.token_usage.response`。前端同步迁移。

### 2. Envelope 定义

```rust
// crates/nova-protocol/src/envelope.rs

// 废弃旧版本（标记 deprecated 或直接移除）
// #[serde(rename = "sessions.token_usage")]
// SessionTokenUsage(obs::SessionTokenUsageRequest),

// 新版本
#[serde(rename = "session.token.usage")]
SessionTokenUsage(obs::SessionTokenUsageRequest),

#[serde(rename = "session.token.usage.response")]
SessionTokenUsageResponse(obs::SessionTokenUsageResponse),

#[serde(rename = "session.token.usage.updated")]
SessionTokenUsageUpdated(obs::SessionTokenUsageResponse),

#[serde(rename = "session.token.usage.detail")]
SessionTokenUsageDetail(obs::SessionTokenUsageDetailRequest),

#[serde(rename = "session.token.usage.detail.response")]
SessionTokenUsageDetailResponse(obs::SessionTokenUsageDetailResponse),
```

### 3. 总览接口 — `session.token.usage`

请求结构不变：

```rust
pub struct SessionTokenUsageRequest {
    pub session_id: String,
}
```

响应结构改为使用 Plan 1 定义的 `SessionTokenUsageSummary`：

```rust
pub struct SessionTokenUsageResponse {
    pub summary: SessionTokenUsageSummary,
}
```

`SessionTokenUsageSummary` 包含：
- session 累计 input/output/cache tokens
- `total_turn_count`
- `turns_with_unknown_cache_usage`
- `turns_with_missing_usage`
- `last_turn_usage: Option<TurnUsage>`（含 source、completeness、raw_provider_usage）
- `updated_at`

### 4. 明细接口 — `session.token.usage.detail`

请求结构（支持分页）：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenUsageDetailRequest {
    pub session_id: String,
    /// 每页返回的最大轮次数，默认 20
    #[serde(default = "default_page_size")]
    pub limit: u32,
    /// 游标：返回此 turn_id 之前的记录（时间倒序）。首次请求不传。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_turn_id: Option<String>,
}

fn default_page_size() -> u32 { 20 }
```

响应结构：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenUsageDetailResponse {
    pub session_id: String,
    /// 按时间倒序排列的 turn usage 列表
    pub turns: Vec<TurnUsageDetail>,
    /// 是否还有更多记录
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TurnUsageDetail {
    pub turn_id: String,
    pub run_id: String,
    pub status: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub usage: Option<TurnUsage>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}
```

### 5. 服务端主动推送 — `session.token.usage.updated`

当前 `SessionTokenUsageUpdated` 事件已在 `envelope.rs:195` 定义但从未发送。

改造：在 `conversation_service.rs` 的 turn 完成流程中，写入 run usage 和更新 session aggregate 之后，通过 gateway outbound channel 发送 `session.token.usage.updated` 事件：

```rust
// conversation_service.rs - execute_agent_turn 尾部

// ... 写入 run usage、更新 session aggregate 之后 ...

// 推送 usage 更新事件
let usage_response = self.get_session_token_usage(session_id).await?;
self.send_event(MessageEnvelope::SessionTokenUsageUpdated(usage_response));
```

前端收到此事件后直接更新 UI，**废弃从 `chat.complete` payload 中自行拼凑 usage 的逻辑**（`gateway-client.ts:663-674`）。

迁移策略：
1. 新增 `session.token.usage.updated` 事件监听
2. 保留 `chat.complete` 中 usage 的旧路径作为 fallback（以防 updated 事件丢失）
3. 当两者同时到达时，以 `session.token.usage.updated` 为准
4. 后续稳定后可移除旧路径

### 6. AgentWorkspaceService 方法实现

```rust
// agent_workspace_service.rs

/// 获取 session token 总览
pub async fn get_session_token_usage(&self, session_id: &str) -> Result<SessionTokenUsageResponse> {
    let runtime = self.get_session_runtime(session_id).await?;
    let quality = self.repository.count_usage_quality(session_id).await?;

    // 获取最近一轮 usage
    let last_turn = self.repository
        .list_run_usage(session_id, 1, None).await?
        .into_iter().next();

    Ok(SessionTokenUsageResponse {
        summary: SessionTokenUsageSummary {
            input_tokens: runtime.token_counters.input_tokens,
            output_tokens: runtime.token_counters.output_tokens,
            cache_creation_input_tokens: runtime.token_counters.cache_creation_input_tokens,
            cache_read_input_tokens: runtime.token_counters.cache_read_input_tokens,
            total_turn_count: quality.total_turns,
            turns_with_unknown_cache_usage: quality.turns_with_unknown_cache,
            turns_with_missing_usage: quality.turns_with_missing_usage,
            last_turn_usage: last_turn.and_then(|t| t.usage),
            updated_at: runtime.token_counters.updated_at,
        },
    })
}

/// 获取按轮 token 明细（分页）
pub async fn get_session_token_usage_detail(
    &self,
    session_id: &str,
    limit: u32,
    before_turn_id: Option<&str>,
) -> Result<SessionTokenUsageDetailResponse> {
    // 多取一条用于判断 has_more
    let fetch_limit = limit + 1;
    let mut turns = self.repository
        .list_run_usage_detail(session_id, fetch_limit, before_turn_id).await?;

    let has_more = turns.len() > limit as usize;
    if has_more {
        turns.pop();
    }

    Ok(SessionTokenUsageDetailResponse {
        session_id: session_id.to_string(),
        turns,
        has_more,
    })
}
```

同时修复现有的 `RunRecord` → 协议 `RunRecord` 转换（`agent_workspace_service.rs:213/240`），把 `usage: None` 改为从 `runs.usage` 列读取填充。

### 7. 与现有 `session.runtime` 快照的关系

- `SessionRuntimeSnapshot` 中保留轻量 token 摘要（`token_counters`），便于一次请求拿到关键概览。
- `LastTurnSnapshot.usage` 已在 Plan 2 中被填充，`session.runtime` 快照自然包含最近一轮 usage。
- 详细 token 记录（完整 turn 列表、provider 原始 JSON）不塞进 `session.runtime`，通过 `session.token.usage.detail` 专题查询。
- 即：`session.runtime` 负责"快照"，`session.token.usage*` 负责"专题查询"。

### 8. 成本估算作为可选扩展位

在 `TurnUsage` 和 `SessionTokenUsageSummary` 中保留可选的成本字段：

```rust
// TurnUsage 中
#[serde(skip_serializing_if = "Option::is_none")]
pub estimated_cost_usd: Option<f64>,

// SessionTokenUsageSummary 中
#[serde(skip_serializing_if = "Option::is_none")]
pub estimated_total_cost_usd: Option<f64>,
```

估算策略：
- 不直接写死在 usage 主链路，避免把金额计算和 token 事实记录耦合
- 单独配置模型单价表（可选功能，配置缺失时不报错，金额字段为 `None`）：

```toml
# .nova/config.toml（示例，本次只预留结构，不强制要求配置）
[[pricing]]
provider = "openai"
model = "gpt-4o"
input_per_million = 2.50
output_per_million = 10.00
cache_read_per_million = 1.25
```

- 估算在组装 response 时计算，不持久化，避免单价变化后历史数据失真

### 9. 前端展示设计

#### 9a. session 页头 / 侧边栏

展示 session 累计 token：
```
Input: 12,345 tokens | Output: 6,789 tokens
Cache hit: 8,901 tokens (3 turns cache unknown)
```

若 `turnsWithMissingUsage > 0`，显示提示：
```
⚠ 2 turns missing usage data
```

#### 9b. agent console token 详情

按时间倒序列出每轮 token 消耗：

```
Turn #5  gpt-4o  2026-05-05 14:32
  Input: 2,345 | Output: 567 | Cache read: 1,200
  Status: success | Source: provider | Completeness: full
  [展开 Provider 原始数据 ▼]

Turn #4  gpt-4o  2026-05-05 14:30
  Input: 1,890 | Output: 423 | Cache: unknown
  Status: success | Source: provider | Completeness: partial
```

支持：
- 展开/折叠 `rawProviderUsage` JSON（格式化显示）
- 失败轮次如果消耗了 token，用不同颜色标记，避免误判"失败就不计费"
- 分页加载（默认最近 20 轮，"加载更多"按钮）

#### 9c. 前端 gateway 客户端改造

`gateway-client.ts` 中：

```typescript
// 新增：监听 session.token.usage.updated 事件
this.on('session.token.usage.updated', (msg) => {
  const summary: SessionTokenUsageSummary = msg.payload.summary;
  this.emit('sessionTokenUsageUpdated', summary);
});

// 新增：请求轮次明细
async getSessionTokenUsageDetail(
  sessionId: string,
  limit?: number,
  beforeTurnId?: string
): Promise<SessionTokenUsageDetailResponse> {
  return this.request('session.token.usage.detail', {
    sessionId,
    limit: limit ?? 20,
    beforeTurnId,
  });
}

// 废弃（迁移完成后移除）：从 chat.complete 中拼凑 usage 的逻辑
// gateway-client.ts:663-674
```

### 10. 前端 TypeScript 类型整理

基于 Plan 1 定义的类型，最终前端类型清单：

| 类型 | 用途 | 状态 |
|------|------|------|
| `TurnUsageView` | 单轮 usage（含 source、completeness、rawProviderUsage） | **新增**（替换旧 `TurnTokenUsageView`） |
| `SessionTokenUsageSummary` | session 总览（含质量元数据） | **新增**（替换旧 `SessionTokenUsageView`） |
| `TurnUsageDetail` | 轮次明细列表项（含 turn_id、model、status） | **新增** |
| `SessionTokenUsageDetailResponse` | 轮次明细分页响应 | **新增** |
| `TokenUsageView` | 保留用于 `ChatCompletePayload` 等非 session 场景 | **保留** |
| `UsageView` | 与 `TokenUsageView` 功能重叠 | **废弃合并** |
| `TurnTokenUsageView` | 被 `TurnUsageView` 替代 | **废弃** |
| `SessionTokenUsageView` | 被 `SessionTokenUsageSummary` 替代 | **废弃** |

## 测试案例

- **测试 1**：`session.token.usage` 返回 `SessionTokenUsageSummary`，包含累计 usage、quality 统计和 `lastTurnUsage`。
- **测试 2**：`session.token.usage.detail` 按时间倒序返回 turn usage 列表，每项含 `turnId`、`model`、`usage`、`status`。
- **测试 3**：`session.token.usage.detail` 分页正确：`limit=2` 时最多返回 2 条，`hasMore` 标记正确，`beforeTurnId` 游标可正确翻页。
- **测试 4**：turn 完成后，服务端发送 `session.token.usage.updated` 事件，前端收到后更新 UI。
- **测试 5**：schema 导出后，前端生成类型与后端协议一致（新增的 `detail` 接口和 `updated` 事件均正确导出）。
- **测试 6**：前端在 `turnsWithUnknownCacheUsage > 0` 时展示"统计不完整"提示。
- **测试 7**：前端在 `turnsWithMissingUsage > 0` 时展示"部分轮次缺失 usage"提示。
- **测试 8**：前端可展开查看 `rawProviderUsage` JSON，OpenAI 嵌套格式和 Anthropic 扁平格式均可正确渲染。
- **测试 9**：成本估算配置缺失时，UI 不报错，金额字段不展示。
- **测试 10**：旧协议名 `sessions.token_usage` 在前后端均已迁移，不再使用。
- **测试 11**：`RunRecord` 转协议 DTO 时，`usage` 字段从 `runs.usage` 列正确读取并填充（不再是 `None`）。
