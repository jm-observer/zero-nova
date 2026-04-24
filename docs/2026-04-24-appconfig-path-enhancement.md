# 2026-04-24 AppConfig 路径管理增强设计

| 章节 | 说明 |
|-----------|------|
| 时间 | 创建：2026-04-24；最后更新：2026-04-24 |
| 项目现状 | 当前路径处理逻辑分散在多个位置：`AppConfig` 仅封装了 `skills_dir`，而 `nova-app` 的启动流程仍直接拼接 `workspace/prompts`、`workspace/.nova/data` 和 `workspace/config.toml`。这使得路径规则难以复用，也增加了配置项扩展后的行为分叉风险。 |
| 整体目标 | 增强 `AppConfig` 的职责，使其成为应用运行时路径解析的唯一入口。所有依赖 workspace 推导的业务路径都通过 `AppConfig` 暴露，统一支持相对路径（相对于 workspace）和绝对路径，并消除 `bootstrap` 内部的路径硬编码。 |
| Plan 拆分 | 1. `Plan 1: AppConfig 结构扩展与路径计算逻辑实现`：在 `nova-core` 中明确配置项落点，抽出统一路径解析逻辑，补充 `data_dir`、`prompts_dir` 等访问方法及单元测试。<br>2. `Plan 2: 业务模块适配与路径硬编码清理`：调整 `nova-app` 引导逻辑，仅通过 `AppConfig` 获取路径，移除独立 `workspace` 推导，并补充集成验证。 |
| 风险与待定项 | 1. `data_dir` 的配置层级需要固定为 `AppConfig` 根级字段，避免与 `tool.skills_dir` 的职责混杂。<br>2. 本次设计只统一“目录路径”的 source of truth，`sessions.db` 文件名仍保留在 `SqliteManager` 内部，暂不在 `AppConfig` 中暴露可配置项。<br>3. `build_application` 目前同时接收 `AppConfig` 和独立 `workspace` 参数，实施时需要收敛为单一来源，否则文档目标无法成立。 |
