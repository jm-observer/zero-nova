# Zero Nova 配置文件优化设计

## 时间
- 创建时间：2026-04-28
- 最后更新：2026-04-28

## 项目现状

当前项目的 `.nova/config.toml` 同时承载了以下几类配置：

- `crates/nova-agent` 的运行时配置：`llm`、`search`、`tool`、`gateway`、`voice`
- `crates/nova-server` 的启动参数回填：`gateway.host`、`gateway.port`
- `deskapp/src-tauri` 的桌面端配置：`remote`、`sidecar`
- Agent 注册与 Prompt 选择：`[[gateway.agents]]`

结合代码现状，当前配置文件存在以下核心问题：

1. 配置结构与代码模型未完全对齐
   - `.nova/config.toml` 中存在 `[gateway.router]`、`[gateway.interaction]`、`[gateway.workflow]` 等区块，但 `crates/nova-agent/src/config.rs` 中没有对应结构体，反序列化后会被静默忽略。
   - `[gateway.trimmer]` 当前使用的是 `max_history_tokens`、`preserve_recent`、`preserve_tool_pairs`，但代码实际读取的是 `enabled`、`context_window`、`output_reserve`、`min_recent_messages`，导致用户填写的值不会生效。

2. 配置语义存在误导
   - `[[gateway.agents]].system_prompt_template` 在配置中被当成"模板文件名"使用（例如 `"agent-nova.md"`），但 `crates/nova-agent/src/app/bootstrap.rs:80-81` 中只要该字段为 `Some`，就会直接将字段值作为 prompt 正文使用，不会尝试读取文件。这意味着当前配置中写的 `"agent-nova.md"` 字符串会被原样当成 system prompt 发给模型，而不是去加载同名文件——实际 prompt 来自 `None` 分支的自动推导路径 `agent-{id}.md`。
   - `[gateway].port`、`[remote].port`、`[sidecar].port_arg` 共同影响同一个网关访问入口，但责任边界没有明确定义，容易造成"配置已改但实际监听端口未按预期生效"的困惑。
   - `.nova/config.toml` 的 `[llm]` 同时承载了"供应商接入配置"和"推理请求默认参数"两类职责：`api_key`、`base_url` 更偏 provider 连接信息，而 `model`、`max_tokens`、`temperature`、`top_p`、`thinking_budget` 更偏模型调用参数，当前混放会放大后续多供应商、多模型场景下的耦合。

3. 可移植性与安全性不足
   - 当前示例中存在真实风格的密钥值，不适合继续作为仓库内默认配置。
   - `[sidecar].command` 指向绝对路径 `D:\git\zero-nova\target\debug\nova_gateway.exe`，与具体机器、构建目录、构建 profile 强耦合。

4. 已支持能力没有在配置中被显式表达
   - `tool.skills_dir`、`tool.prompts_dir`、`tool.project_context_file`、`tool.default_policy`
   - `gateway.skill_routing_enabled`、`gateway.skill_history_strategy`
   - `gateway.side_channel.*`
   这些字段代码已支持，但当前配置文件没有形成稳定、清晰、可维护的默认写法。

5. 缺少"配置所有者"与"兼容策略"设计
   - Agent Runtime、WS Gateway、Tauri DeskApp 共用一个 TOML 文件，但没有定义哪些字段由谁消费、哪些字段允许缺省、哪些字段保留兼容映射。

6. 代码默认值与配置文件不一致
   - `GatewayConfig::default_port()` 返回 `9090`，但 `.nova/config.toml` 和文档中均使用 `18801`。两者之间没有显式约定，依赖"配置文件总是存在"的隐含假设。

7. `AppConfig::config_path()` 方法未消费 `config_path` 字段
   - `config.rs:344-346` 中 `config_path()` 硬编码返回 `workspace.join("config.toml")`，完全忽略了 `self.config_path` 字段。虽然现有测试覆盖了该字段的路径解析，但方法实现未对齐，属于已有 bug。

## 整体目标

本次优化目标不是单纯"重排注释"，而是为 `.nova/config.toml` 建立一套与项目实现一致、可扩展、可迁移的配置契约：

- 让配置结构与 Rust 代码模型一一对应，避免静默失效
- 明确 Agent Runtime、Gateway Server、DeskApp 三类消费者的边界
- 将示例配置改造成"可直接复制使用"的安全模板，而不是环境耦合样例
- 将当前 `[llm]` 拆分为 `provider` 与 `llm` 两层，分离连接信息与调用参数
- 为旧字段提供有限兼容，避免一次性破坏已有本地配置
- 补齐配置加载与校验测试，确保未来增量演进时不再出现"写了但没生效"的问题
- 修正代码默认值（如 `default_port`）与实际使用值的不一致
- 修复 `config_path()` 方法未消费字段的已知 bug

## 不在本次范围

- **`[voice]` 配置**：当前 `VoiceConfig` 结构已稳定，字段与代码一一对应，本次不做改动。如后续 voice 能力扩展（如多 provider、流式 STT），可单独立项。
- **多 provider 并行接入**：本次仅拆分 `provider` / `llm` 两层结构，不引入 provider 数组或按 agent 切换 provider 的能力。

## Plan 拆分

### Plan 1：配置模型对齐与分层重构
- 目标：定义新的 TOML 结构与对应 Rust 结构体，使 `.nova/config.toml` 与 `nova-agent`、`nova-server`、`deskapp` 的实际消费路径一致。
- 重点：修正无效字段、消除歧义字段、统一目录与 Prompt 配置语义、提供目标 Rust struct 定义。
- 依赖：无
- 执行顺序：第 1 步

### Plan 2：加载、兼容与校验策略
- 目标：在不立即破坏旧配置的前提下，建立显式兼容映射、校验规则与错误提示。
- 重点：旧字段兼容、端口优先级、密钥来源、路径解析与告警策略、兼容映射实现位置。
- 依赖：Plan 1
- 执行顺序：第 2 步

### Plan 3：示例配置、文档与测试收口
- 目标：让仓库中的配置样例、README、桌面端行为和自动化测试全部与新契约一致。
- 重点：模板配置、安全示例、迁移说明、配置回归测试、deskapp 兼容验证。
- 依赖：Plan 1、Plan 2
- 执行顺序：第 3 步

### Plan 状态
- Plan 1：已完成
- Plan 2：已完成
- Plan 3：待开始

## 变更影响矩阵

| 变更项 | nova_cli | nova_gateway_ws | nova_gateway_stdio | deskapp | Breaking |
|---|---|---|---|---|---|
| `[llm]` 拆分为 `[provider]` + `[llm]` | ✅ | ✅ | ✅ | ❌ (不消费) | 有兼容映射 |
| `system_prompt_template` → `prompt_file` / `prompt_inline` | ✅ | ✅ | ✅ | ❌ | 有兼容映射 |
| `[gateway.trimmer]` 字段名修正 | ✅ | ✅ | ✅ | ❌ | 有兼容映射 |
| 移除 `[gateway.router/interaction/workflow]` | ✅ | ✅ | ✅ | ❌ | 仅 warning |
| `default_port()` 从 9090 改为 18801 | ✅ | ✅ | ✅ | ❌ | 行为对齐 |
| `sidecar.command` 改为命令名 | ❌ | ❌ | ❌ | ✅ | Breaking |
| 环境变量覆盖 `NOVA_API_KEY` / `TAVILY_API_KEY` | ✅ | ✅ | ✅ | ❌ | 新增能力 |
| 修复 `config_path()` 方法 | ✅ | ✅ | ✅ | ❌ | Bug fix |

## 风险与待定项

### 已知风险
- `system_prompt_template` 当前已被部分本地配置当成"文件名"使用，若直接改语义会影响已有环境。Plan 2 已定义兼容映射策略：若值以 `.md` 结尾则映射为 `prompt_file`，否则映射为 `prompt_inline`。
- `deskapp/src-tauri/src/config.rs` 与 `crates/nova-agent/src/config.rs` 分别维护配置模型，若继续分叉演进，后续仍会产生漂移。
- `crates/nova-server/src/bin/nova_gateway_ws.rs` 会用 CLI 参数覆盖 `gateway.host` / `gateway.port`，Plan 2 已定义优先级：CLI > 配置文件 > 代码默认值。
- deskapp 的 `TomlConfig` 只反序列化 `remote` 和 `sidecar`，引入 `[provider]` 等新区块后不会导致反序列化失败（serde 默认忽略未知字段），但这一行为需要在 Plan 3 中显式测试确认。

### 已决定项（原待确认）
- **敏感字段策略**：采纳"配置文件占位 + 环境变量覆盖"方案。详见 Plan 2 第 3.3 节。
- **单文件 vs 拆分**：继续共用单个 `config.toml`，通过文档和结构注释明确各区块所有者。详见 Plan 1 第 1 节。
- **未实现区块**：`[gateway.router]`、`[gateway.interaction]`、`[gateway.workflow]` 从默认配置中移除，仅在配置中出现时输出 warning。详见 Plan 1 第 2 节。
