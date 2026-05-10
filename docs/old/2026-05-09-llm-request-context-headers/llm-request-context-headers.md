# LLM 请求上下文 Header 透传设计

## 时间
- 创建日期：2026-05-09
- 最后更新：2026-05-10

## 项目现状

### 1. 当前 LLM 出站调用链路
- 入口：`crates/nova-agent/src/agent.rs` 的 `AgentRuntime::execute_turn_loop`
- 调用：`self.client.stream(&all_messages, tool_definitions, model_config).await`
- 协议层：`crates/nova-agent/src/provider/mod.rs` 中 `LlmClient::stream` 仅接收 `messages/tools/config`
- OpenAI 兼容实现：`crates/nova-agent/src/provider/openai_compat/mod.rs`，通过 `async-openai` 发起请求

### 2. 当前问题
- 出站 HTTP 请求未携带会话/智能体上下文标识，无法在后续 HTTP 过滤系统中进行精确链路筛选。
- `session_id` 与 `agent_id` 在会话运行态中存在，但未被透传到 Provider 层。

### 3. 约束与目标
- 变更要小而聚焦，不混入重构。
- 不影响既有对话主流程与流式事件语义。
- 允许上游服务忽略未知 Header；不依赖上游强制支持。

## 整体目标
- 在大模型出站请求头中增加：
  - `x-session-id`
  - `x-agent-id`
- 通过显式上下文对象透传，确保字段来源稳定、可审计。
- 对透传行为提供开关与最小校验，降低兼容风险。

## Plan 拆分

| Plan | 描述 | 依赖 | 顺序 | 状态 |
|---|---|---|---|---|
| Plan 1 | 上下文模型与协议接口扩展 | 无 | 1 | 已完成 |
| Plan 2 | OpenAI 兼容 Provider Header 注入实现 | Plan 1 | 2 | 已完成 |
| Plan 3 | 调用链路接入与开关配置落地 | Plan 1, Plan 2 | 3 | 已完成 |
| Plan 4 | 测试、验证与发布回归 | Plan 1, Plan 2, Plan 3 | 4 | 已完成 |

## 风险与待定项

### 风险
1. `async-openai` 对自定义 Header 注入方式受 SDK 接口约束，可能需切换到 `reqwest` 手动请求或扩展 SDK 配置。
2. 部分企业网关对未知 Header 有严格策略，可能返回 4xx。
3. 若上下文透传为空字符串，会造成过滤规则污染。

### 待定项
1. Header 命名是否固定为 `x-session-id` / `x-agent-id`，是否需要统一前缀（如 `x-nova-*`）。
2. 是否在后续版本增加 `x-trace-id`，用于一次 turn 内多请求关联。
3. 是否需要将 Header 透传扩展到 `anthropic` 与 `voice/openai_compat` 路径。
