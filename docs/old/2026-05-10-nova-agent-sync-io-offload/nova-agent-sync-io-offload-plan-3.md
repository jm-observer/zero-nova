# Plan 3: bootstrap.rs 启动路径阻塞 I/O 清理与回归校验

## 前置依赖
- Plan 1
- Plan 2

## 本次目标
- 清理 `crates/nova-agent/src/app/bootstrap.rs` 中 async 启动链路的剩余同步文件读取。
- 串联并落地 Plan 1/2 的异步接口，确保启动阶段不阻塞 Tokio runtime worker。

## 涉及文件
- `crates/nova-agent/src/app/bootstrap.rs`
- `crates/nova-agent/src/prompt.rs`（若需要暴露额外 async API）
- `crates/nova-agent/src/skill.rs`（异步加载调用落地）
- 相关测试文件（bootstrap/prompt/skill）

## 详细设计
1. `warn_unused_gateway_sections` 下沉
- 当前实现：
  - `fn warn_unused_gateway_sections(config: &AppConfig) -> Result<()>`
  - 内部 `std::fs::read_to_string(&config_path).ok()`
- 改造：
  - 升级为 `async fn warn_unused_gateway_sections(config: &AppConfig) -> Result<()>`
  - 使用 `tokio::fs::read_to_string(&config_path).await`，读取失败保持“静默跳过 + 不阻断启动”的现有语义（当前 `.ok()`）。
  - 保留遗留 section 检测列表与日志输出内容不变。

2. 启动流程串联
- `build_application` 中：
  - `warn_unused_gateway_sections(&config).await?;`
  - `skill_registry.load_from_dir_async(&skill_dir).await`
- 若 `prompt` 相关读取仍在同步函数中触发，统一前置到 async 加载并注入，避免在 `SystemPromptBuilder` 构建阶段触发阻塞读取。

3. 错误与日志边界
- 启动期 I/O 错误分级保持一致：
  - 非关键配置扫描失败（unused section 检测）不阻断主流程。
  - 关键配置/必要 prompt 读取失败沿 `Result` 上抛并带 `.context(...)`（若原先已如此则保持）。
- 避免多层重复日志：入口层输出一次即可。

4. 性能验证策略
- 在 debug 日志下记录关键加载步骤耗时（可选，非功能变更）。
- 重点验证“长目录技能扫描 + 大型 prompt 文件”场景下启动响应不被单线程阻塞。

## 测试案例
1. 正常路径
- 完整配置启动，`build_application` 返回应用实例；日志包含技能加载与 agent bootstrap 关键信息。

2. 边界条件
- 配置文件缺失或不可读：`warn_unused_gateway_sections` 不阻断，应用仍可启动（与当前行为一致）。
- skills 目录不存在：保持 `Ok` 降级与 warn 日志。

3. 异常路径
- prompt 文件读取失败：按现有策略返回空或错误，验证行为无回归。

4. 修复流程与回归
- 在 `D:/git/zero-nova` 根目录执行：
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --all --check`
  - `cargo test --workspace`
- 三项全部通过后，更新总览中 Plan 状态并进入下一实施阶段。
