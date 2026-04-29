# Plan 4: 测试、文档迁移与发布策略

## 前置依赖
- Plan 1
- Plan 2
- Plan 3

## 本次目标
- 补齐单元/集成测试，覆盖兼容迁移、运行态切换、`@` 解析。
- 更新用户文档与配置示例，明确 `config_dir` 与 `project_dir` 的职责边界。
- 通过修复流程并形成可回滚发布策略。

## 涉及文件
- `crates/nova-agent/tests/bootstrap_paths.rs`
- `crates/nova-agent/tests/integration/*`（按需新增）
- `docs/` 下用户文档（按现有文档结构落位）
- `AGENTS.md`（若需补充使用约定）

## 详细设计
- 测试补充：
  - 配置层：`workspace -> config_dir` 更名后 helper 行为保持一致。
  - 会话层：project_dir 动态设置/覆盖/重置（重置回 cwd）。
  - 解析层：`@` 规则完整覆盖。
  - 提示词层：`EnvironmentSnapshot` 输出包含 `Config directory:` 和 `Project directory:`。
- 回归策略：
  - 对现有测试做最小改动，确保历史行为不受影响。
  - 新增测试文件尽量按职责分组（config / session / resolver / prompt）。
- 文档更新：
  - 增加"目录模型"章节：
    - `config_dir`: 配置、skills、data、prompts（在提示词中显示为 `Config directory:`）
    - `project_dir`: 用户工作目录，会话可切换，默认 cwd（在提示词中显示为 `Project directory:`）
  - 增加"运行中切换 project_dir"示例和 `@` 使用示例。
- 发布与回滚：
  - 按 Plan 粒度提交，保持每个提交可独立回滚。

## 测试案例
- 正常路径：完整修复流程三项全部通过。
- 边界路径：Windows/Unix 风格路径混用下解析正确。
- 回归路径：旧配置文件不新增字段时可正常启动，project_dir 自动取 cwd。
- 提示词路径：Environment 部分同时包含 `Config directory:` 和 `Project directory:`，Git 信息来自 project_dir。
