# Plan 2: 运行时绑定与客户端装配

## 前置依赖

Plan 1

## 本次目标

移除“全局单 Provider / 单 Client”的运行时假设，让顶级 Agent、子 Agent、会话覆写都能解析到明确的 Provider + LLM 绑定。

## 涉及文件

1. `crates/nova-agent/src/app/bootstrap.rs`
2. `crates/nova-agent/src/tool/builtin/agent.rs`
3. `crates/nova-agent/src/app/agent_workspace_service.rs`
4. `crates/nova-agent/src/conversation/control.rs`
5. `crates/nova-cli/src/main.rs`
6. `crates/nova-server/src/bin/nova_gateway_stdio.rs`
7. `crates/nova-server/src/bin/nova_gateway_ws.rs`

## 详细设计

### 1. 引入绑定解析层

新增统一解析入口，例如：

```rust
pub struct ResolvedAgentBinding {
    pub provider_id: String,
    pub provider: ProviderConfig,
    pub llm_id: Option<String>,
    pub model_config: ModelConfig,
}
```

解析顺序：

1. Agent 指定 `llm` 时，优先使用该 llm
2. Agent 未指定 `llm` 时，使用 `defaults.llm`
3. 若会话存在 override，override 覆盖模型名与参数，但不能绕过 provider 合法性校验

### 2. 顶级应用启动

`build_application` 不再依赖外部预先构造好的固定 client。改为：

1. 先解析默认 Agent 的 `ResolvedAgentBinding`
2. 基于该 binding 构造顶层默认 runtime client
3. `AgentRegistry` 中为每个 Agent 记录自己的默认 binding 元数据

如果后续 `ConversationService` 需要按 active agent 动态切换 client，则这里还需进一步把 client 工厂注入 runtime，而不是注入固定 client 实例。

### 3. 子 Agent 执行

`AgentTool::run_subagent` 当前直接使用 `self.config.provider` 构造 `OpenAiCompatClient`，这必须改掉。

新的执行路径：

1. 先根据 `subagent_type` 找到 `AgentSpec`
2. 解析该 Agent 的 `ResolvedAgentBinding`
3. 用该 binding 构造 client
4. `model_override` 仅允许覆写模型参数，不允许偷偷跨 provider

### 4. 会话覆写

当前会话覆写结构里已有：

```rust
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}
```

首版保留该协议，但要补两层规则：

1. `provider` 必须命中注册表
2. 当 `model` 命中某个已注册 llm 时，优先解析为注册 llm
3. 当 `model` 只是裸模型名时，只允许在当前 provider 下直接构造临时 `ModelConfig`

### 5. CLI 与服务端覆写

当前 CLI / gateway bin 里通过 `--model` 和 `--base-url` 直接改写 `config.llm` / `config.provider`。在新设计下要改为：

1. `--provider <id>`：切换默认 provider
2. `--llm <id>`：切换默认 llm
3. `--model <raw-name>`：仅作为临时模型名覆写
4. `--base-url <url>`：只用于调试模式，覆盖某个已选 provider 的 base_url，不应再修改全局单实例字段

### 6. 语音路径暂不并入本次改造

`VoiceService` 目前仍复用 `config.provider`。首版不和文本 LLM 绑定体系强耦合，建议先维持独立配置或显式约束为继续走默认 provider，避免把本次范围扩大到 STT/TTS。

## 测试案例

1. 顶级 `nova` Agent 与 `developer` Agent 绑定不同 provider 时，运行时能分别解析成功
2. `AgentTool` 拉起子 Agent 时会使用对应 provider，而不是全局默认 provider
3. 会话 override 指向合法 provider + model 时生效
4. 会话 override 指向未知 provider 时失败
5. `model_override` 不会把 Agent 偷偷切到另一个 provider
6. CLI 指定 `--provider` / `--llm` 后，最终 binding 符合预期
