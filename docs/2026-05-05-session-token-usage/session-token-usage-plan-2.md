# Plan 2: Provider Usage 采集与持久化链路

## 前置依赖
Plan 1

## 本次目标
在 provider、agent runtime、conversation service、repository 之间打通 usage 采集链路，确保每个 session 的每一轮都能拿到稳定的 token 记录，并可回溯来源与完整性。包括 OpenAI 兼容层 cache 字段解析、原始 JSON 捕获和按轮落库。

## 涉及文件
- `crates/nova-agent/src/provider/openai_compat.rs` — OpenAI 兼容层 usage 解析改造
- `crates/nova-agent/src/provider/anthropic.rs` — Anthropic provider 适配确认
- `crates/nova-agent/src/provider/types.rs` — `OpenAiUsage` 结构体扩展
- `crates/nova-agent/src/agent.rs` — turn 内 usage 累积逻辑适配 Option 语义
- `crates/nova-agent/src/app/conversation_service.rs` — turn 完成后写入 run usage
- `crates/nova-agent/src/conversation/service.rs` — `update_runtime_state` 适配 Option cache
- `crates/nova-agent/src/conversation/repository.rs` — 新增 `update_run_usage` / `sum_session_usage` / `count_usage_quality`
- `crates/nova-agent/src/conversation/sqlite_manager.rs` — `create_run` / `update_run_status` 补 usage 写入，`load_run` 补 usage 读取

## 详细设计

### 1. OpenAI 兼容层 cache 字段解析

当前问题：`OpenAiUsage` 只有 `prompt_tokens` 和 `completion_tokens`，`process_chunk` 中 cache 硬编码为 0。

OpenAI 实际 usage 结构为嵌套格式：

```json
{
  "usage": {
    "prompt_tokens": 100,
    "completion_tokens": 50,
    "prompt_tokens_details": {
      "cached_tokens": 30
    },
    "completion_tokens_details": {
      "reasoning_tokens": 10
    }
  }
}
```

改造 `OpenAiUsage` 结构体（`provider/types.rs`）：

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct OpenAiUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    #[serde(default)]
    pub prompt_tokens_details: Option<OpenAiPromptTokensDetails>,
    #[serde(default)]
    pub completion_tokens_details: Option<OpenAiCompletionTokensDetails>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OpenAiPromptTokensDetails {
    pub cached_tokens: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OpenAiCompletionTokensDetails {
    pub reasoning_tokens: Option<u64>,
}
```

字段映射规则：

| OpenAI 字段 | 映射到 | 说明 |
|------------|--------|------|
| `prompt_tokens` | `input_tokens` | 直接映射 |
| `completion_tokens` | `output_tokens` | 直接映射 |
| `prompt_tokens_details.cached_tokens` | `cache_read_input_tokens` | 有值时 `Some(n)`，字段缺失时 `None` |
| （无对应字段） | `cache_creation_input_tokens` | OpenAI 不区分 cache creation，始终 `None` |
| `completion_tokens_details.reasoning_tokens` | （本次不映射） | 预留，后续扩展 |

改造 `process_chunk`（`openai_compat.rs:277`）：

```rust
fn process_chunk(&mut self, chunk: ChatCompletionChunk) {
    if let Some(usage) = &chunk.usage {
        // 捕获原始 JSON 用于持久化和前端展示
        let raw_json = serde_json::to_value(usage).ok();

        self.event_queue.push_back(ProviderStreamEvent::MessageComplete {
            usage: Usage {
                input_tokens: usage.prompt_tokens,
                output_tokens: usage.completion_tokens,
                cache_creation_input_tokens: None, // OpenAI 不提供
                cache_read_input_tokens: usage
                    .prompt_tokens_details
                    .as_ref()
                    .and_then(|d| d.cached_tokens),
                raw_provider_usage: raw_json,
            },
            stop_reason: self.pending_stop_reason.take(),
        });
        return;
    }
    // ...
}
```

### 2. Anthropic provider 适配

Anthropic 原生返回的 usage 已包含 `cache_creation_input_tokens` 和 `cache_read_input_tokens`，当前通过 `serde` 直接反序列化到 `Usage`。

需要确认的改动：
- `Usage` 的 cache 字段改为 `Option<u64>` 后，Anthropic 的 `#[serde(default)]` 反序列化行为：如果 API 响应中该字段存在则为 `Some(n)`，缺失则为 `None`，符合预期。
- 补充 `raw_provider_usage` 捕获：在 `StreamEvent::MessageStop` 处理中，将原始 usage value 存入 `raw_provider_usage`。

### 3. AgentRuntime turn 内 usage 累积逻辑适配

当前 `agent.rs` 中 `cumulative_usage` 的累加逻辑（两处：`run_turn` 和 `run_turn_with_context`）需要适配 `Option<u64>` 语义：

```rust
// 每次迭代结束累积
cumulative_usage.input_tokens += iter_usage.input_tokens;
cumulative_usage.output_tokens += iter_usage.output_tokens;

// cache 字段：Option + Option 的聚合规则
// - 双方都有值：相加
// - 一方有值：取有值方
// - 双方都 None：保持 None
cumulative_usage.cache_creation_input_tokens = match (
    cumulative_usage.cache_creation_input_tokens,
    iter_usage.cache_creation_input_tokens,
) {
    (Some(a), Some(b)) => Some(a + b),
    (Some(a), None) | (None, Some(a)) => Some(a),
    (None, None) => None,
};
// cache_read_input_tokens 同理

// raw_provider_usage：保留最后一次 iteration 的（如果多 iteration 需要看明细，可考虑用数组）
if iter_usage.raw_provider_usage.is_some() {
    cumulative_usage.raw_provider_usage = iter_usage.raw_provider_usage;
}
```

**completeness 推导规则**（在 turn 结束时）：
- 若 `input_tokens > 0 && output_tokens > 0`，且 cache 字段至少有一个为 `Some` → `Full` 或 `Partial`
- 若 cache 字段全部为 `None` → `Partial`
- 若 `input_tokens == 0 && output_tokens == 0` → `Missing`

**source 推导规则**：
- 所有 iteration 都有 provider usage → `Provider`
- 部分 iteration 有 usage → `Mixed`
- 无 iteration 有 usage → 不应出现（provider 不返回 usage 时已标记 Missing）

### 4. ConversationService 在 turn 完成后写入 usage 明细

在 `conversation_service.rs` 的 `execute_agent_turn` 中，turn 完成后的处理顺序：

```
1. 从 TurnResult 提取 usage
2. 构建 TurnUsage（含 source、completeness、raw_json）
3. 调用 repository.update_run_usage(run_id, turn_usage) — 写入 run 记录
4. 调用 sessions.update_runtime_state(..., token_delta) — 更新 session 累计缓存
5. 填充 LastTurnSnapshot.usage — 让 runtime 快照带上最近一轮 usage
```

步骤 3 和 4 **建议在同一个 SQLite 事务中执行**，但如果实现复杂度过高，可以接受"run usage 先写成功、session aggregate 后续可重建"的策略。

### 5. Repository 新增方法

```rust
// crates/nova-agent/src/conversation/repository.rs（或对应的 trait）

/// 写入单轮 usage 到 runs.usage 列
async fn update_run_usage(&self, run_id: &str, usage_json: &str) -> Result<()>;

/// 聚合某 session 下所有 run 的 usage，用于重建 session aggregate
async fn sum_session_usage(&self, session_id: &str) -> Result<SessionUsageAggregate>;

/// 统计某 session 下 usage 质量分布
async fn count_usage_quality(&self, session_id: &str) -> Result<UsageQualityCounts>;
```

`SessionUsageAggregate` 用于重建：
```rust
pub struct SessionUsageAggregate {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}
```

`UsageQualityCounts` 用于总览接口：
```rust
pub struct UsageQualityCounts {
    pub total_turns: u32,
    pub turns_with_unknown_cache: u32,
    pub turns_with_missing_usage: u32,
}
```

SQLite 查询示例：

```sql
-- sum_session_usage
SELECT
    COALESCE(SUM(json_extract(usage, '$.input_tokens')), 0) as input_tokens,
    COALESCE(SUM(json_extract(usage, '$.output_tokens')), 0) as output_tokens,
    COALESCE(SUM(json_extract(usage, '$.cache_creation_input_tokens')), 0) as cache_creation_input_tokens,
    COALESCE(SUM(json_extract(usage, '$.cache_read_input_tokens')), 0) as cache_read_input_tokens
FROM runs
WHERE session_id = ?1 AND usage IS NOT NULL;

-- count_usage_quality
SELECT
    COUNT(*) as total_turns,
    COUNT(CASE WHEN usage IS NOT NULL
        AND json_extract(usage, '$.cache_creation_input_tokens') IS NULL
        AND json_extract(usage, '$.cache_read_input_tokens') IS NULL
        THEN 1 END) as turns_with_unknown_cache,
    COUNT(CASE WHEN usage IS NULL THEN 1 END) as turns_with_missing_usage
FROM runs
WHERE session_id = ?1;
```

### 6. SQLite 写入改造

`sqlite_manager.rs` 中的 `create_run` 当前传 `Option::<String>::None` 给 `usage` 列。

改造 `update_run_status`（或新增 `update_run_usage`），在 turn 完成时写入：

```rust
pub async fn update_run_usage(&self, run_id: &str, usage_json: &str) -> Result<()> {
    sqlx::query("UPDATE runs SET usage = ?1 WHERE run_id = ?2")
        .bind(usage_json)
        .bind(run_id)
        .execute(&self.pool)
        .await?;
    Ok(())
}
```

`load_run` 读取时，对 `usage` 列为 NULL 的历史记录做兼容：

```rust
let usage: Option<String> = row.get("usage");
let parsed_usage = usage
    .as_deref()
    .map(|s| serde_json::from_str(s))
    .transpose()?;
// parsed_usage: Option<StoredTurnUsage>
// 若为 None，表示历史数据，视为 completeness: Missing
```

### 7. `LastTurnSnapshot.usage` 填充

当前 `conversation_service.rs:310` 创建 `snapshot_internal` 时 `usage: None`。

改造：turn 完成后，将 `TurnUsage` 写入 snapshot：

```rust
let snapshot = LastTurnSnapshot {
    turn_id: turn_id.clone(),
    usage: Some(TurnUsage {
        input_tokens: turn_result.usage.input_tokens,
        output_tokens: turn_result.usage.output_tokens,
        cache_creation_input_tokens: turn_result.usage.cache_creation_input_tokens,
        cache_read_input_tokens: turn_result.usage.cache_read_input_tokens,
        source: inferred_source,
        completeness: inferred_completeness,
        raw_provider_usage: turn_result.usage.raw_provider_usage.clone(),
    }),
    // ...其他字段
};
```

这样 `session.runtime` 快照自然包含最近一轮 usage，前端不需要额外请求。

### 8. session 累计更新适配 Option cache

`update_runtime_state` 的 `token_delta` 参数从 `(u64, u64, u64, u64)` 改为结构化类型：

```rust
pub struct TokenDelta {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
}
```

累加逻辑：
```rust
control.token_counters.input_tokens += delta.input_tokens;
control.token_counters.output_tokens += delta.output_tokens;
// cache 只在有值时累加，None 时跳过（不加 0）
if let Some(v) = delta.cache_creation_input_tokens {
    control.token_counters.cache_creation_input_tokens += v;
}
if let Some(v) = delta.cache_read_input_tokens {
    control.token_counters.cache_read_input_tokens += v;
}
```

### 9. 取消与失败路径处理

| 场景 | usage 是否已获取 | 处理方式 |
|------|----------------|---------|
| turn 正常完成 | 是 | 正常写入，completeness 按字段推导 |
| turn 在 provider 返回 usage 后失败（如工具执行失败） | 是 | 写入 usage，run status 标记 `failed`，token 已消耗的事实不应隐藏 |
| turn 在 provider 返回 usage 前取消 | 否 | run usage 写入 `{"completeness": "missing"}`，不写入伪造的 0 |
| provider 流式传输中断，部分 iteration 有 usage | 部分 | 写入已获取的 usage，completeness 标记 `partial` |

在 `conversation_service.rs` 中，需要确保 **无论 turn 成功还是失败，都尝试写入已获取的 usage**。可在 `execute_agent_turn` 的 error path 中也调用 `update_run_usage`。

### 10. session aggregate 重建能力

提供一个方法，在检测到不一致时从 run usage 重建 session counters：

```rust
pub async fn rebuild_session_token_counters(&self, session_id: &str) -> Result<()> {
    let aggregate = self.repository.sum_session_usage(session_id).await?;
    let session = self.sessions.get(session_id).await?.context("Session not found")?;
    {
        let mut control = session.control.write()?;
        control.token_counters.input_tokens = aggregate.input_tokens;
        control.token_counters.output_tokens = aggregate.output_tokens;
        control.token_counters.cache_creation_input_tokens = aggregate.cache_creation_input_tokens;
        control.token_counters.cache_read_input_tokens = aggregate.cache_read_input_tokens;
        control.token_counters.updated_at = Utc::now().timestamp_millis();
    }
    self.sessions.persist_runtime_control(session_id, &session).await?;
    Ok(())
}
```

本次不做自动触发，仅提供能力。后续可在 session 加载时做一致性校验。

## 测试案例

- **测试 1**：OpenAI 兼容 provider 返回含 `prompt_tokens_details.cached_tokens` 的 usage 时，`cache_read_input_tokens` 正确映射为 `Some(n)`，`cache_creation_input_tokens` 为 `None`。
- **测试 2**：OpenAI 兼容 provider 返回不含 `prompt_tokens_details` 的 usage 时，两个 cache 字段均为 `None`。
- **测试 3**：Anthropic provider 返回含 cache 字段的 usage 时，`cache_creation_input_tokens` 和 `cache_read_input_tokens` 均为 `Some(n)`。
- **测试 4**：一轮多 iteration 时，turn usage 为所有 iteration usage 的正确累加值（含 Option cache 聚合规则）。
- **测试 5**：turn 完成后，`runs.usage` 列被写入有效 JSON，且包含 `raw_provider_usage` 原始数据。
- **测试 6**：turn 失败但 provider 已返回 usage 时，该 usage 仍然被写入 run 记录，completeness 按实际情况标记。
- **测试 7**：turn 在 provider 返回 usage 前取消时，run usage 为 `completeness: missing`，不包含伪造的 0。
- **测试 8**：session aggregate 可通过 `rebuild_session_token_counters` 从 run usage 重建，结果与增量累加一致。
- **测试 9**：历史无 usage 的 run 记录（`usage = NULL`）可正常加载，不影响 session aggregate 计算。
- **测试 10**：`LastTurnSnapshot.usage` 在 turn 完成后被正确填充，`session.runtime` 快照包含最近一轮 usage。
- **测试 11**：`raw_provider_usage` 可保存 OpenAI 嵌套格式和 Anthropic 扁平格式两种 JSON 结构。
