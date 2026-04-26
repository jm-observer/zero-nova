# Crate Consolidation Design

| 章节 | 说明 |
|-----------|------|
| 时间 | 2026-04-26 |
| 项目现状 | 目前项目中有 11 个 crate，其中 `nova-core`、`nova-conversation` 和 `nova-app` 职责划分过细。`nova-core` 负责 Agent 核心逻辑，`nova-conversation` 负责会话存储和管理，`nova-app` 则是对外的应用门面。这些模块通常需要一起使用。 |
| 整体目标 | 将 `nova-core`、`nova-conversation` 和 `nova-app` 合并为一个统一的 `nova-agent` crate。简化依赖关系，降低外部调用方的集成成本。 |
| Plan 拆分 | 1. **Plan 1: 创建 nova-agent crate 并迁移核心逻辑** - 初始化新 crate，合并 `nova-core` 内容。<br>2. **Plan 2: 迁移会话管理逻辑** - 将 `nova-conversation` 内容迁入 `nova-agent`。<br>3. **Plan 3: 合并应用门面逻辑** - 将 `nova-app` 内容迁入 `nova-agent`。<br>4. **Plan 4: 全局依赖更新与清理** - 更新所有其他 crate 的依赖，删除旧 crate。 |
| 风险与待定项 | 1. 循环依赖风险：需要确保迁移过程中不会引入循环依赖。<br>2. 命名冲突：合并后可能需要调整某些模块的导出路径。 |
