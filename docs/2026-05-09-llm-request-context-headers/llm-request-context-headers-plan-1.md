# Plan 1: 上下文模型与协议接口扩展

## 前置依赖
- 无

## 本次目标
- 定义 Provider 出站请求所需的轻量上下文模型。
- 在 `LlmClient` trait 上扩展新的入参，支持透传 `session_id` 与 `agent_id`。
- 明确字段校验与空值策略，避免污染下游过滤。

## 涉及文件
- `crates/nova-agent/src/provider/mod.rs`
- `crates/nova-agent/src/agent.rs`
- `crates/nova-agent/src/provider/openai_compat/mod.rs`
- （可选）`crates/nova-agent/src/provider/types.rs` 或独立 `context.rs`

## 详细设计

### 1. 新增请求上下文结构
建议新增结构体 `ProviderRequestContext`：

```rust
#[derive(Debug, Clone, Default)]
pub struct ProviderRequestContext {
    pub session_id: Option<String>,
    pub agent_id: Option<String>,
}
```

设计要点：
- 使用 `Option<String>`，兼容暂时无法提供上下文的调用点。
- 保持结构轻量，仅承载 HTTP 过滤需要的稳定标识。
- 不引入业务可变字段，避免接口膨胀。

### 2. 扩展 `LlmClient` trait
将签名从：

```rust
async fn stream(&self, messages: &[Message], tools: &[ToolDefinition], config: &ModelConfig)
    -> Result<Box<dyn StreamReceiver>>;
```

调整为：

```rust
async fn stream(
    &self,
    messages: &[Message],
    tools: &[ToolDefinition],
    config: &ModelConfig,
    request_context: &ProviderRequestContext,
) -> Result<Box<dyn StreamReceiver>>;
```

设计要点：
- 通过显式参数传递上下文，避免线程局部变量或全局状态。
- 后续可平滑扩展 trace 字段，不影响现有调用语义。

### 3. 字段校验策略
新增统一校验函数（Provider 侧复用）：
- 空白字符串视为无效（`trim().is_empty()`）。
- 建议允许字符集：`[A-Za-z0-9._:-]`，超出范围可选择：
  - 严格模式：丢弃该 Header 并记录 debug 日志。
  - 宽松模式：仅 `trim` 后透传。

当前建议先采用“宽松 + 非空校验”最小方案，减少行为变化。

## 测试案例
1. `request_context` 全为空：不应影响原始调用路径。
2. `session_id` 有值、`agent_id` 空：仅透传一个 Header。
3. 两者都为空白字符串：都不透传。
4. 特殊字符输入：按约定行为（丢弃或透传）可预测。
