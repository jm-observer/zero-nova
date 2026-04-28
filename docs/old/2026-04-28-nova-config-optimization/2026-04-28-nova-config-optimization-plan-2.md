# Plan 2：加载、兼容与校验策略

## 前置依赖
- Plan 1：配置模型对齐与分层重构

## 本次目标
- 定义新旧配置字段之间的兼容映射。
- 建立启动阶段的显式校验与告警机制。
- 明确端口、路径、密钥等关键字段的优先级规则。
- 明确兼容映射的实现位置和环境变量覆盖时机。

## 涉及文件
- `crates/nova-agent/src/config.rs`
- `crates/nova-agent/src/app/bootstrap.rs`
- `crates/nova-server/src/bin/nova_gateway_ws.rs`
- `crates/nova-server/src/bin/nova_gateway_stdio.rs`
- `deskapp/src-tauri/src/config.rs`
- `.nova/config.toml`

## 详细设计

### 1. 兼容策略总原则

配置兼容采用"短期兼容、启动告警、后续移除"的策略：

- 当前版本：接受旧字段，映射到新字段，并打印一次 warning
- 下一个阶段：文档中标记废弃字段
- 再下一个阶段：移除兼容映射

这样可以避免用户本地配置在一次升级后全部失效。

### 2. 兼容映射实现方案

#### 2.0 实现位置与执行流程

兼容映射集中在 `crates/nova-agent/src/config.rs` 中实现，通过新增 `migrate()` 方法完成。

完整的配置加载流程：

```
TOML 文件
  → toml::from_str::<RawAppConfig>()   // 第 1 步：宽松反序列化（接受新旧字段）
  → RawAppConfig::migrate()            // 第 2 步：旧字段映射 + 兼容 warning
  → OriginAppConfig                    // 第 3 步：得到规范化的配置
  → apply_env_overrides()              // 第 4 步：环境变量覆盖
  → validate()                         // 第 5 步：校验
  → AppConfig::from_origin()           // 第 6 步：注入 workspace，构造最终配置
```

其中 `RawAppConfig` 是一个中间结构体，同时包含新旧字段：

```rust
/// 宽松反序列化用的中间结构体，同时接受新旧字段名
#[derive(Debug, Deserialize)]
struct RawAppConfig {
    // 新字段
    #[serde(default)]
    provider: Option<ProviderConfig>,
    #[serde(default)]
    llm: Option<RawLlmConfig>,
    // ... 其他字段 ...
}

/// 兼容期间的 LLM 配置，同时包含新旧字段
#[derive(Debug, Deserialize)]
struct RawLlmConfig {
    // 旧字段（provider 拆分前）
    api_key: Option<String>,
    base_url: Option<String>,
    // 新字段
    #[serde(flatten)]
    model_config: ModelConfig,
}

impl RawAppConfig {
    fn migrate(self) -> (OriginAppConfig, Vec<String>) {
        let mut warnings = Vec::new();
        // ... 映射逻辑，每条映射产生一条 warning ...
        (config, warnings)
    }
}
```

#### 2.1 `llm` 拆分为 `provider` + `llm`

兼容规则：

- 旧写法：
  - `llm.api_key` → `provider.api_key`
  - `llm.base_url` → `provider.base_url`
  - `llm.model` → `llm.model`（位置不变）
  - `llm.max_tokens` → `llm.max_tokens`（位置不变）
  - `llm.temperature` → `llm.temperature`（位置不变）
  - `llm.top_p` → `llm.top_p`（位置不变）
  - `llm.thinking_budget` → `llm.thinking_budget`（位置不变）
  - `llm.reasoning_effort` → `llm.reasoning_effort`（位置不变）
- 若新旧字段同时存在（即同时存在 `[provider].api_key` 和 `[llm].api_key`）：新字段优先，旧字段忽略并输出 warning

补充约束：

- `thinking_budget` 与 `reasoning_effort` 做互斥校验；若两者同时设置，输出 warning 并优先使用 `thinking_budget`（因为它提供了更精确的控制）

#### 2.2 Agent Prompt 字段

兼容规则：

- 如果存在 `prompt_file`：按新语义处理
- 否则如果存在 `prompt_inline`：按新语义处理
- 否则如果存在旧字段 `system_prompt_template`：
  - 若值看起来像文件名或相对路径（如以 `.md` 结尾）：映射为 `prompt_file`
  - 否则映射为 `prompt_inline`
  - 两种情况均输出 warning，提示用户迁移到新字段

这样可以兼容当前 `.nova/config.toml` 与 `.nova/examples/agents.toml` 的习惯写法。

#### 2.3 Trimmer 字段

旧字段语义澄清：

- `max_history_tokens`：旧配置中表示"允许保留的历史消息最大 token 数"，语义上是**仅输入侧**的限额，不包含输出预留。

兼容规则：

- `max_history_tokens` → 折算为新字段：
  - `context_window = max_history_tokens + output_reserve`
  - 若旧配置未提供 `output_reserve`，采用默认值 `8192`
  - 同时设置 `enabled = true`（因为旧配置存在该字段说明用户有意启用裁剪）
- `preserve_recent` → `min_recent_messages`
- `preserve_tool_pairs` → 不映射，输出 warning 说明该字段当前代码未实现

说明：

- `preserve_tool_pairs` 当前代码并无直接消费路径，不能伪装成"已支持字段"。
- 若业务上确实需要保留工具调用配对，应在后续单独为 Trimmer 补实现，而不是继续保留失效字段。

#### 2.4 网关相关字段

兼容规则：

- `[gateway.router]`、`[gateway.interaction]`、`[gateway.workflow]` 若存在：
  - 当前版本不报错
  - 记录 warning：说明这些配置暂未生效

### 3. 关键字段优先级定义

#### 3.1 Host / Port

优先级：

1. CLI 参数：`nova_gateway_ws --host/--port`
2. `gateway.host` / `gateway.port`
3. 代码默认值（`127.0.0.1:18801`，Plan 1 已修正）

说明：

- `remote.host` / `remote.port` 不参与服务端监听，只影响桌面端连接默认值。
- DeskApp 在 auto sidecar 模式下，应优先使用其实际注入给 sidecar 的端口构造连接地址，而不是单纯信任 `remote.port`。

#### 3.2 Sidecar Command

优先级：

1. 绝对路径 `sidecar.command`
2. PATH 中的可执行命令名
3. 与当前可执行文件同目录的候选文件

不再推荐把 `target/debug/...` 作为仓库默认配置。

#### 3.3 API Key / Search Key

环境变量覆盖规则：

- `provider.api_key`（注意：拆分后不再是 `llm.api_key`）：允许被 `NOVA_API_KEY` 覆盖
- `search.tavily_api_key`：允许被 `TAVILY_API_KEY` 覆盖

推荐顺序：

1. 环境变量
2. 配置文件
3. 空值（启动时报错或在对应能力启用时报错）

环境变量覆盖发生在 `apply_env_overrides()` 阶段（即 migrate 之后、validate 之前），确保：
- 旧字段已经映射到新位置
- 环境变量优先级高于配置文件中的任何写法（无论新旧）
- 校验阶段看到的是最终生效值

### 4. 启动阶段校验

在 `validate()` 阶段增加轻量校验逻辑，覆盖以下问题：

- `[[gateway.agents]]` 不能为空
- Agent `id` 不允许重复
- `prompt_file` / `prompt_inline` 不能同时存在
- `skill_history_strategy` 只允许 `global | per_skill | segments`
- `sidecar.mode` 只允许 `auto | manual`
- 开启 `search.backend = "tavily"` 时，必须存在 `tavily_api_key` 或对应环境变量
- `gateway.port` 与 `remote.port` 不一致时，给出说明性日志而非静默接受
- `thinking_budget` 和 `reasoning_effort` 不能同时设置

### 5. 错误与日志策略

遵循"不要静默吞错"的要求：

- 配置结构非法（反序列化失败）：直接返回错误，阻止启动
- 校验规则未通过（如 agents 为空、字段互斥冲突）：直接返回错误，阻止启动
- 配置可兼容但含废弃字段：记录一次 warning，程序继续运行
- 配置存在无效区块：记录一次 warning，并说明不会生效
- 示例配置中的占位密钥：不记录 error，仅在实际启用对应能力且未替换时返回明确错误

## 测试案例

- 正常路径：仅使用新字段时，配置加载成功且无 warning。
- 正常路径：仅使用旧字段时，配置加载成功并产生预期 warning。
- 正常路径：环境变量覆盖 `provider.api_key` 后，最终生效值为环境变量值。
- 正常路径：`migrate()` 将旧 `llm.api_key` 正确映射到 `provider.api_key`。
- 边界条件：`gateway.port` 与 `remote.port` 不一致时，DeskApp 与 Gateway 各自按既定职责运行。
- 边界条件：旧 `max_history_tokens = 50000` 折算为 `context_window = 58192`、`output_reserve = 8192`。
- 边界条件：`thinking_budget` 和 `reasoning_effort` 同时存在时，使用 `thinking_budget` 并输出 warning。
- 异常场景：`[[gateway.agents]]` 为空时，启动失败。
- 异常场景：`prompt_file` 和 `prompt_inline` 同时设置时，启动失败。
- 异常场景：`search.backend = "tavily"` 且缺失密钥时，返回明确错误。
- 异常场景：Agent `id` 重复时，启动失败并指出重复的 id。
