# Plan 1: Session Project 数据模型收敛

## 前置依赖
无

## 本次目标
把 `project` 收敛为 session 级可空运行态字段，删除默认 project / reset 相关接口与配置依赖，为后续 session 恢复和继承规则提供稳定数据模型。

## 涉及文件
- `D:\git\zero-nova\crates\nova-agent\src\conversation\control.rs`
- `D:\git\zero-nova\crates\nova-agent\src\conversation\service.rs`
- `D:\git\zero-nova\crates\nova-agent\src\tool.rs`
- `D:\git\zero-nova\crates\nova-agent\src\app\conversation_service.rs`
- `D:\git\zero-nova\crates\nova-agent\src\app\application.rs`
- `D:\git\zero-nova\crates\nova-agent\src\app\bootstrap.rs`
- 与配置解析相关的 `config.rs`、测试文件

## 详细设计

### 1. `ControlState.project_dir` 改为 `Option<PathBuf>`
当前定义：

```rust
pub project_dir: PathBuf
```

目标定义：

```rust
pub project_dir: Option<PathBuf>
```

保留字段名 `project_dir`，理由：
- 最小化 JSON 持久化结构变更
- 降低数据库历史数据兼容成本
- 代码层依然能明确表达其语义是“session 当前项目目录”

### 2. 移除 `default_project_dir()` 与强制回退
需要删除：
- `control.rs` 中 `default_project_dir()`
- `ControlState::new()` 对当前目录的隐式填充
- `SessionService` 中的 `default_project_dir` 字段
- `SessionService::new_with_default_project_dir(...)`
- `bootstrap` 中对 `tool.default_project_dir` 的解析和应用

替代策略：
- `ControlState::new(default_agent)` 直接初始化 `project_dir = None`
- 历史数据如果没有 `project_dir` 字段，按 `None` 处理

### 3. 删除 reset 能力
需要删除：
- `ProjectDirService::reset_project_dir`
- `SessionService::reset_project_dir`
- `ConversationService::reset_project_dir`
- `AgentApplication::reset_project_dir`
- 所有调用点、测试和文档中的 `reset_project_dir`

理由：
- 当前业务没有“默认 project”
- 允许 `project = None` 后，reset 没有明确业务目标

### 4. 删除默认 project 配置入口
需要删除或废弃：
- `.nova/config.toml` 中的 `tool.default_project_dir`
- `ToolConfig.default_project_dir`
- `AppConfig::default_project_dir()`
- `bootstrap.rs::resolve_default_project_dir`

配置层不再承载 project 语义，避免出现静态配置和 session 运行态的双重来源。

### 5. 兼容性与迁移
需要兼容两类历史数据：
- 旧 session JSON 中 `project_dir` 为字符串路径
- 更旧 session JSON 中字段缺失

建议行为：
- 若字段存在且为字符串，反序列化为 `Some(PathBuf)`
- 若字段缺失，则为 `None`

这样历史 session 仍可读取，不会因为字段从必填改可空而损坏已有数据。

## 测试案例

- 测试 1：旧格式 `project_dir = "."` 的 control state 可成功反序列化为 `Some(PathBuf)`
- 测试 2：缺失 `project_dir` 的 legacy control state 可成功反序列化为 `None`
- 测试 3：`ControlState::new("agent-1")` 默认生成 `project_dir = None`
- 测试 4：删除 `default_project_dir` 配置后，配置解析与启动流程不再依赖该字段
- 测试 5：所有 `reset_project_dir` 相关公开接口已移除，编译链路无残留引用
