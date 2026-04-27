# 2026-04-27 Crate Consolidation Overview

**时间**: 2026-04-27

## 项目现状
当前代码库包含多个独立的 crate（如 `nova-core`, `nova-conversation`, `nova-app` 等），它们之间存在重复依赖和交叉引用。部分共享模块散落在不同的 crate 中，导致维护成本高、编译时间长，并且 Cargo workspace 配置复杂。

## 整体目标
- **统一依赖**：在根目录的 `Cargo.toml` 中集中管理所有依赖版本，消除重复声明。
- **合并共享模块**：将通用功能（如日志、错误处理、模型定义）迁移至 `nova-core`，其他 crate 通过 `pub(crate)` 或 `pub` 引用。
- **简化 workspace**：确保每个 crate 的职责单一，去除不必要的子 crate，保持 workspace 整洁。
- **保证 CI**：在完成所有改动后，运行完整检查（`cargo clippy`, `cargo fmt`, `cargo test`）并通过。

## Plan 拆分
| Plan | 描述 | 依赖 |
|------|------|------|
| Plan 1 | Crate dependency unification – 将所有共享依赖统一到根 Cargo.toml，更新各子 crate 的 `[dependencies]` 引用。 | 无 |
| Plan 2 | Consolidate shared modules into `nova-core` – 把重复的代码抽取到核心库，并在其他 crate 中使用 `pub(crate)`/`pub` 导出。 | Plan 1 |
| Plan 3 | Update Cargo workspace configuration – 调整 `Cargo.toml` 的 `[workspace]` 部分，删除已合并的 crate，确保路径正确。 | Plan 2 |

## 风险与待定项
- **兼容性**：某些 crate 可能依赖特定版本的库，统一后需要确认没有破坏向后兼容。
- **编译时间**：大幅度重构后首次全量编译可能耗时较长，需要预留 CI 资源。
- **测试覆盖**：确保所有功能在新结构下仍然通过现有测试。

---

*此文档遵循项目的设计文档规范，后续每个 Plan 将在 `docs/` 中以单独文件形式详细描述实现细节。*
