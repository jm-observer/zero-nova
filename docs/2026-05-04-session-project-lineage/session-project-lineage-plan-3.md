# Plan 3: 无 Project 运行时行为与测试补齐

## 前置依赖
Plan 1: Session Project 数据模型收敛  
Plan 2: 同 Agent 最近 Session 继承与恢复

## 本次目标
在 `project = None` 成为合法状态后，统一 prompt、项目上下文、工具路径解析和项目管理工具的行为，并补齐集成测试，确保系统不会再偷偷回退到当前工作目录。

## 涉及文件
- `D:\git\zero-nova\crates\nova-agent\src\app\conversation_service.rs`
- `D:\git\zero-nova\crates\nova-agent\src\prompt.rs`
- `D:\git\zero-nova\crates\nova-agent\src\tool.rs`
- `D:\git\zero-nova\crates\nova-agent\src\tool\builtin\project_manager.rs`
- `D:\git\zero-nova\crates\nova-agent\src\tool\builtin\mod.rs`
- `D:\git\zero-nova\crates\nova-agent\tests\integration\` 下相关集成测试

## 详细设计

### 1. Prompt 层支持 `project = None`
当前 `PromptConfig` 和 `EnvironmentSnapshot` 默认假设存在项目目录。改造后需要：
- `PromptConfig` 支持 `project_dir: Option<PathBuf>`
- prompt 渲染时若为空，输出明确文案，例如 `Project directory: (not set)`
- 不再尝试拼接项目上下文路径

目标是让模型知道当前没有项目，而不是继续收到误导性的目录信息。

### 2. 项目上下文加载改为可选
当前 `load_project_context_with_config(_project_dir, ...)` 依赖 project 路径。改造后：
- 若 `project_dir = None`，直接返回 `None`
- 不查找 `PROJECT.md`
- 不记录错误日志

这属于正常分支，不应当被当作异常。

### 3. 工具路径解析的无 project 行为
当前路径解析默认从 `environment.project_dir` 出发。改造后需要区分：

- 绝对路径：仍允许直接解析
- 相对路径 + `project_dir = Some(path)`：按该路径解析
- 相对路径 + `project_dir = None`：返回明确错误

推荐错误文案方向：

```text
Current session has no project directory. Set a project before using relative paths.
```

这样对 LLM 和用户都足够清晰。

### 4. ProjectManager 工具动作收敛
既然不引入 reset，工具只保留：
- `get`
- `set`

建议语义：
- `get`：若当前 session `project_dir = None`，返回“未设置”
- `set`：规范化路径并写回当前 session

工具不再提供默认 project 或 reset 相关语义，避免与新的 session 模型冲突。

### 5. Turn 级环境一致性
当前 `ConversationService::execute_agent_turn` 在 turn-context 路径下会重新读取 session 的 project。改造后要保证：
- `project_dir = None` 时也能正常构造 turn
- `ToolContext.environment` 能表达“无项目”状态
- 所有依赖 environment 的工具都不再假设 project 非空

### 6. 旧行为清理
需要清理以下旧假设：
- `bootstrap` 启动时预采集的默认项目上下文
- `project_context_file` 与默认项目目录的强绑定
- 任意使用 `current_dir` 作为 session project 兜底的逻辑

## 测试案例

- 集成测试 1：新建 `project_dir = None` 的 session 后，prompt 显示“未设置项目目录”
- 集成测试 2：`project_dir = None` 时不加载 `PROJECT.md`
- 集成测试 3：`project_dir = None` 时，对相对路径 `Read` / `Write` / `Edit` 返回明确错误
- 集成测试 4：`project_dir = None` 时，绝对路径工具调用仍可工作
- 集成测试 5：`ProjectManager.get` 在空 project 下返回“未设置”，`set` 后返回规范化路径
- 集成测试 6：同 Agent 最近 session 继承的 `project_dir` 会在下一轮 turn 的 prompt 与工具环境中保持一致
