# Plan 2: Schema 导出与工件管理流水线

## 前置依赖
- Plan 1

## 本次目标
- 建立可重复执行的 Schema 导出流程。
- 明确 Schema 工件在仓库中的位置、生成命令和校验规则。
- 保证开发者在本地与 CI 生成结果一致。

## 涉及文件
- `crates/nova-protocol/Cargo.toml`
- `crates/nova-protocol/src/lib.rs`
- `scripts/`（新增 schema 导出脚本）
- `schemas/`（新增 schema 工件目录）
- `Makefile.toml`（或等价任务入口）

## 详细设计
### 1. 生成入口
- 在 `crates/nova-protocol` 提供 schema 导出入口（建议独立 bin 或脚本驱动）。
- 统一命令：`cargo run -p nova-protocol --bin export-schema`（示例）。
- 输出目录固定为仓库根 `schemas/`。

### 2. 工件治理
- Schema 工件纳入版本控制，不依赖运行时动态生成。
- 每次协议类型变更，必须同步更新 `schemas/` 并提交。
- 对生成结果执行稳定化处理（排序、格式化），减少无意义 diff。

### 3. 失败策略
- 若存在协议代码变更但 schema 未更新，CI 直接失败。
- 若 schema 导出失败，阻断合并，不允许跳过。

### 4. 渐进落地
- 阶段 1：先导出 `GatewayMessage` 与核心 payload。
- 阶段 2：扩展到 observability 全量类型。

## 测试案例
- 正常路径：执行导出命令后 `schemas/` 生成完整文件且可重复执行无 diff。
- 边界条件：仅修改注释或非协议代码，不应导致 schema 变更。
- 异常场景：新增字段但未更新 schema，CI 的 schema-diff 检查应失败。
