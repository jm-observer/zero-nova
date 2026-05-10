# nova-agent-bigfile-split

## 时间
- 创建日期：2026-05-10
- 最后更新：2026-05-10

## 项目现状
`crates/nova-agent` 存在多处超大文件，已超过项目约定的单文件 500 行建议上限，当前前 12 个大文件如下：
- `crates/nova-agent/src/prompt.rs`：2381 行
- `crates/nova-agent/src/conversation/service.rs`：1912 行
- `crates/nova-agent/src/config.rs`：1755 行
- `crates/nova-agent/src/agent.rs`：1102 行
- `crates/nova-agent/src/skill.rs`：1010 行
- `crates/nova-agent/src/conversation/repository.rs`：973 行
- `crates/nova-agent/src/app/agent_workspace_service.rs`：905 行
- `crates/nova-agent/src/tool.rs`：837 行
- `crates/nova-agent/src/tool/builtin/agent.rs`：732 行
- `crates/nova-agent/src/app/application.rs`：715 行
- `crates/nova-agent/src/conversation/sqlite_manager.rs`：683 行
- `crates/nova-agent/src/app/conversation_service.rs`：667 行

现有问题：
- 类型定义、核心流程、IO 边界、测试辅助等职责混杂在同一文件，评审与定位成本高。
- 文件间依赖隐式耦合，局部改动容易触发跨模块回归。
- 超长函数与深层嵌套集中出现，不利于后续演进和缺陷隔离。

## 整体目标
在不改变外部行为、公共接口语义与协议格式的前提下，完成 `nova-agent` 超大文件拆分设计：
- 将核心超大文件按职责拆为子模块，建立稳定目录边界。
- 明确每个 Plan 的输入输出、依赖顺序、测试覆盖与回滚策略。
- 约束拆分节奏为“小批次、可回归、可回滚”，避免将重构风险叠加到功能改动。

## Plan 拆分
| Plan | 描述 | 依赖 | 执行顺序 | 状态 |
|---|---|---|---|---|
| Plan 1 | 现状盘点与优先级分层（高优先范围冻结） | 无 | 1 | 待开始 |
| Plan 2 | `prompt.rs` 与 `config.rs` 拆分设计（规则与配置核心） | Plan 1 | 2 | 待开始 |
| Plan 3 | `conversation/*` 与 `app/*` 拆分设计（服务与存储边界） | Plan 1 | 3 | 待开始 |
| Plan 4 | `agent.rs`、`skill.rs`、`tool.rs` 拆分设计（运行时与工具域） | Plan 1 | 4 | 待开始 |
| Plan 5 | 渐进实施策略、验证矩阵与回滚预案 | Plan 2, Plan 3, Plan 4 | 5 | 待开始 |

执行策略：
- 先冻结高优先文件清单与拆分粒度，再按“配置/提示词 -> 会话与应用层 -> 运行时与工具层”推进。
- 每完成一个 Plan，执行一次修复流程（`cargo clippy --workspace -- -D warnings`、`cargo fmt --all --check`、`cargo test --workspace`），通过后更新状态。

## 风险与待定项
- 风险 1：拆分时可见性收紧可能引发循环依赖，需要提前设计 `mod` 边界与 re-export 规则。
- 风险 2：拆分引发路径变更，测试/快照/工具读取逻辑可能受影响，需要补充路径相关回归。
- 风险 3：一次性改动范围过大可能导致冲突与排障困难，必须坚持小批次迁移。
- 待定项：`prompt.rs` 是否先按“构建流程”拆分，还是先按“数据结构与模板资源”拆分，需要在 Plan 2 落定。
