# Agent Guidelines

## 基本行为
- 使用**中文**与用户交流
- 遇到需求不明确时，主动提问，不自行假设
- 修改现有代码前，先理解当前实现意图
- 单次变更保持小而聚焦，不将重构混入功能修改

## 项目概述
Rust 异步应用程序。<!-- 补充具体业务功能描述 -->

## 技术栈

| 关注点      | 选型与约束 |
|------------|-----------|
| 异步运行时  | `tokio`（full features） |
| 日志        | `log` 宏（`info!`、`error!` 等）；初始化用 `custom_utils::logger::logger_feature`；**禁止** `println!` 输出应用日志 |
| HTTP 客户端 | `reqwest` + `rustls-tls`（无 OpenSSL 依赖）；必须 `default-features = false` 并显式启用所需 feature，如 `reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls", "stream"] }`；始终用异步 `Client`，**禁止**阻塞 API |
| 错误处理    | `anyhow::Result` + `?` 传播；需上下文时加 `.context("...")` |
| 序列化      | `serde` + `serde_json` |

## 代码结构
- `src/main.rs`：只做运行时启动和日志初始化，保持精简
- `src/lib.rs` 及子模块：承载所有业务逻辑；规模增长时拆分子模块，通过 `lib.rs` 统一导出

## 代码质量

**格式**：遵循 `rustfmt.toml`（120 列，4 空格缩进）和 `clippy.toml` 阈值。

**错误处理**：
- `lib.rs` 及子模块：禁止 `.unwrap()` / `.expect()`，一律用 `?` + `anyhow::Result`
- `main.rs` 和测试代码：允许 `.unwrap()`
- 禁止用 `#[allow(...)]` 压制警告；确有必要时须在注释中说明理由

**安全与性能**：
- **禁止 `unsafe`**：不得使用 `unsafe` 代码块，除非有充分理由并在注释中详细说明必要性和安全保证
- **避免不必要的 `.clone()`**：优先使用借用（`&T`），仅在确需所有权转移时才克隆；review 时关注无意义的 `.clone()` 调用
- **禁止阻塞异步运行时**：`async` 上下文中禁止调用阻塞操作（`std::fs`、`std::thread::sleep`、同步网络 I/O 等），使用 `tokio` 对应的异步 API 或 `tokio::task::spawn_blocking`
- **可见性最小化**：模块、结构体、函数默认私有，仅在需要外部访问时标记 `pub(crate)` 或 `pub`，避免过度暴露内部实现

**依赖管理**：
- 未经用户明确同意，不得添加新依赖
- 引入前评估必要性，优先选维护良好、传递依赖少的 crate
- 不确定选型时，向用户列出候选方案及取舍，不自行决定
- **Workspace 依赖统一**：若项目为 workspace，所有成员 crate 的 `[dependencies]` 必须使用 `{ workspace = true }` 形式引用依赖，版本统一在根 `Cargo.toml` 的 `[workspace.dependencies]` 中声明

## 计划与设计文档

进行功能计划、方案设计或架构调整时，**必须**在项目根目录的 `docx/` 目录下创建对应的设计文档（Markdown 格式）。

文档应包含以下内容：

| 章节       | 说明 |
|-----------|------|
| 时间       | 文档创建 / 最后更新日期 |
| 项目现状    | 当前相关模块或功能的状态概述 |
| 本次目标    | 本次设计 / 计划要达成的目标，清晰可验证 |
| 详细设计    | 方案说明、模块划分、接口定义、数据流、关键实现思路等 |
| 测试案例    | 覆盖正常路径、边界条件、异常场景的测试用例设计 |
| 风险与待定项 | 已知风险、待确认事项或后续迭代计划（可选） |

命名建议：`docx/<日期>-<简要主题>.md`，例如 `docx/2026-04-19-metrics-export-design.md`。

## 修复流程
每次代码修改后，必须按以下循环执行，**全部通过才视为完成**：

1. `cargo clippy --workspace -- -D warnings`
2. `cargo fmt --check --all`（仅格式化本项目 crate；若存在 git submodule 等外部项目，在对应目录的 `rustfmt.toml` 中添加 `ignore` 或从 `workspace.members` 中排除，确保 `cargo fmt` 不触及非本项目代码）
3. `cargo test --workspace`

若任一步骤失败，继续修复并重新执行完整循环，直到三项全部通过。  
**禁止在循环未完成时停下来，不得以"请你测试一下"结束任务。**

> 所有命令均在 workspace 根目录执行，覆盖全部 crate。

## CI / 发布
- 构建目标：`x86_64-pc-windows-msvc`、`aarch64-unknown-linux-gnu`
- 推送 `v*` 标签触发 Release；推送前本地确认修复流程全部通过
