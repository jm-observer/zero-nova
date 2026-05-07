# Plan 1: 配置模型与迁移

## 前置依赖

无

## 本次目标

将配置层从“单 provider + 单 llm”升级为“命名 provider 集合 + 命名 llm 集合 + agent 绑定”，并提供旧配置迁移路径。

## 涉及文件

1. `.nova/config.toml`
2. `.nova/examples/agents.toml`
3. `crates/nova-agent/src/config.rs`
4. `crates/nova-agent/tests/*` 中涉及配置解析的测试

## 详细设计

### 1. 新配置结构

新增以下顶层结构：

```rust
pub struct OriginAppConfig {
    pub providers: HashMap<String, ProviderConfig>,
    pub llms: HashMap<String, RegisteredLlmConfig>,
    pub defaults: DefaultBindingConfig,
    pub search: SearchConfig,
    pub tool: ToolConfig,
    pub gateway: GatewayConfig,
    pub voice: VoiceConfig,
    pub config_path: Option<String>,
}
```

其中：

1. `ProviderConfig` 继续承载 `api_key` / `base_url`
2. `RegisteredLlmConfig` 承载 `provider` 和 `ModelConfig`
3. `DefaultBindingConfig` 承载全局默认 `provider` 与 `llm`

### 2. Agent 配置扩展

为 `AgentSpec` 增加：

```rust
pub struct AgentSpec {
    pub provider: String,
    pub llm: Option<String>,
}
```

语义：

1. `provider` 必填，表示 Agent 默认走哪个 Provider
2. `llm` 选填，表示 Agent 默认走该 Provider 下哪个已注册 LLM

### 3. 原始配置解析层

`RawAppConfig` 需要同时兼容两类输入：

1. 旧结构：`provider` + `llm`
2. 新结构：`providers` + `llms` + `defaults`

迁移顺序建议：

1. 先解析新结构
2. 若新结构缺失，再尝试旧结构迁移
3. 若新旧同时存在，新结构优先，旧结构只输出 warning，不再参与合并

### 4. 迁移规则

旧配置迁移到新结构时：

1. `provider` -> `providers.default`
2. `llm` -> `llms.default`
3. `llms.default.provider = "default"`
4. `defaults.provider = "default"`
5. `defaults.llm = "default"`
6. 未配置 `agent.provider` 的 Agent 自动补 `"default"`
7. 未配置 `agent.llm` 的 Agent 自动补 `"default"`

### 5. 校验规则

`OriginAppConfig::validate` 增加以下校验：

1. `providers` 非空
2. `llms` 非空
3. `defaults.provider` 存在
4. `defaults.llm` 存在
5. `llms[defaults.llm].provider == defaults.provider`
6. 所有 `llm.provider` 都必须命中 `providers`
7. 所有 `agent.provider` 都必须命中 `providers`
8. 所有 `agent.llm` 都必须命中 `llms`
9. 所有 `agent.llm` 的 provider 必须与 `agent.provider` 一致

### 6. 错误信息要求

校验错误必须可直接定位配置项，例如：

1. `agent 'developer' references unknown provider 'cloud2'`
2. `agent 'developer' llm 'gpt4o' belongs to provider 'cloud', expected 'local'`
3. `defaults.llm 'local_gemma' belongs to provider 'local', but defaults.provider is 'cloud'`

## 测试案例

1. 新结构最小配置可成功加载
2. 单 provider 下多个 llm 可成功加载
3. `agent.provider` 指向不存在 provider 时校验失败
4. `agent.llm` 指向不存在 llm 时校验失败
5. `agent.llm` 与 `agent.provider` 不匹配时校验失败
6. 旧 `[provider] + [llm]` 配置能迁移为 `default` 命名对象
7. 新旧结构同时存在时，新结构优先并输出 warning
