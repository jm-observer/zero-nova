# Plan 3: 测试与验收闭环

## 前置依赖
- Plan 1 必需
- Plan 2 若实施则纳入

## 本次目标
- 为“消除同步阻塞读取”和“高频读取稳定性”建立自动化验证。
- 让后续 review 能快速识别同类回归。

## 涉及文件
- `crates/nova-agent/src/app/application.rs`
- `crates/nova-agent/tests/*`（若已有合适的集成测试入口）
- `docs/2026-05-10-nova-agent-config-snapshot-async/nova-agent-config-snapshot-async.md`

## 详细设计
### 1. 单元测试
- 为 `config_snapshot` 补充异步测试，验证基础快照内容正确。
- 为 `update_config` + `config_snapshot` 补充并发测试，验证在有限超时时间内能完成且结果一致。
- 超时、重试等参数提取为具名常量，避免 magic number。

### 2. 失败场景测试
- 非法 `payload`：断言返回 `Failed to parse config update payload` 上下文。
- 写配置文件失败：断言 `self.config` 与快照状态不被部分更新。
- 若实施 Plan 2：新增“更新成功后立即读取快照”为新值的测试。

### 3. 静态防回归
- 检查 `crates/nova-agent/src/app/application.rs` 中不再包含 `blocking_read`。
- 检查 `config_snapshot` 不被重新改回同步方法。
- review 清单增加一项：异步上下文禁止使用同步阻塞锁或桥接异步读取。

### 4. 修复流程
- 在仓库根执行：
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --all --check`
  - `cargo test --workspace`
- 三项全部通过后，再更新总览文档中的 Plan 状态。

## 验收标准
- correctness：不再阻塞 runtime worker。
- consistency：更新成功后，配置内存态、磁盘态、快照态保持一致。
- maintainability：新增测试能覆盖正常路径、失败路径、并发路径。
