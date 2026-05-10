# Plan 1: 现状盘点与优先级分层

## 前置依赖
无

## 本次目标
- 冻结 `nova-agent` 超大文件拆分的高优先范围，形成可执行的目标清单。
- 对每个目标文件标注职责密度、外部依赖强度与改动风险，确定实施批次。
- 建立拆分验收口径，确保后续 Plan 使用统一标准。

## 涉及文件
- `docs/2026-05-10-nova-agent-bigfile-split/nova-agent-bigfile-split.md`
- `docs/2026-05-10-nova-agent-bigfile-split/nova-agent-bigfile-split-plan-1.md`
- `crates/nova-agent/src/prompt.rs`
- `crates/nova-agent/src/config.rs`
- `crates/nova-agent/src/conversation/service.rs`
- `crates/nova-agent/src/agent.rs`
- `crates/nova-agent/src/skill.rs`
- `crates/nova-agent/src/tool.rs`
- `crates/nova-agent/src/app/agent_workspace_service.rs`

## 详细设计
1. 拆分优先级规则
- P0（高优先）：文件行数 > 1000 且承担跨模块入口职责。
- P1（中高优先）：文件行数 700-1000 且包含多类职责混合。
- P2（次优先）：500-700 行，先观察是否可通过局部抽取降到阈值以下。

2. 拆分评分维度
- 结构复杂度：类型数量、函数数量、平均函数长度、嵌套层级。
- 依赖复杂度：`use` 数量、跨目录依赖、双向依赖风险。
- 变更风险：对外导出符号数量、测试覆盖质量、是否包含状态机/并发共享状态。

3. 高优先拆分清单（冻结）
- P0：`prompt.rs`、`config.rs`、`conversation/service.rs`、`agent.rs`、`skill.rs`
- P1：`tool.rs`、`app/agent_workspace_service.rs`、`conversation/repository.rs`

4. 验收口径
- 拆分后单文件目标：核心文件 <= 500 行；允许个别文件短期 <= 650 行，但必须在文档中记录原因。
- 公共导出：统一在原入口 `mod.rs` 或父模块 `lib.rs` re-export，保持外部调用路径稳定。
- 行为一致性：原有集成测试必须通过；新增最小回归测试覆盖拆分边界。

## 测试案例
- 正常路径：执行全量测试，确认拆分前后行为无变化。
- 边界条件：针对 re-export 路径，验证旧路径导入不报错。
- 异常场景：故意移除子模块导出，验证编译期可快速暴露边界错误。
