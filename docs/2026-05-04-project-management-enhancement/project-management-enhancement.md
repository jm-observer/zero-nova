# 项目管理增强设计 (Project Management Enhancement)

| 章节 | 说明 |
|-----------|------|
| 时间 | 创建：2026-05-04<br>最后更新：2026-05-04 |
| 项目现状 | 1. `ControlState::new` 默认将 `project_dir` 初始化为进程 `current_dir`。<br>2. `SessionService` / `ConversationService` / `AgentApplication` 已具备 `set_project_dir`、`reset_project_dir`、`get_project_dir` 能力，但该能力尚未暴露给 LLM 工具层。<br>3. `build_application` 仍以 `current_dir` 构造初始 `PromptConfig` 与 `EnvironmentSnapshot`。<br>4. `ToolContext.environment` 当前来源于 `agent.config.initial_env_snapshot`，不是按会话实时刷新，切换项目目录后工具层的相对路径解析可能仍指向旧目录。 |
| 整体目标 | 1. 支持在 `.nova/config.toml` 中声明全局默认项目目录。<br>2. 新增 `ProjectManager` 内置工具，让 Agent 可以查询、切换、重置当前会话的项目目录。<br>3. 保证会话中的提示词上下文、环境快照、相对路径解析三者都与最新 `project_dir` 一致。 |
| 非目标 | 1. 不在本次设计中引入多项目工作区或项目列表管理。<br>2. 不修改现有 `SessionService` 的持久化模型结构。<br>3. 不新增外部依赖。 |

## Plan 拆分

| 状态 | Plan | 说明 | 依赖 | 执行顺序 |
|------|------|------|------|----------|
| 待开始 | Plan 1: 配置项增强与默认路径解析 | 在配置层引入 `default_project_dir`，并定义相对路径解析、缺省值和启动期校验策略。 | 无 | 1 |
| 待开始 | Plan 2: ProjectManager 工具与工具上下文扩展 | 新增内置工具，并为工具执行链补足 `session_id` / 会话应用访问能力。 | Plan 1 | 2 |
| 待开始 | Plan 3: 启动集成与会话级环境一致性 | 将默认路径接入新会话创建、Turn 准备和工具执行环境，补齐端到端测试。 | Plan 1, Plan 2 | 3 |

## 现状分析

### 1. 已有能力
- 会话状态中已经持久化 `project_dir`，且 `SessionService::set_project_dir` 会将更新写回仓储。
- `set_project_dir` 内部已做异步 `canonicalize`，失败时保留原始路径并记录 `warn`，因此设计无需重复发明规范化逻辑。
- `ConversationService::execute_agent_turn` 在 `use_turn_context = true` 路径下会重新读取 session 的 `project_dir`，并基于该路径构造 `PromptConfig`、项目上下文和 `EnvironmentSnapshot`。

### 2. 当前缺口
- 新建会话时 `ControlState::new` 直接回落到进程 `current_dir`，无法受配置文件控制。
- `ProjectManager` 尚不存在，LLM 无法通过工具调用更新当前会话目录。
- 工具执行阶段的 `ToolContext.environment` 仍使用应用启动时采集的初始环境快照，因此即使会话的 `project_dir` 已改变，`Read` / `Write` / `Edit` 等工具对相对路径的解析仍可能基于旧目录。
- 当 `gateway.use_turn_context = false` 时，旧的 `run_turn` 路径不会按会话重建 prompt 环境，项目目录切换的收益会被部分削弱。

## 关键设计决策

### 1. 默认项目目录优先级
新会话的初始 `project_dir` 优先级定义为：

1. `tool.default_project_dir`
2. 进程 `current_dir`

这样可以保持向后兼容；未配置时维持当前行为。

### 2. 配置路径解析规则
- `default_project_dir` 使用字符串配置，载入后解析为 `PathBuf`。
- 若为绝对路径，直接使用。
- 若为相对路径，则相对 `config_dir` 解析，而不是相对进程 `current_dir`。

原因是 `AppConfig` 中 `skills_dir`、`prompts_dir`、`project_context_file` 已采用相同规则，继续保持配置语义一致。

### 3. 启动期校验策略
- 配置加载阶段只做语法解析，不强制校验目标路径存在。
- 在真正使用默认项目目录初始化会话或采集环境快照时，再执行异步存在性检查和规范化。
- 若路径不存在、不是目录或无法访问，则记录一次告警并回退到进程 `current_dir`。

原因是配置文件可能在不同机器间同步；过早失败会降低可移植性，而启动期回退更稳健。

### 4. 工具接口边界
`ProjectManager` 至少提供三个动作：

- `get_project_dir`
- `set_project_dir`
- `reset_project_dir`

不建议只暴露 `set_project_dir`。查询和重置都是高频补充动作，否则模型需要依赖自然语言记忆当前目录，稳定性不足。

### 5. 工具执行链补充上下文
由于当前 `ToolContext` 不包含 `session_id`，也没有访问 `AgentApplication` / `ConversationService` 的入口，`ProjectManager` 无法定位当前会话并完成状态写回。因此必须补充以下至少一项：

- 在 `ToolContext` 中加入 `session_id` 与项目管理服务句柄。
- 或在 `ProjectManager` 构造时注入一个专用 trait 对象，仅暴露 `get/set/reset_project_dir` 三个方法。

推荐第二种：缩小工具对应用层的依赖面，避免把整个 `AgentApplication` 暴露给工具层。

### 6. 环境一致性要求
项目目录切换成功后，以下三个位置必须读取同一个会话级目录：

- prompt 中展示的 `project_dir`
- 项目上下文加载逻辑
- 工具层相对路径解析使用的 `ToolContext.environment.project_dir`

任何一处仍然沿用应用启动时的 `current_dir`，都会导致“Agent 说已经切换，但工具还在旧目录执行”的行为偏差。

## 风险与待定项

### 已知风险
- 若仅实现配置项和工具，但不修正 `ToolContext.environment` 的来源，相对路径工具会继续错误解析路径。
- 若保留 `use_turn_context = false` 的旧路径不处理，则项目目录切换后的 prompt 上下文与工具执行结果可能不一致。
- Windows 与 Linux 的规范化结果不同，文档和测试不能假设路径分隔符固定为 `/`。

### 待定项
- 是否在本次实现中顺带将旧 `run_turn` 路径迁移为统一的 turn-context 路径。如果不迁移，需要在文档和实现中明确该功能依赖 `gateway.use_turn_context = true` 才完整生效。
- `default_project_dir` 无效时是静默回退还是启动告警后继续运行。建议采用“单次告警 + 回退”。
