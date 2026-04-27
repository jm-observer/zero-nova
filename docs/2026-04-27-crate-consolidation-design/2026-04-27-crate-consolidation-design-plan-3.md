# Plan 3: workspace 成员与依赖清理

## Plan 编号与标题
- Plan 3: workspace 成员与依赖收敛

## 前置依赖
- Plan 2

## 本次目标
- 从根 `Cargo.toml` 移除被合并 crate 的 `members` 与 internal dependency 声明。
- 修复所有 `use`、crate 名引用、feature 标记和 bin 调用路径。
- 确保 workspace 编译图简化且无悬空依赖。

## 涉及文件
- 修改: `Cargo.toml`（root）
- 修改: 各 crate `Cargo.toml`（引用更新）
- 修改: 受影响的 Rust 源码中的 `use channel_*` / `use nova_server_*`
- 可选新增: `docs/.../migration-notes.md`（迁移映射说明）

## 详细设计
- 成员清理:
  - 移除 `channel-core`、`channel-stdio`、`channel-websocket`、`nova-server-stdio`、`nova-server-ws`（以最终合并结果为准）。
- 依赖清理:
  - 删除 root `workspace.dependencies` 中不再需要的 internal crate path。
  - 保留外部依赖并迁移到仍然使用它们的 crate。
- 引用修复:
  - 将 `channel_core::...` 替换为 `nova_server::transport::core::...`（或最终导出的兼容路径）。
  - 保证 `nova-gateway-core` 只改路径，不改语义。
- 兼容性处理:
  - 若存在外部脚本直接 `cargo run -p nova-server-ws`，给出替代命令并在文档中标注。

## 测试案例
- 正常路径:
  - `cargo check --workspace` 无 unresolved import、无 missing package。
- 边界条件:
  - 仅启用 ws/stdio 子集功能时 feature 仍可编译。
- 异常场景:
  - 旧 crate 名被调用时给出可定位错误（由文档和 CI 提示补偿）。
