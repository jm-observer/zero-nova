# Plan 3: 观测、示例与测试补齐

## 前置依赖

Plan 1、Plan 2

## 本次目标

让前端观测、配置示例和回归测试都反映真实的 Provider / LLM 绑定，避免功能已经支持但界面与日志仍显示 `"default"`。

## 涉及文件

1. `.nova/config.toml`
2. `.nova/examples/agents.toml`
3. `crates/nova-agent/src/app/agent_workspace_service.rs`
4. `crates/nova-agent/src/app/bootstrap.rs`
5. `crates/nova-agent/src/conversation/repository.rs`
6. `crates/nova-agent/src/conversation/service.rs`
7. `crates/nova-protocol/src/observability.rs`
8. `crates/nova-agent/tests/*`

## 详细设计

### 1. Agent Inspect 与 Runtime Snapshot

当前 `inspect_agent` 在 agent 默认模型场景下会把 provider 写成 `"default"`。改造后应返回：

1. `effective_model.orchestration.provider = <真实 provider id>`
2. `effective_model.execution.provider = <真实 provider id>`
3. `source = global_default | agent_default | session_override`

### 2. Run 记录与使用量

持久化层和快照层应记录真实执行模型：

1. `orchestration_model.provider`
2. `execution_model.provider`
3. `execution_model.model`

这样前端才能区分“同名模型在不同 Provider 下”的执行差异。

### 3. 日志与错误消息

关键日志需要带上 provider id / llm id，例如：

1. `Bootstrapped agent 'developer' with provider='local', llm='local_gemma'`
2. `Unknown provider override 'cloud2' for session 'xxx'`
3. `Subagent 'developer' resolved provider='cloud', model='gpt-4o'`

### 4. 示例配置更新

`.nova/config.toml` 与 `.nova/examples/agents.toml` 要同步更新为新结构，至少覆盖：

1. 单 provider 多 llm 示例
2. 多 provider 多 llm 示例
3. `1 agent -> 1 provider` 的推荐写法
4. Agent 可选填 `llm` 的回退语义说明

### 5. 文档口径统一

需要统一以下表述：

1. Provider 是“连接目标与鉴权配置”
2. LLM 是“某个 Provider 下的默认模型参数模板”
3. Agent 是“persona + tool policy + provider binding + optional llm binding”

## 测试案例

1. `inspect_agent` 返回真实 provider id，而非 `"default"`
2. 运行记录中能区分不同 provider 的同名模型
3. `.nova/examples/agents.toml` 示例可被解析
4. 多 provider 配置下，子 Agent 观测数据与实际 binding 一致
5. 旧配置迁移后，观测层仍能正确展示 `default` 命名绑定
