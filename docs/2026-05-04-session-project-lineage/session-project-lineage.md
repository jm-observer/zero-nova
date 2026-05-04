# Session Project 继承设计

| 章节 | 说明 |
|-----------|------|
| 时间 | 创建：2026-05-04<br>最后更新：2026-05-04 |
| 项目现状 | 1. `ControlState.project_dir` 当前为必填 `PathBuf`，默认回退到进程 `current_dir`。<br>2. `SessionService::create` 依赖 `default_project_dir` 创建新 session。<br>3. `ConversationService::switch_agent` 只在当前 session 内切换 `active_agent`，不会按 Agent 恢复最近 session。<br>4. `reset_project_dir` 已存在于应用层和工具层接口中，但该动作不符合最新业务语义。 |
| 整体目标 | 1. 将 `project` 明确为纯 `Session` 级运行态字段，并允许为空。<br>2. 将“最近 session”的继承与恢复边界限制在同一 `Agent` 内，判定依据为最近活跃时间。<br>3. 删除默认 project 配置和 reset 语义，保证 prompt、项目上下文、工具路径解析都能正确处理 `project = None`。 |
| 非目标 | 1. 不在本次设计中引入 `Agent.project`、`tool.default_project_dir` 或其他默认 project 配置。<br>2. 不在本次设计中实现跨 Agent 的 session/project 继承。<br>3. 不在本次设计中引入多活跃 session 调度策略或 session 分组管理。 |

## Plan 拆分

| 状态 | Plan | 说明 | 依赖 | 执行顺序 |
|------|------|------|------|----------|
| 待开始 | Plan 1: Session Project 数据模型收敛 | 将 `project` 从必填路径改为 session 级可空状态，并移除默认 project / reset 相关接口。 | 无 | 1 |
| 待开始 | Plan 2: 同 Agent 最近 Session 继承与恢复 | 以最近活跃时间为准实现 Agent 内 session 恢复与新建继承规则。 | Plan 1 | 2 |
| 待开始 | Plan 3: 无 Project 运行时行为与测试补齐 | 统一 prompt、工具、项目上下文在 `project = None` 下的行为，并补齐端到端测试。 | Plan 1, Plan 2 | 3 |

## 核心业务语义

### 1. 归属关系
- `project` 只属于 `Session`，不属于 `Agent`，也不属于 `Tool` 配置。
- `Agent` 只表示角色入口、提示词与工具能力，不携带默认项目目录。
- `Tool` 只消费当前 session 的 `project`，不得定义默认 project 来源。

### 2. 继承边界
- 新建 session 时，只允许继承“同 Agent 最近活跃 session”的 `project`。
- 不允许从 Agent A 的当前 session 继承到 Agent B 的新 session。
- 若目标 Agent 没有任何历史 session，允许创建 `project = None` 的新 session。

### 3. 恢复规则
- 切换到某个 Agent 时，优先恢复该 Agent 最近活跃的 session。
- “最近 session” 的判定依据是最近活跃时间，即现有 session 的 `updated_at`。
- 若该 Agent 没有历史 session，则允许直接创建一个 `project = None` 的新 session。

### 4. 无 Project 语义
- `project = None` 是合法业务状态，不表示错误。
- 当 session 未绑定 project 时：
- prompt 中应明确展示“未设置项目目录”。
- 不加载项目上下文文件。
- 基于 project 根目录解析相对路径的工具应返回明确错误，提示先设置 project。
- 不应偷偷回退到进程 `current_dir`，避免把“无 project”错误伪装成“有默认目录”。

## 当前实现与目标语义的主要偏差

### 1. `project_dir` 当前强制非空
`ControlState.project_dir` 为 `PathBuf`，并通过 `default_project_dir()` 回退到 `current_dir`。这会导致：
- 无法表达 `project = None`
- 旧 session 反序列化时会隐式填充目录
- 工具层和 prompt 层无法区分“用户未设置项目”与“项目就是当前目录”

### 2. Session 创建依赖全局默认目录
`SessionService` 当前持有 `default_project_dir`，`create()` 固定用该值初始化。这与“从同 Agent 最近 session 继承，否则为 None”的规则冲突。

### 3. Agent 切换仍是单 Session 内部切换
`ConversationService::switch_agent` 当前只修改 `active_agent`，没有“按 Agent 找回最近 session”的路由语义。这会让不同 Agent 共享同一个 session 上下文，破坏继承边界。

### 4. reset 语义与业务模型冲突
当前代码存在 `reset_project_dir`，并回退到 `default_project_dir`。在新模型中既没有默认 project，也不需要 reset，因此应整体移除，而不是重新解释。

## 关键设计决策

### 1. `project` 改为可空
- `ControlState.project_dir` 调整为 `Option<PathBuf>`，建议同步更名为 `project` 或 `project_dir: Option<PathBuf>`。
- 为兼容现有序列化数据，旧字段缺失或旧结构解码失败时要有明确迁移策略。

推荐保留字段名 `project_dir`，只改类型为 `Option<PathBuf>`。这样数据库中的 `runtime_control` JSON 迁移成本更低。

### 2. 最近活跃时间直接复用 `updated_at`
- session 最近活跃判定以持久化层已有的 `updated_at` 为准。
- 不额外引入 `last_active_at` 字段，避免重复状态。
- 所有会改变 session 活跃性的操作都必须保证 `updated_at` 被刷新。

### 3. Agent 切换语义升级为“恢复 session”
- “切换 Agent”不再只是改写当前 session 的 `active_agent`。
- 应升级为：
- 查找该 Agent 最近活跃 session
- 找到则直接返回该 session
- 未找到则创建一个新的 `project = None` session，并将其作为该 Agent 当前 session

这意味着接口层可能需要把“切换 agent”的返回从 `AgentDescriptor` 扩展为“Agent + Session”联合结果，或新增专门的恢复接口。

### 4. 新建 Session 的继承规则
- 显式创建某 Agent 的新 session 时：
- 先查该 Agent 最近活跃 session
- 若存在，则复制其 `project`
- 若不存在，则写入 `None`

“继承”只复制 `project`，不复制消息历史、工具状态、模型覆盖等其他运行态。

### 5. `project = None` 时禁止静默路径回退
- prompt 可以无项目运行
- 工具不能假装仍有 project 根目录
- 对于 `Read` / `Write` / `Edit` 这类依赖相对路径基准的工具，若传入相对路径且当前 session 没有 project，应返回可读错误

这样能把状态问题显式暴露给用户和模型，避免误操作落到错误目录。

## 风险与待定项

### 已知风险
- `switch_agent` 若改成“恢复最近 session”，会影响现有前端或调用方对返回值和 UI 流程的假设。
- `project_dir: Option<PathBuf>` 会影响 prompt 构造、工具路径解析、项目上下文加载等多条链路，若有一处仍假设非空，会产生运行时错误。
- 老数据中 `runtime_control.project_dir` 当前总是字符串路径，迁移时需要兼容旧值和缺省值。

### 待确认项
- `switch_agent` 是否直接创建新 session，还是先返回“无历史 session”再由上层显式调用创建。当前文档按“允许直接创建 `project = None` session”设计。
- 前端或协议层是否需要显式暴露“该 Agent 最近 session id”，以便 UI 切换时不丢失上下文。
- `ProjectManager` 工具是否只保留 `get` / `set` 两个动作。当前文档默认不引入 `reset`。

## 与旧设计的关系

- 本设计替代 [project-management-enhancement.md](/D:/git/zero-nova/docs/2026-05-04-project-management-enhancement/project-management-enhancement.md) 中关于 `tool.default_project_dir`、`Agent.project` 倾向和 `reset_project_dir` 的相关假设。
- 旧文档仍可保留用于追溯讨论过程，但后续实现应以本目录文档为准。
