# Plan 3: 启动集成与会话级环境一致性

## 前置依赖
Plan 1：配置项增强与默认路径解析  
Plan 2：ProjectManager 工具与工具上下文扩展

## 本次目标
将默认项目目录和动态切换能力真正接入运行时，确保新会话初始化、Turn 构建、工具相对路径解析三条链路都读取同一个会话级 `project_dir`。

## 涉及文件
- `D:\git\zero-nova\crates\nova-agent\src\app\bootstrap.rs`
- `D:\git\zero-nova\crates\nova-agent\src\app\application.rs`
- `D:\git\zero-nova\crates\nova-agent\src\conversation\control.rs`
- `D:\git\zero-nova\crates\nova-agent\src\conversation\service.rs`
- `D:\git\zero-nova\crates\nova-agent\src\app\conversation_service.rs`
- `D:\git\zero-nova\crates\nova-agent\src\agent.rs`
- `D:\git\zero-nova\crates\nova-agent\tests\integration\project_dir.rs`
- 需要时补充新的集成测试文件

## 详细设计

### 1. 新会话默认目录来源
当前 `SessionService::create` 内部通过 `ControlState::new(&agent_id)` 初始化 `project_dir`，默认落到进程 `current_dir`。要接入配置，需显式把“默认项目目录”从外部传入，而不是继续由 `ControlState` 隐式决定。

推荐改法：
- 为 `ControlState` 增加类似 `new_with_project_dir(default_agent: &str, project_dir: PathBuf)` 的构造函数
- 或让 `SessionService::create` 接受 `initial_project_dir: PathBuf`

推荐前者，影响面更小，语义也更贴近“控制状态初始化”。

### 2. bootstrap 阶段的默认目录解析
`build_application` 当前直接：

- `std::env::current_dir()` 作为 `default_project_dir`
- 用该路径采集 `EnvironmentSnapshot`
- 预加载项目上下文
- 为初始 `PromptConfig` 写入项目目录

需要改为：

1. 先读取 `config.default_project_dir()`
2. 异步校验该路径是否可用
3. 可用则使用配置路径
4. 不可用则记录 `warn!` 并回退到 `current_dir`

然后再基于最终结果构建：
- 初始环境快照
- 项目上下文
- Agent descriptor 的初始 prompt

### 3. ProjectManager 成功切换后的运行时一致性
仅更新 `Session.control.project_dir` 还不够，还要保证下一轮工具执行也看到新路径。

因此需要把 `ToolContext.environment` 的来源从：

- `agent.config.initial_env_snapshot`

改成：

- 当前 turn / 当前 session 对应的最新 `EnvironmentSnapshot`

实现路径可选：

#### 方案 A：在 `AgentRuntime` 执行工具前注入当前 turn 的 environment
- 在 turn 准备阶段把 `EnvironmentSnapshot` 放入 `TurnContext`
- 在 `execute_tool_calls` 构造 `ToolContext` 时使用 turn 级 environment

这是推荐方案，因为 prompt 与工具天然共享同一份 turn 级上下文。

#### 方案 B：在工具执行时按 session_id 现查 environment
- 通过 `session_id` 和服务句柄实时采集

该方案会让每次工具调用重复采集环境，成本更高，也会把工具层和会话层耦合得更重。

### 4. `use_turn_context` 的兼容策略
当前仅 `use_turn_context = true` 时会在 `ConversationService` 内按 session 目录重建 `PromptConfig` 与环境快照；旧 `run_turn` 路径没有这个能力。

本 Plan 需要做一个明确选择：

- 方案 1：把项目管理增强明确限定在 `use_turn_context = true`
- 方案 2：同步补齐旧 `run_turn` 路径的环境与 prompt 构造

推荐方案 2，如果改动可控。否则至少需要：
- 在设计文档中明确功能前提
- 在无 turn-context 时记录告警
- 补测试覆盖该退化行为

### 5. reset 语义统一
需要明确 `reset_project_dir` 的行为到底回退到哪里。

当前实现回退到进程 `current_dir`。在引入配置默认值后，建议统一为：

1. 若配置了 `tool.default_project_dir`，回退到该配置对应的最终有效路径
2. 否则回退到进程 `current_dir`

否则“reset”会与“新会话默认值”产生不一致，用户容易困惑。

### 6. 可观测性
建议在以下场景保留单次 `info!` / `warn!`：
- 使用配置默认目录初始化应用
- 配置目录不可用并回退
- ProjectManager 触发目录切换成功
- ProjectManager 触发目录重置成功

同一错误不应在工具层、应用层、会话层重复打印。

## 测试案例

- 集成测试 1：配置默认目录后，新 session 初始 `project_dir` 正确
  - 场景：`tool.default_project_dir` 指向有效目录
  - 预期：创建 session 后读取到该目录，而不是进程 `current_dir`

- 集成测试 2：默认目录无效时回退到 `current_dir`
  - 场景：配置目录不存在
  - 预期：应用继续启动，新 session 回退到 `current_dir`

- 集成测试 3：切换目录后，下一轮 prompt 环境使用新目录
  - 场景：执行 `ProjectManager.set` 后启动一轮对话
  - 预期：turn 级 `PromptConfig` / `EnvironmentSnapshot` 中的 `project_dir` 为新值

- 集成测试 4：切换目录后，相对路径工具基于新目录解析
  - 场景：先切目录，再使用 `Read`/`Write`/`Edit` 读取相对路径
  - 预期：路径解析以新 `project_dir` 为根，而不是应用启动目录

- 集成测试 5：`reset` 恢复到统一默认来源
  - 场景：配置了 `default_project_dir`，切换到其他目录后执行 reset
  - 预期：回退到配置默认目录，而不是硬编码 `current_dir`

- 集成测试 6：`use_turn_context = false` 的兼容行为
  - 场景：关闭 turn-context
  - 预期：若本次未补齐旧路径，则测试应明确验证并记录该限制；若已补齐，则验证行为与 turn-context 模式一致
