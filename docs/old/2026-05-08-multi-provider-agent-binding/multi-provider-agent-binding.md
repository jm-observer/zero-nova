# 多 Provider 与 Agent 绑定设计

- **时间**：2026-05-08
- **状态**：Plan 1 / Plan 2 / Plan 3 已完成

---

## 项目现状

当前 `.nova/config.toml` 与 `crates/nova-agent/src/config.rs` 采用的是单全局 Provider + 单全局 LLM 模式：

1. `[provider]` 只有一组 `api_key` / `base_url`
2. `[llm]` 只有一组模型参数，默认被所有 Agent 共享
3. `[[gateway.agents]]` 目前只承载 prompt、别名、工具白名单和可选 `model_config`
4. `build_application` 在启动时只构造一个 `LlmClient`
5. `AgentTool` 在拉起子 Agent 时仍然直接复用全局 `config.provider`

这带来三个直接限制：

1. 不能同时接入多个 Provider
2. 不能把多个 LLM 显式归属到某个 Provider
3. Agent 只能改模型参数，不能显式切换到底使用哪个 Provider

---

## 整体目标

在保持现有配置风格可读、迁移成本可控的前提下，完成以下设计目标：

1. 将单个 `[provider]` 扩展为可命名、可引用的 `providers` 集合
2. 将单个 `[llm]` 扩展为“归属于某个 provider 的命名 llm 集合”
3. 允许同一个 Provider 下声明多个 LLM
4. 让每个 `[[gateway.agents]]` 显式绑定一个 Provider，并可选择该 Provider 下的默认 LLM
5. 保留兼容旧配置的迁移路径，避免一次性打断现有工作区

---

## 结论与判断

### 1. `provider` 数组化是合理的

合理。因为系统已经出现以下真实需求：

1. 不同 Agent 可能需要不同网关地址、不同鉴权凭证或不同供应商能力
2. 同一工作区可能同时接入本地 OpenAI 兼容网关、云端推理服务和专用语音端点
3. 把多 Provider 继续塞进单个 `[provider]` 会迫使上层通过环境变量或命令行覆写，配置语义会越来越混乱

因此不应继续扩大单实例 `[provider]`，而应引入命名 Provider 表。

### 2. `llm` 归属到 `provider`，且一个 Provider 对应多个 LLM，是合理的

合理。这比“全局 LLM 列表 + 每个 LLM 再反向写 provider 名”更清晰，因为：

1. `model` 是否可用本质上依赖具体 Provider 的 API 端点与兼容协议
2. 同一个模型名在不同 Provider 下可能语义不同，甚至根本不可互换
3. 先从 Provider 选模型，天然符合运行时装配顺序

建议设计为“全局命名 `llms` 集合，每个 llm 强制声明 `provider`”，而不是把 llm 直接嵌在 provider 下面。这样 TOML 结构更利于引用、校验和向后扩展；但语义上必须明确“llm 从属于 provider”。

### 3. `1 个 agent 配置 1 个 provider` 在当前阶段是合理的

合理，且建议作为首版强约束。原因：

1. 当前 `AgentDescriptor` 与 `AgentRuntime` 都按“一个 Agent 一套身份配置”运行，`provider` 绑定到 Agent 最符合现有装配方式
2. Agent 的 prompt persona、工具权限、模型选择，通常应和执行后端一起稳定下来，避免同一 Agent 在不同 Provider 间漂移
3. 如果允许“一个 Agent 同时声明多个 provider”，就必须额外定义切换策略、失败回退、能力差异掩码、观测口径，复杂度会明显上升

因此首版建议：

1. `agent.provider` 必填，表示该 Agent 默认使用哪个 Provider
2. `agent.llm` 选填，但若填写则必须属于 `agent.provider`
3. 同一 Agent 的不同阶段若未来需要不同模型，可在后续再扩展 `orchestration_llm` / `execution_llm`

---

## 建议配置结构

建议把配置从：

```toml
[provider]
base_url = "http://127.0.0.1:8082/v1"

[llm]
model = "gpt-oss-120b"
max_tokens = 8192
```

演进为：

```toml
[providers.local]
base_url = "http://127.0.0.1:8082/v1"
api_key_env = "NOVA_API_KEY"

[providers.cloud]
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"

[llms.local_gpt_oss]
provider = "local"
model = "gpt-oss-120b"
max_tokens = 8192
temperature = 0.5

[llms.local_gemma]
provider = "local"
model = "gemma-4-26B-A4B-it"
max_tokens = 8192
temperature = 0.5

[llms.cloud_gpt4o]
provider = "cloud"
model = "gpt-4o"
max_tokens = 8192
temperature = 0.3

[defaults]
provider = "local"
llm = "local_gpt_oss"

[[gateway.agents]]
id = "nova"
display_name = "Nova"
description = "默认通用助手"
prompt_file = "agent-nova.md"
provider = "local"
llm = "local_gpt_oss"

[[gateway.agents]]
id = "developer"
display_name = "Developer"
description = "开发任务子代理"
prompt_file = "agent-developer.md"
provider = "local"
llm = "local_gemma"
```

---

## 核心设计决策

### 1. Provider 与 LLM 都使用“命名映射”，不使用匿名数组

虽然需求口语上是“数组化”，但实际落地更建议 TOML 命名表，而不是匿名数组：

1. `providers.<id>` 与 `llms.<id>` 比 `[[providers]]` / `[[llms]]` 更容易被 Agent 和会话覆写引用
2. 命名键天然具备唯一性，更适合做配置校验
3. 配置 diff 更稳定，可读性更高

因此这里把“数组化”解释为“由单实例扩展为多实例集合”，而不是强制使用 TOML 数组语法。

### 2. `llm` 必须显式声明 `provider`

运行时不做“按模型名猜 provider”的隐式推断，原因：

1. 同名模型可能挂在多个 Provider 下
2. 观测层需要精确记录执行使用的是哪一个 Provider
3. 配置错误应在加载期暴露，而不是在请求发出后失败

### 3. `agent.provider` 与 `agent.llm` 同时存在，但 `agent.llm` 优先

解析规则建议如下：

1. 若 `agent.llm` 存在，则用该 llm 的 `provider` 和模型参数构造执行配置
2. 若 `agent.llm` 不存在，则使用 `agent.provider` + 全局 `defaults.llm` 中属于该 provider 的默认模型
3. 若 `defaults.llm` 不属于 `agent.provider`，配置校验直接失败

这样既允许 Agent 只声明 Provider，也允许 Agent 绑定明确模型。

### 4. 会话级 override 继续保留，但必须受注册表约束

当前 `SessionModelOverrideRequest` 已有 `provider + model` 结构。首版应改为：

1. override 的 `provider` 必须命中已注册 provider
2. override 的 `model` 必须与该 provider 下可解析的 llm 对应，或者显式声明为“裸模型覆写”
3. 若只覆写模型名、不覆写 provider，则默认沿用当前 Agent 的 provider

### 5. 运行时要从“单 client”升级为“按绑定即时构造 client”

因为不同 Agent 会走不同 Provider，`build_application` 不能再只注入一个全局 `LlmClient` 实例。首版建议引入解析层与工厂层：

1. `ProviderRegistry`：保存 provider 定义
2. `LlmRegistry`：保存 llm 定义，并能按 llm id 解出 provider + model config
3. `ResolvedAgentBinding`：Agent 最终生效的 provider / llm / model config
4. `LlmClientFactory`：根据 `ResolvedAgentBinding` 创建具体 client

---

## 数据模型调整

### 新增配置模型

1. `providers: HashMap<String, ProviderConfig>`
2. `llms: HashMap<String, RegisteredLlmConfig>`
3. `defaults: DefaultBindingConfig`
4. `AgentSpec.provider: String`
5. `AgentSpec.llm: Option<String>`

### 新增运行时模型

1. `ResolvedProviderConfig`
2. `ResolvedLlmConfig`
3. `ResolvedAgentBinding`
4. `ProviderRegistry`
5. `LlmRegistry`

### 校验规则

1. `providers` 不能为空
2. `llms` 不能为空
3. 每个 `llm.provider` 必须引用已存在 provider
4. 每个 `agent.provider` 必须引用已存在 provider
5. 每个 `agent.llm` 若存在，必须引用已存在 llm
6. `agent.llm` 若存在，则该 llm 的 `provider` 必须等于 `agent.provider`
7. `defaults.provider` 与 `defaults.llm` 必须存在且相互匹配

---

## 迁移策略

为避免直接打断现有配置，建议保留一轮兼容迁移：

1. 旧 `[provider]` 自动迁移为 `providers.default`
2. 旧 `[llm]` 自动迁移为 `llms.default`
3. 若旧配置未给 Agent 指定 `provider`，则自动补成 `default`
4. 若旧配置未给 Agent 指定 `llm`，则自动补成 `default`
5. 启动时输出一次 `warn!`，提示用户迁移到新结构

兼容窗口结束后，再移除旧字段解析。

---

## Plan 拆分

| Plan | 标题 | 职责 | 依赖 | 状态 |
|---|---|---|---|---|
| **Plan 1** | 配置模型与迁移 | 重构 TOML 结构、配置解析、兼容迁移与校验规则 | 无 | 已完成 |
| **Plan 2** | 运行时绑定与客户端装配 | 将 Agent/会话解析到 Provider + LLM 绑定，并替换全局单 client 假设 | Plan 1 | 已完成 |
| **Plan 3** | 观测、示例与测试补齐 | 更新示例配置、inspect/runtime 观测输出与回归测试 | Plan 1, Plan 2 | 已完成 |

执行顺序：Plan 1 → Plan 2 → Plan 3

---

## 风险与待定项

| 类型 | 描述 | 缓解措施 |
|---|---|---|
| **兼容复杂度** | 旧 `[provider]` / `[llm]` 与新结构并存，迁移逻辑会变长 | 明确只保留一轮兼容，并集中在 `RawAppConfig::migrate` |
| **运行时分叉** | 顶级 Agent、子 Agent、语音服务可能分别走不同 Provider | 先只覆盖文本 LLM 路径，语音配置单独维持现状 |
| **观测口径不一致** | 当前很多地方仍把 provider 写成 `"default"` | Plan 3 统一改成真实 provider id |
| **CLI 覆写语义变化** | `--model` / `--base-url` 只适配单 provider 世界 | 后续需要补 `--provider` / `--llm` 覆写策略 |
| **后续需求扩张** | 可能很快出现“一个 Agent 两个模型”的需求 | 首版保持 `1 agent -> 1 provider -> 1 default llm`，后续再扩展双模型绑定 |
