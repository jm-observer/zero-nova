# Plan 2: 业务模块适配与路径硬编码清理

| 章节 | 说明 |
|-----------|------|
| Plan 编号与标题 | Plan 2: 业务模块适配与路径硬编码清理 |
| 前置依赖 | Plan 1 |
| 本次目标 | 修改 `nova-app` 中的引导逻辑，移除所有业务路径的本地拼接逻辑，只通过 `AppConfig` 获取运行时目录，并补充覆盖关键路径分支的集成验证。 |
| 涉及文件 | `crates/nova-app/src/bootstrap.rs`、`crates/nova-app` 下对应测试文件（如现有测试模块或新增集成测试） |

## 详细设计

### 1. 修改 `build_application`
- 删除 `build_application` 中仅用于路径推导的独立 `workspace` 参数，统一以 `config.workspace` 作为唯一基准路径来源。
- `config_path` 也改为从 `config.workspace` 推导，避免运行时出现 `config.workspace` 与函数参数 `workspace` 不一致的分叉。
- 在初始化 `SqliteManager` 前，调用 `config.data_dir()` 获取目录。
- 移除 `bootstrap.rs` 中硬编码的 `.join(".nova").join("data")`。
- 修改获取 Prompt 模板的路径，改用 `config.prompts_dir()`。

### 2. 验证路径传递
确保 `SqliteManager` 接收到的是经 `AppConfig::data_dir()` 规范化后的目录路径。`SqliteManager` 继续负责在该目录下创建并使用 `sessions.db`，本 Plan 不修改其构造接口。

### 3. Prompt 与 Skill 路径行为保持一致
Agent 默认 Prompt 文件的查找逻辑改为基于 `config.prompts_dir()`。这意味着：
- 默认情况下仍读取 `{workspace}/prompts/agent-<id>.md`。
- 若后续配置显式指定相对或绝对 `prompts_dir`，启动流程无需感知细节，只消费 `AppConfig` 返回的结果。

Skill 目录在 `bootstrap` 中已经通过 `config.skills_dir()` 获取，本 Plan 不改变其业务行为，只确保 Prompt、Data、Config 三类路径遵循同一模式。

## 测试案例
- 增加启动相关测试，验证默认配置下 `sessions.db` 仍生成在 `{workspace}/.nova/data`。
- 增加启动相关测试，验证配置相对 `data_dir` 后，数据库目录解析相对于 `config.workspace`。
- 增加启动相关测试，验证默认 Prompt 仍从 `{workspace}/prompts` 读取。
- 增加启动相关测试，验证配置相对或绝对 `prompts_dir` 后可正确读取默认 Prompt。
- 回归验证 Skills 加载路径未受影响，仍基于 `config.skills_dir()` 工作。
