# Plan 1: Token 数据模型与统计口径收敛

## 前置依赖
无

## 本次目标
统一 session token 统计的数据模型，明确 turn 级与 session 级边界，并把 cache token 的"未知"与"0"分开表达，避免后续实现阶段继续传播错误语义。同时明确既有类型的演变路径，减少重复结构。

## 涉及文件
- `crates/nova-agent/src/provider/types.rs` — Provider 层 `Usage` 结构体改造
- `crates/nova-agent/src/agent.rs` — turn 内 usage 累积逻辑适配
- `crates/nova-agent/src/conversation/control.rs` — `SessionTokenCounters` 语义降级为派生缓存
- `crates/nova-agent/src/conversation/model.rs` — 内部 `RunRecord` 补充 usage 字段
- `crates/nova-agent/src/conversation/service.rs` — `update_runtime_state` 累加逻辑适配 Option
- `crates/nova-agent/src/conversation/sqlite_manager.rs` — run usage 写入/读取逻辑
- `crates/nova-protocol/src/chat.rs` — 协议层 `Usage` 结构体改造
- `crates/nova-protocol/src/observability.rs` — `TurnUsage`、`SessionTokenCounters`、`SessionTokenUsageSummary` 定义
- `deskapp/src/core/types.ts` — 前端 TypeScript 类型同步

## 详细设计

### 1. 既有类型演变映射

当前代码中存在两组几乎相同的 usage 类型，本次明确它们的归属和改造方案：

| 现有类型 | 所在文件 | 归属层 | 改造方案 |
|---------|---------|-------|---------|
| `provider::types::Usage` | `provider/types.rs:105` | Provider 层 | cache 字段改 `Option<u64>`，增加 `raw_json: Option<serde_json::Value>` |
| `chat::Usage` | `chat.rs:7` | Protocol 层 | 保持前端 DTO 角色，cache 字段改 `Option<u64>` |
| `control::SessionTokenCounters` | `control.rs:47` | Domain 层 | 保持结构不变，语义降级为"派生缓存"，注释说明 |
| `observability::SessionTokenCounters` | `observability.rs:89` | Protocol 层 | 扩展为 `SessionTokenUsageSummary`，增加质量元数据 |
| `observability::TurnUsage` | `observability.rs:248` | Protocol 层 | 增加 `source`、`completeness`、`raw_provider_usage` 字段 |

### 2. Provider 层 Usage 改造

```rust
// crates/nova-agent/src/provider/types.rs

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// None = provider 未提供该字段，Some(0) = provider 明确返回 0
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    /// None = provider 未提供该字段，Some(0) = provider 明确返回 0
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    /// provider 返回的原始 usage JSON，用于调试和审计
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_provider_usage: Option<serde_json::Value>,
}
```

### 3. UsageSource 和 UsageCompleteness 枚举定义

```rust
// crates/nova-protocol/src/observability.rs

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    /// provider 官方返回的 usage
    Provider,
    /// 本地估算（预留，本次不实现）
    Estimated,
    /// 多来源混合（如一轮内多次请求，部分有 usage 部分无）
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsageCompleteness {
    /// 所有字段（input, output, cache_creation, cache_read）均有值
    Full,
    /// 主字段（input, output）有值，cache 等辅助字段部分缺失
    Partial,
    /// 整轮 usage 缺失（provider 未返回或 turn 在获取 usage 前中止）
    Missing,
}
```

### 4. Turn 级 Usage 扩展

协议层 `TurnUsage` 补充元数据和原始 JSON：

```rust
// crates/nova-protocol/src/observability.rs

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TurnUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub source: UsageSource,
    pub completeness: UsageCompleteness,
    /// provider 返回的原始 usage JSON，供前端展开查看
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_provider_usage: Option<serde_json::Value>,
}
```

### 5. 内部 RunRecord 补充 usage 字段

```rust
// crates/nova-agent/src/conversation/model.rs

pub struct RunRecord {
    pub id: String,
    pub session_id: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub orchestration_model: Option<ModelRef>,
    pub execution_model: Option<ModelRef>,
    pub tool_call_count: Option<u32>,
    // 以下为新增
    pub usage_input_tokens: Option<u64>,
    pub usage_output_tokens: Option<u64>,
    pub usage_cache_creation_input_tokens: Option<u64>,
    pub usage_cache_read_input_tokens: Option<u64>,
    pub usage_source: Option<String>,
    pub usage_completeness: Option<String>,
    pub usage_raw_json: Option<serde_json::Value>,
}
```

SQLite `runs.usage` 列已存在（TEXT 类型），存储为 JSON 字符串。序列化格式：

```json
{
  "input_tokens": 1234,
  "output_tokens": 567,
  "cache_creation_input_tokens": null,
  "cache_read_input_tokens": 89,
  "source": "provider",
  "completeness": "partial",
  "raw_provider_usage": { "prompt_tokens": 1234, "completion_tokens": 567, "prompt_tokens_details": { "cached_tokens": 89 } }
}
```

利用已有的 `runs.usage` TEXT 列，不需要 schema migration。读取时对 `usage` 为 NULL 的历史记录兼容处理，视为 `completeness: missing`。

### 6. Session 级累计值保留，但定义为派生缓存

`ControlState.token_counters`（`SessionTokenCounters`）保留结构不变，增加注释标注其语义：

```rust
// crates/nova-agent/src/conversation/control.rs

/// Session 级 token 累计缓存。
/// 这是一个派生值，真实基线来自各 run 的 usage 明细。
/// 正常路径下由 turn 完成时增量更新；异常场景下可从 run usage 重建。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionTokenCounters {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub updated_at: i64,
}
```

cache 字段在 session 累计层面继续用 `u64`（不是 Option），因为：
- 累加时遇到 `None` 的轮次直接跳过，不加 0
- "有多少轮 cache 未知"由查询时实时统计（`SELECT COUNT`），不在累计器中冗余维护

### 7. Session 总览协议结构

```rust
// crates/nova-protocol/src/observability.rs

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenUsageSummary {
    /// session 累计 input tokens
    pub input_tokens: u64,
    /// session 累计 output tokens
    pub output_tokens: u64,
    /// session 累计 cache creation input tokens（仅包含 provider 明确返回的轮次）
    pub cache_creation_input_tokens: u64,
    /// session 累计 cache read input tokens（仅包含 provider 明确返回的轮次）
    pub cache_read_input_tokens: u64,
    /// 该 session 下总 turn 数
    pub total_turn_count: u32,
    /// cache usage 未知的 turn 数（实时查询得出）
    pub turns_with_unknown_cache_usage: u32,
    /// usage 完全缺失的 turn 数
    pub turns_with_missing_usage: u32,
    /// 最近一轮的 usage（若有）
    pub last_turn_usage: Option<TurnUsage>,
    /// 最后更新时间（毫秒时间戳）
    pub updated_at: i64,
}
```

`turns_with_unknown_cache_usage` 和 `turns_with_missing_usage` 通过查询 `runs` 表实时统计（方案 A），不在累计器中维护增量计数器。理由：
- 它们是派生值，维护增量计数器会引入额外的一致性负担
- `runs` 表按 `session_id` 索引，查询开销可控

### 8. 协议层 chat::Usage 同步改造

```rust
// crates/nova-protocol/src/chat.rs

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
}
```

### 9. 前端 TypeScript 类型同步

```typescript
// deskapp/src/core/types.ts

/** Provider 标准化后的单轮 usage */
export interface TurnUsageView {
  inputTokens: number;
  outputTokens: number;
  cacheCreationInputTokens?: number | null;
  cacheReadInputTokens?: number | null;
  source: 'provider' | 'estimated' | 'mixed';
  completeness: 'full' | 'partial' | 'missing';
  /** provider 原始 usage JSON，供展开查看 */
  rawProviderUsage?: Record<string, unknown> | null;
}

/** Session 级 token 总览 */
export interface SessionTokenUsageSummary {
  inputTokens: number;
  outputTokens: number;
  cacheCreationInputTokens: number;
  cacheReadInputTokens: number;
  totalTurnCount: number;
  turnsWithUnknownCacheUsage: number;
  turnsWithMissingUsage: number;
  lastTurnUsage?: TurnUsageView | null;
  updatedAt: number;
}
```

清理现有冗余类型：
- `TokenUsageView` — 保留，仍用于 `ChatCompletePayload` 等非 session 场景
- `UsageView` — 如果与 `TokenUsageView` 功能重叠，合并为一个
- `SessionTokenUsageView`（当前未使用）— 替换为 `SessionTokenUsageSummary`
- `TurnTokenUsageView` — 替换为 `TurnUsageView`

## 测试案例

- **测试 1**：provider usage 缺少 cache 字段时，`Usage.cache_creation_input_tokens` 为 `None`（不是 `Some(0)`）；`serde_json::from_str` 对缺失字段反序列化后仍为 `None`。
- **测试 2**：`TurnUsage` 序列化到 JSON 后，`source` 与 `completeness` 枚举值为 snake_case 字符串。
- **测试 3**：session 总览中 `turns_with_unknown_cache_usage` 等于 `runs` 表中 cache 字段为 NULL 的记录数。
- **测试 4**：历史无 usage 的 `RunRecord`（`runs.usage = NULL`）仍可被正确加载，usage 各字段为 `None`，`usage_completeness` 视为 `"missing"`。
- **测试 5**：`raw_provider_usage` 字段可正确存储和读取任意 JSON 结构（OpenAI 嵌套格式、Anthropic 扁平格式）。
- **测试 6**：前端 TypeScript 类型与 Rust protocol 字段在 camelCase 命名上一一对应。
