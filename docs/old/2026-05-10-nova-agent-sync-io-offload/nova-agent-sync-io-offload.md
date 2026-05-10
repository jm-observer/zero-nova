# nova-agent-sync-io-offload

## 时间
- 创建日期：2026-05-10
- 最后更新：2026-05-10

## 项目现状
- `crates/nova-agent/src/skill.rs` 的 `SkillRegistry::load_from_dir` 及其递归扫描、解析流程使用 `std::fs::read_dir/read_to_string`，调用发生在 `build_application` 异步启动路径中，存在阻塞 runtime 风险。
- `crates/nova-agent/src/prompt.rs` 同时存在异步与同步两套加载函数：`load_developer_project_prompt_async` 已异步；`load_single_project_context`、`load_developer_project_prompt`、`WorkflowStagePrompts::load_from_file` 仍为同步读取，若被异步主路径调用会引入阻塞点。
- `crates/nova-agent/src/app/bootstrap.rs` 的 `warn_unused_gateway_sections` 通过 `std::fs::read_to_string` 扫描配置文件，当前由 `build_application` 调用，属于启动关键路径同步 I/O。

## 整体目标
- 将 `src/skill.rs`、`src/prompt.rs`、`src/app/bootstrap.rs` 在异步调用链上的同步文件 I/O 全部下沉为非阻塞实现。
- 统一策略：优先 `tokio::fs`；对必须保留同步 API 的场景，用 `tokio::task::spawn_blocking` 封装并提供异步入口，避免直接在 runtime worker 线程执行阻塞 I/O。
- 保持现有行为一致（加载顺序、容错语义、日志语义不变），仅做 I/O 执行模型优化。

## Plan 拆分
- Plan 1（待开始）：`skill.rs` 技能扫描链路异步化。
  说明：把技能目录递归扫描与文件读取从 `std::fs` 迁移到 `tokio::fs`（或受控 `spawn_blocking`），并在 `bootstrap.rs` 调整调用。
  依赖：无。
  执行顺序：1。
- Plan 2（待开始）：`prompt.rs` 同步加载入口下沉。
  说明：为项目上下文、开发者提示词、workflow stage 文件加载提供异步优先路径；保留同步 API 仅作兼容，并明确调用边界。
  依赖：Plan 1（建议，非强依赖）。
  执行顺序：2。
- Plan 3（待开始）：`bootstrap.rs` 启动路径阻塞 I/O 清理与回归校验。
  说明：下沉 `warn_unused_gateway_sections` 的同步读取，串联 Plan 1/2 的异步接口，并完成端到端验证。
  依赖：Plan 1、Plan 2。
  执行顺序：3。

## 风险与待定项
- 风险：`skill.rs` 若改为完全异步递归，函数签名会从同步 `Result<()>` 升级为 `async`，影响调用方与测试用例；需控制改动面。
- 风险：`prompt.rs` 现有同步 API 可能在非 Tokio 上下文中被测试或工具复用；若直接移除会破坏兼容，建议保留同步封装并新增 async 主入口。
- 风险：`spawn_blocking` 滥用会导致线程池压力，需限定仅用于无法快速异步化且调用频率低的路径。
- 待定：`WorkflowStagePrompts::load_from_file` 是否升级为 `load_from_file_async` 并替换全部调用；若当前仅在同步初始化阶段调用，可先保留同步 API + 新增异步版本。
