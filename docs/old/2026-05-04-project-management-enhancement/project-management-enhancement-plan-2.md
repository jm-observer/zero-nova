# Plan 2: ProjectManager 工具与工具上下文扩展

## 前置依赖
Plan 1：配置项增强与默认路径解析

## 本次目标
新增一个可被 LLM 调用的 `ProjectManager` 内置工具，用于查询、切换、重置当前会话的项目目录，并补足该工具所需的会话级执行上下文。

## 涉及文件
- `D:\git\zero-nova\crates\nova-agent\src\tool\builtin\project_manager.rs`（新增）
- `D:\git\zero-nova\crates\nova-agent\src\tool\builtin\mod.rs`
- `D:\git\zero-nova\crates\nova-agent\src\tool.rs`
- `D:\git\zero-nova\crates\nova-agent\src\agent.rs`
- 如需引入专用 trait：`D:\git\zero-nova\crates\nova-agent\src\app\application.rs` 或独立 `app` 子模块

## 详细设计

### 1. 现状约束
当前 `ToolContext` 仅包含：
- `event_tx`
- `tool_use_id`
- `task_store`
- `skill_registry`
- `read_files`
- `environment`

它没有：
- `session_id`
- 会话应用服务访问入口

因此新工具无法知道要更新哪个 session，也无法调用现有 `get/set/reset_project_dir` 逻辑。

### 2. 推荐的依赖注入方式
定义一个最小接口，例如：

```rust
#[async_trait]
pub trait ProjectDirService: Send + Sync {
    async fn get_project_dir(&self, session_id: &str) -> Result<PathBuf>;
    async fn set_project_dir(&self, session_id: &str, project_dir: PathBuf) -> Result<PathBuf>;
    async fn reset_project_dir(&self, session_id: &str) -> Result<PathBuf>;
}
```

然后：
- 在 `ToolContext` 中新增 `session_id: String`
- 在 `ProjectManagerTool` 构造函数中注入 `Arc<dyn ProjectDirService>`

这样工具层只依赖项目目录管理能力，不直接依赖完整 `AgentApplication`。

### 3. 工具输入模型
建议将单工具设计为多动作协议，而不是拆成多个工具名：

```json
{
  "type": "object",
  "properties": {
    "action": {
      "type": "string",
      "enum": ["get", "set", "reset"]
    },
    "path": {
      "type": "string",
      "description": "Required when action is 'set'"
    }
  },
  "required": ["action"]
}
```

原因：
- 降低工具数量和注册复杂度
- 让模型更容易在一次工具选择中完成查询/切换/重置
- 与 `TaskUpdate` 这类动作型工具风格一致

### 4. 工具语义

#### `action = "get"`
- 读取当前 session 的 `project_dir`
- 返回规范化后的当前路径字符串

#### `action = "set"`
- 校验 `path` 非空
- 调用现有应用层 `set_project_dir`
- 使用底层已有的 canonicalize + 持久化逻辑
- 返回更新后的最终路径

#### `action = "reset"`
- 调用现有应用层 `reset_project_dir`
- 返回回退后的路径

### 5. 错误处理
工具本身不应重复实现文件系统校验，应复用底层 `set_project_dir` 能力，并把错误转成清晰文本返回。建议错误粒度至少覆盖：
- 缺少 `path`
- session 不存在
- 路径设置失败

如果后续希望在设置前显式拦截“路径不存在 / 不是目录”，应将这部分校验下沉到 `SessionService::set_project_dir`，而不是仅存在于工具层，避免 UI/API/工具行为分叉。

### 6. 注册策略
在 `register_builtin_tools` 中直接注册 `ProjectManager`，不走 deferred tool。原因：
- 这是基础会话控制能力，应该始终可用
- 使用频率高，不适合要求模型先通过 `ToolSearch` 再加载

### 7. 用户交互示例

- User: `当前项目目录是什么？`
- Agent: 调用 `ProjectManager({"action":"get"})`
- Tool: 返回 `Current project directory: D:/projects/alpha`

- User: `切换到 D:/projects/beta`
- Agent: 调用 `ProjectManager({"action":"set","path":"D:/projects/beta"})`
- Tool: 返回 `Project directory updated to: D:/projects/beta`

- User: `恢复到默认目录`
- Agent: 调用 `ProjectManager({"action":"reset"})`
- Tool: 返回 `Project directory reset to: D:/git/zero-nova`

## 测试案例

- 测试 1：`get` 返回当前会话目录
  - 场景：session 已存在且 `project_dir` 为默认值
  - 预期：工具返回当前目录，`is_error = false`

- 测试 2：`set` 成功更新目录
  - 场景：传入有效目录
  - 预期：调用底层服务成功，返回更新后的规范化路径

- 测试 3：`reset` 成功回退目录
  - 场景：session 已切换到其他目录后执行 reset
  - 预期：回退到默认来源定义的目录

- 测试 4：`set` 缺少 path
  - 场景：`action = "set"` 但无 `path`
  - 预期：工具返回参数错误

- 测试 5：工具上下文缺少 session_id
  - 场景：异常构造或未来回归
  - 预期：工具明确返回内部上下文缺失错误，而不是 silent failure
