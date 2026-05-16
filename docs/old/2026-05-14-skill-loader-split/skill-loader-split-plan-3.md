# Plan 3: 验证与收口

## 前置依赖

Plan 2

## 本次目标

执行项目修复流程，确保新增 crate 纳入 workspace 后通过 clippy、fmt 和测试。

## 涉及文件

- `Cargo.lock`
- 可能由格式化或测试生成的 schema 文件

## 详细设计

按 workspace 维度运行 `cargo clippy --workspace -- -D warnings`、`cargo fmt --all --check` 和 `cargo test --workspace`。如发现格式或 clippy 问题，先修复本次拆分引入的问题，再重新执行完整流程。

## 测试案例

- 正常路径：workspace 全量测试通过。
- 边界条件：新增 crate 被 workspace 构建覆盖。
- 异常场景：若存在与本次无关的历史失败，记录失败位置并避免扩大修改范围。
