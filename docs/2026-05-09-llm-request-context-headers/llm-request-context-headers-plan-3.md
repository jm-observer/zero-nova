# Plan 3: 调用链路接入与开关配置落地

## 前置依赖
- Plan 1
- Plan 2

## 本次目标
- 将 `session_id`、`agent_id` 从会话运行态传递到 Provider 层。
- 增加可配置开关，支持在兼容问题时快速关闭 Header 注入。

## 涉及文件
- `crates/nova-agent/src/agent.rs`
- `crates/nova-agent/src/config.rs`
- `crates/nova-agent/src/app/bootstrap.rs`（如需初始化默认配置）
- `crates/nova-agent/src/provider/openai_compat/mod.rs`

## 详细设计

### 1. 调用链路透传
在 `AgentRuntime::execute_turn_loop` 调用 `self.client.stream(...)` 前构造上下文：
- `session_id`：来自当前 turn 入参 `session_id`
- `agent_id`：来自当前 `model_config.provider` 不是 agent；应从会话控制态 `active_agent` 传入

落地方式二选一：
1. `execute_turn_loop` 增加入参 `agent_id: Option<&str>`，由上层调用点传入（推荐，来源明确）。
2. 在运行时对象中读取当前会话控制态（耦合偏高，不推荐）。

### 2. 配置开关
在 `AppConfig` 新增：

```rust
pub struct OutboundContextHeaderConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}
```

并挂载在 `AppConfig`：

```rust
pub outbound_context_headers: OutboundContextHeaderConfig,
```

行为：
- `enabled = true`：按 Plan 2 规则注入。
- `enabled = false`：忽略上下文，不注入任何新增 Header。

### 3. 边界行为
- `session_id` 缺失：不注入该 Header，不报错。
- `agent_id` 缺失：不注入该 Header，不报错。
- 配置热更新后：新发起请求按最新配置生效。

## 测试案例
1. 配置开关开启：上下文存在则注入。
2. 配置开关关闭：上下文存在也不注入。
3. `agent_id` 缺失：仅注入 `x-session-id`。
4. 多会话并发：不同 session 的 header 值不串线。
