# Plan 1: 配置项增强与默认路径解析

## 前置依赖
无

## 本次目标
在 `.nova/config.toml` 的 `[tool]` 模块中引入 `default_project_dir`，并为该配置定义稳定的解析与回退策略，使后续启动流程和新会话创建都可以复用同一套规则。

## 涉及文件
- `D:\git\zero-nova\crates\nova-agent\src\config.rs`
- `D:\git\zero-nova\crates\nova-agent\tests\` 下与配置解析相关的测试文件（可新增或扩展现有 `config.rs` 单测）

## 详细设计

### 1. 配置结构变更
在 `ToolConfig` 中新增字段：

```rust
pub default_project_dir: Option<String>,
```

此处优先保持与 `skills_dir`、`prompts_dir`、`project_context_file` 一致的字符串配置形式，避免在反序列化阶段过早固化成绝对路径。

### 2. AppConfig 路径解析辅助方法
在 `AppConfig` 上新增统一辅助方法，例如：

```rust
pub fn default_project_dir(&self) -> Option<PathBuf>
```

解析规则：
- 配置为空时返回 `None`
- 绝对路径直接返回
- 相对路径相对 `config_dir` 解析

这样后续 `bootstrap`、会话创建和测试都不需要复制路径拼接逻辑。

### 3. 启动期合法性策略
本 Plan 只定义规则，不在 `OriginAppConfig::validate` 中强制要求目录存在。原因：
- 配置文件可能被共享到不存在该目录的机器
- 当前 `validate` 主要负责静态结构和枚举值校验，不适合引入环境相关 I/O

真正的目录存在性、是否为目录、是否可访问，放到使用点异步校验。

### 4. 配置示例

```toml
[tool]
default_project_dir = "projects/alpha"
```

若 `config_dir = D:/workspace/.nova`，则最终解析为：

```text
D:/workspace/.nova/projects/alpha
```

若用户希望指定任意位置，可继续使用绝对路径：

```toml
[tool]
default_project_dir = "D:/my_work_project"
```

### 5. 对现有逻辑的影响范围
- 不影响未配置该项的用户，仍回退到 `current_dir`
- 不修改 session 持久化结构
- 不引入新的 schema 文件或协议字段

## 测试案例

- 测试 1：读取包含绝对默认路径的配置
  - 输入：`default_project_dir = "D:/repo/app"`
  - 预期：`AppConfig::default_project_dir()` 返回对应绝对路径

- 测试 2：读取包含相对默认路径的配置
  - 输入：`default_project_dir = "projects/app"` 且 `config_dir = D:/workspace/.nova`
  - 预期：解析结果为 `D:/workspace/.nova/projects/app`

- 测试 3：未设置默认路径
  - 输入：缺少 `default_project_dir`
  - 预期：`AppConfig::default_project_dir()` 返回 `None`

- 测试 4：默认路径与其他 tool 配置共存
  - 输入：同时配置 `skills_dir`、`prompts_dir`、`project_context_file`、`default_project_dir`
  - 预期：各字段解析互不干扰
