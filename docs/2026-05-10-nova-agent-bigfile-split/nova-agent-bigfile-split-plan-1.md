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

### 1. 现状盘点数据统计

| 文件 | 行数 | pub struct | pub enum | pub trait | pub fn | async fn | #[derive] | use crate:: | use std:: | RwLock | Mutex | Arc | Cell/RefCell | #[test] |
|------|------|-----------|---------|----------|--------|----------|-----------|------------|----------|--------|--------|-----|-------------|---------|
| `prompt.rs` | 2381 | 17 | 7 | 12 | 68 | 12 | 18 | 5 | 4 | 2 | 2 | 47 | — | 47 |
| `config.rs` | 1755 | 18 | 12 | 13 | 13 | 5 | 25 | 2 | 4 | — | — | 10 | — | 33 |
| `conversation/service.rs` | 1844 | 1 | 3 | 3 | 3 | 6 | 1 | 33 | 6 | 2 | 1 | 1 | 4 | 10 |
| `conversation/repository.rs` | 1011 | 3 | 1 | 1 | 1 | 3 | 3 | 4 | 0 | — | — | 1 | 2 | 0 |
| `agent.rs` | 1105 | 5 | 1 | 6 | 6 | 4 | 4 | 17 | 6 | 5 | 4 | 14 | 15 | 4 |
| `skill.rs` | 1010 | 4 | 4 | 3 | 19 | 4 | 8 | 0 | 1 | — | — | 10 | — | 15 |
| `tool.rs` | 837 | 9 | 1 | 2 | 17 | 14 | 6 | 5 | 3 | 3 | 12 | 42 | 4 | 0 |
| `app/agent_workspace_service.rs` | 905 | 1 | 1 | 1 | 6 | 5 | 0 | 11 | 7 | 5 | 一 | 24 | 2 | 2 |

> 数据收集时间：2026-05-10
> 注：`—` 表示计数为 0

**特殊导入标注**

| 文件 | sqlx | tokio | chrono |
|------|------|-------|--------|
| `prompt.rs` | N | Y | Y |
| `config.rs` | N | N | N |
| `service.rs` | N | Y | Y |
| `repository.rs` | Y (40) | Y | Y |
| `agent.rs` | N | Y | Y |
| `skill.rs` | N | N | N |
| `tool.rs` | N | Y | N |
| `agent_workspace_service.rs` | N | Y | Y |

### 2. 拆分优先级规则
- **P0（高优先）**：文件行数 > 1000 且承担跨模块入口职责。
  - `prompt.rs`（2381 行）：提示词构建核心，包含 17 个 pub struct、7 个 pub enum、68 个 pub fn
  - `config.rs`（1755 行）：配置管理核心，包含 18 个 pub struct、13 个 pub fn
  - `conversation/service.rs`（1844 行）：会话服务层，RwLock 使用最密集（2 次）
  - `agent.rs`（1105 行）：Agent 核心运行时，Cell/RefCell 使用频繁（15 次）
  - `skill.rs`（1010 行）：技能管理核心，4 个 pub enum
- **P1（中高优先）**：文件行数 700-1000 且包含多类职责混合。
  - `tool.rs`（837 行）：2 个 pub trait，Mutex 使用最多（12 次）
  - `app/agent_workspace_service.rs`（905 行）：应用层枢纽，Arc 使用密集（24 次）
  - `conversation/repository.rs`（1011 行）：唯一使用 sqlx 的文件（40 处引用）
- **P2（次优先）**：500-700 行，先观察是否可通过局部抽取降到阈值以下。

### 3. 拆分评分维度

**结构复杂度评分**
| 文件 | 类型密度 | 函数密度 | 平均函数长度(估) | 嵌套层级(估) | 综合评分 |
|------|---------|---------|-----------------|-------------|---------|
| prompt.rs | 高（17 struct + 7 enum） | 极高（68 个 pub fn） | 45-60 行 | 3-4 层 | ★★★★★ |
| config.rs | 高（18 struct） | 中（13 个 pub fn） | 80-100 行 | 2-3 层 | ★★★★☆ |
| service.rs | 中（1 struct） | 低（3 个 pub fn） | 100+ 行 | 4-5 层 | ★★★★☆ |
| agent.rs | 中（5 struct） | 中（6 个 pub fn） | 60-80 行 | 3-4 层 | ★★★★☆ |
| tool.rs | 高（9 struct + 2 trait） | 高（17 个 pub fn） | 40-50 行 | 2-3 层 | ★★★☆☆ |

**依赖复杂度评分**
| 文件 | crate 依赖 | 跨目录依赖 | 双向依赖风险 | 综合评分 |
|------|-----------|-----------|-------------|---------|
| prompt.rs | 5 个 crate:: | 高 | 中 | ★★★★☆ |
| agent.rs | 17 个 crate:: | 极高 | 高 | ★★★★★ |
| service.rs | 33 个 crate:: | 高 | 中 | ★★★★☆ |
| agent_workspace_service.rs | 11 个 crate:: | 高 | 中 | ★★★★☆ |

**变更风险评分**
| 文件 | 导出符号数 | 测试覆盖 | 状态机/并发 | 综合评分 |
|------|-----------|---------|-----------|---------|
| prompt.rs | 68 pub fn + 17 struct | 高（47 测试） | 低 | ★★★☆☆ |
| service.rs | 低 | 中（10 测试） | 高（并发共享） | ★★★★☆ |
| agent.rs | 中 | 中（4 测试） | 高（Cell/RefCell） | ★★★★☆ |
| config.rs | 13 pub fn + 18 struct | 高（33 测试） | 低 | ★★☆☆☆ |

### 4. 高优先拆分清单（冻结）
- **P0**：`prompt.rs`、`config.rs`、`conversation/service.rs`、`agent.rs`、`skill.rs`
- **P1**：`tool.rs`、`app/agent_workspace_service.rs`、`conversation/repository.rs`

### 5. 验收口径
- 拆分后单文件目标：核心文件 <= 500 行；允许个别文件短期 <= 650 行，但必须在文档中记录原因。
- 公共导出：统一在原入口 `mod.rs` 或父模块 `lib.rs` re-export，保持外部调用路径稳定。
- 行为一致性：原有集成测试必须通过；新增最小回归测试覆盖拆分边界。

## 测试案例
- 正常路径：执行全量测试，确认拆分前后行为无变化。
- 边界条件：针对 re-export 路径，验证旧路径导入不报错。
- 异常场景：故意移除子模块导出，验证编译期可快速暴露边界错误。
