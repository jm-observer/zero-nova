# Session Token 统计设计

| 章节 | 说明 |
|-----------|------|
| 时间 | 创建：2026-05-05<br>最后更新：2026-05-05 |
| 项目现状 | 见下方 |
| 整体目标 | 见下方 |
| 非目标 | 见下方 |

## 项目现状

1. `nova-agent` 已在 `AgentRuntime` 和 `ConversationService` 中拿到单轮 `Usage`，并把输入、输出、cache creation、cache read 四类计数累加到 `ControlState.token_counters`。
2. `session.token.usage` / `sessions.token_usage` 协议与 `AgentWorkspaceService::get_session_token_usage` 已存在，但当前只返回 session 级累计值，没有按轮明细，也没有区分统计来源可靠性。
3. OpenAI 兼容 provider 通过 `stream_options.include_usage = true` 获取 usage，但当前只映射 `prompt_tokens` 和 `completion_tokens`，cache 相关字段固定为 0。
4. `RunRecord` / `RunStepRecord` 已存在，但尚未持久化每轮 usage，因此无法稳定回答"某个 session 为什么消耗这么多 token"。

### 补充现状（代码审查发现）

5. **`runs.usage` TEXT 列已存在于 SQLite DDL 中**（`sqlite_manager.rs:82`），只是 `create_run` 时始终传 `None`、`update_run_status` 也不更新。Plan 2 不需要 schema migration，只需补写入逻辑。
6. **`LastTurnSnapshot.usage: Option<TurnUsage>` 已在协议层定义**（`observability.rs:84`），但 `conversation_service.rs:310` 创建快照时始终填 `None`，前端拿不到最近一轮的独立 usage。
7. **`SessionTokenUsageUpdated` 事件已在 `envelope.rs:195` 定义但从未被发送。** 前端实际通过解析 `chat.complete` payload 中的 `usage` 字段在客户端本地拼凑 token 统计（`gateway-client.ts:663-674`）。
8. **协议消息命名不一致**：请求是 `sessions.token_usage`（下划线），更新事件是 `sessions.token.usage`（点号）。本次统一改为点号风格。
9. **两份几乎相同的 Usage 结构体并行存在**：`provider::types::Usage`（`provider/types.rs:105`）和 `nova_protocol::chat::Usage`（`chat.rs:7`）；两份几乎相同的 `SessionTokenCounters` 分别在 `control.rs:47` 和 `observability.rs:89`。本次在 Plan 1 中明确它们的演变路径。

## 整体目标

1. 建立 session 级 token 统计的统一口径，明确 input / output / cache creation / cache read 的业务含义。
2. 在现有累计统计基础上补齐 turn/run 级明细，支持查询单轮、累计、最近变化和来源置信度。
3. 设计 provider usage 适配层，兼容不同厂商返回字段不一致、部分字段缺失、cache 统计不可用的现实情况；**保留 provider 原始 usage JSON，随 session 入库并同步前端展示**。
4. 为后续 UI 展示、成本估算、异常排查留出稳定协议，而不要求本次一步到位完成精细计费。
5. **前端界面需要看到具体的 token 消耗**：session 总览、每轮明细、cache 命中情况、provider 原始数据。

## 非目标

1. 本次设计不尝试做"精确计费对账系统"，不承诺与厂商账单 100% 一致。
2. 本次设计不引入本地 tokenizer 对所有历史消息做离线重算，避免把估算逻辑与厂商真实 usage 混在一起。
3. 本次设计不扩展到 embedding、语音转写、TTS 等非 chat token 维度；如需纳入，后续以独立 usage domain 扩展。
4. 本次不处理 reasoning tokens（extended thinking / o-series），后续按需扩展。

## Token 口径定义

### 1. input tokens
- 指单次模型请求被 provider 计入输入侧的 token 数。
- 包含 system prompt、历史消息、工具 schema、工具结果回填、memory 注入等所有实际进入本轮请求体的内容。
- 若 provider 已返回官方 usage，以 provider 返回值为准；不做二次估算覆盖。

### 2. output tokens
- 指单次模型响应生成侧的 token 数。
- 包含 assistant 文本输出、reasoning 输出和工具调用参数输出，只要 provider 已计入 completion / output usage，就统一归入 output。

### 3. cache creation input tokens
- 指 provider 为 prompt cache 首次写入而计费或统计的输入 token。
- 这是"本轮输入里被新建缓存"的部分，不等于总 input tokens。
- 若 provider 不支持或未返回，状态应为"未知"而不是默认认为 0。

### 4. cache read input tokens
- 指本轮请求命中了已有 prompt cache，因此被标记为缓存读取的输入 token。
- 它通常是 input token 的一个子集，用来表达"本轮哪些输入复用了缓存"。
- 同样，provider 未返回时应视为"未知"，不能简单当作 0 参与语义判断。

## 核心设计结论

### 1. 统计单位以 `turn` 为基础，以 `session` 为聚合结果
- 每次 `start_turn` 产生一个 `turn_id` / `run_id`，该轮的 usage 是最小可追踪单位。
- session 总 usage 始终由该 session 下所有已完成 turn 的 usage 聚合而来。
- 现有 `ControlState.token_counters` 可以保留作为"快速读取缓存"，但不能成为唯一真实来源。
- **run 与 turn 保持 1:1 关系**，本次不做拆分。

### 2. "未知"与 "0" 必须分开表达
- `input_tokens`、`output_tokens` 通常可要求为必填数值。
- `cache_creation_input_tokens`、`cache_read_input_tokens` 应改成可空统计值，区分三种状态：
  - provider 明确返回 0
  - provider 明确返回正数
  - provider 根本没有提供

### 3. provider usage 是 source of truth，原始 JSON 需持久化
- 对 chat 主链路，优先信任 provider 响应中的 usage。
- 若 provider 不返回 usage，则本轮主统计可标记为 `partial` 或 `missing`，而不是偷偷写入估算值覆盖真实统计。
- **provider 返回的原始 usage JSON 必须持久化到 run 记录中**，随 session 入库，并同步到前端展示，供调试和审计使用。
- 后续如要加本地 tokenizer 估算，应单独放在 `estimated_usage` 字段，不混进正式 usage。

### 4. session 页面至少要能回答四个问题
- 到现在总共花了多少 input / output token。
- 最近一轮分别花了多少，cache 有没有命中。
- 哪些轮次 usage 缺失、来源不完整，避免误读累计值。
- **provider 原始返回了什么**，在前端可展开查看原始 JSON。

## Plan 拆分

| 状态 | Plan | 说明 | 依赖 | 执行顺序 |
|------|------|------|------|----------|
| 待开始 | Plan 1: Token 数据模型与统计口径收敛 | 调整后端 usage 结构，补齐 turn/session 两级数据模型，明确 unknown/zero 语义，定义既有类型的演变路径。 | 无 | 1 |
| 待开始 | Plan 2: Provider Usage 采集与持久化链路 | 在 provider、agent runtime、conversation、repository 间打通 usage 采集与按轮落库，包含 OpenAI cache 字段解析和原始 JSON 持久化。 | Plan 1 | 2 |
| 待开始 | Plan 3: 协议暴露、UI 展示与成本估算扩展位 | 统一协议命名，扩展 observability 协议与前端视图，提供 session 总览、轮次明细、provider 原始 JSON 展示和可选成本估算。 | Plan 1, Plan 2 | 3 |

## 当前实现与目标语义的主要偏差

### 1. session 总量可查，但 turn 明细不可查
- 当前 `ControlState.token_counters` 只保留累计值。
- `RunRecord` 没有 usage 字段落库（SQLite 列存在但始终 NULL），`get_run_detail` 也返回 `usage: None`。
- 结果是总量能看，原因链路看不到。

### 2. cache 统计被错误压平为 0
- `openai_compat.rs` 当前把 cache creation / cache read 固定写成 0。
- 这会把"provider 未提供"伪装成"确定没有 cache 命中"，语义不正确。
- OpenAI 实际在 `prompt_tokens_details.cached_tokens` 中返回了 cache 信息，但当前 `OpenAiUsage` 结构体未解析该嵌套字段。

### 3. session 总量是写时累加，缺少重建机制
- 当前在 `SessionService::update_runtime_state` 中直接做增量相加。
- 如果未来某轮重试、取消、重复写入或补写 usage，累计值可能失真。

### 4. 协议层没有表达统计质量
- 前端只能看到数字，看不到这些数字是 provider 官方值、部分缺失值，还是完全未知。
- 对成本分析和问题排查不够用。

### 5. 前端 token 展示依赖客户端拼凑
- 前端通过解析 `chat.complete` payload 里的 usage 字段自行累加，而不是走 `session.token.usage` 专用协议。
- `SessionTokenUsageUpdated` 事件已定义但从未发送。

### 6. `LastTurnSnapshot.usage` 始终为 None
- 协议层已预留但从未填充，前端无法从 runtime 快照中直接获取最近一轮 usage。

## 风险与待定项

### 已知风险
- 不同 provider 对 usage 字段命名差异很大，甚至有的只在最终事件返回 usage，有的完全不返回 cache 相关统计。
- 如果继续只保留增量累计、不存 turn 明细，后续所有修正都需要扫描消息历史或重新跑 provider，不可维护。
- 若把"未知 cache"继续写成 0，前端会错误展示"缓存未命中"，影响后续优化判断。
- 协议命名统一（从 `sessions.token_usage` 改为 `session.token.usage`）是 breaking change，需要前后端同步更新。

### 已确认事项
- run 与 turn 保持 1:1 关系，本次不拆分。
- reasoning tokens 本次不处理，后续按需扩展。
- provider 原始 usage JSON 需入库并同步前端展示。
- 协议命名统一为点号风格 `session.token.usage`。
- 成本估算先做可选扩展位，不阻塞 usage 主设计。

## 建议实施顺序

1. 先完成数据模型和协议语义收敛，避免后面边实现边改字段。
2. 再补 provider usage 采集和按轮持久化，确保"先拿到真数据"。
3. 最后做总览接口、前端展示和成本估算，把展示建立在稳定数据之上。
