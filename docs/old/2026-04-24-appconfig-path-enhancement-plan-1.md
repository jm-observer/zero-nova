# Plan 1: AppConfig 结构扩展与路径计算逻辑实现

| 章节 | 说明 |
|-----------|------|
| Plan 编号与标题 | Plan 1: AppConfig 结构扩展与路径计算逻辑实现 |
| 前置依赖 | 无 |
| 本次目标 | 在 `nova-core` 中扩展 `AppConfig`，固定路径配置结构，提供统一的路径解析入口，并用单元测试覆盖默认值、相对路径和绝对路径行为。 |
| 涉及文件 | `crates/nova-core/src/config.rs` |

## 详细设计

### 1. 配置项扩展
将 `data_dir` 增加到 `OriginAppConfig` / `AppConfig` 根级别，不放入 `ToolConfig`。原因是该目录承载的是应用运行期数据，而非某个工具子系统的局部配置；放在根级更符合职责边界，也便于后续为其他模块复用。

```rust
pub struct OriginAppConfig {
    // ... 现有字段
    pub data_dir: Option<String>,
}

pub struct AppConfig {
    // ... 现有字段
    pub data_dir: Option<String>,
}
```

### 2. 统一路径计算逻辑
在 `AppConfig` 内部实现私有辅助方法 `resolve_path(path: Option<&str>, default: impl FnOnce() -> PathBuf) -> PathBuf`，统一处理以下规则：
- 配置值不存在时，返回默认路径。
- 配置值为绝对路径时，直接返回。
- 配置值为相对路径时，基于 `self.workspace` 进行拼接。

该方法只负责“目录解析”，不承担目录创建、副作用 IO 或文件名拼接，以保持职责单一。

### 3. 增加路径访问方法
在 `AppConfig` 中增加以下方法：
- `data_dir()`: 返回数据目录（默认为 `{workspace}/.nova/data`）。
- `skills_dir()`: 已有，但需重构以使用 `resolve_path`。
- `prompts_dir()`: 返回 Prompt 模板目录（默认为 `{workspace}/prompts`）。

这里不新增 `sqlite_db_path()`。本次设计将 “数据库文件名为 `sessions.db`” 继续视为 `SqliteManager` 的内部约定，仅把数据库所在目录统一收口到 `AppConfig::data_dir()`，避免同时在两个模块维护路径拼接规则。

### 4. `workspace` 作为路径推导基准
`AppConfig::workspace` 继续作为所有相对路径解析的基准目录。Plan 1 不调整调用方签名，但会在文档和方法设计上明确：后续业务模块不得再绕过 `AppConfig` 自行拼接 workspace 相关路径。

## 测试案例
- 测试 `data_dir` 为空时，默认指向 `.nova/data`。
- 测试 `data_dir` 为相对路径时，相对于 `workspace`。
- 测试 `data_dir` 为绝对路径时，直接返回。
- 测试 `skills_dir` 默认值仍为 `{workspace}/.nova/skills`。
- 测试 `skills_dir` 为相对路径时，相对于 `workspace`。
- 测试 `prompts_dir` 默认值为 `{workspace}/prompts`。
- 测试 `prompts_dir` 为绝对路径时，直接返回。
