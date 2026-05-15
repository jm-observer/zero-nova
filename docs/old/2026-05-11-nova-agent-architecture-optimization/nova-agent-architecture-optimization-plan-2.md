# Plan 2：运行时 I/O 与 HTTP 横切治理

## 前置依赖
- 无（可与 Plan 1 并行评审；实施时需关注会话加载路径是否复用相同 I/O 约束）

## 本次目标
- 收敛运行时热路径中的同步桥接逻辑，明确启动期与运行期 I/O 边界。
- 统一 provider、Web 工具以及可纳入范围的 voice HTTP Client 注入方式、超时策略和通用请求配置。
- 清理与运行时安全边界相关的 panic、`#[allow(...)]` 和隐式 fallback 实现。

## 涉及文件
- `crates/nova-agent/src/prompt/context.rs`
- `crates/nova-agent/src/skill/registry.rs`
- `crates/nova-agent/src/config/loaders.rs`
- `crates/nova-agent/src/app/bootstrap.rs`
- `crates/nova-agent/src/app/application.rs`
- `crates/nova-agent/src/provider/anthropic.rs`
- `crates/nova-agent/src/provider/openai_compat/mod.rs`
- `crates/nova-agent/src/provider/health.rs`
- `crates/nova-agent/src/tool/builtin/web_fetch.rs`
- `crates/nova-agent/src/tool/builtin/web_search/mod.rs`
- `crates/nova-agent/src/voice/openai_compat.rs`（若确认其生命周期可纳入共享 client）
- 可选新增：`crates/nova-agent/src/network.rs` 或 `crates/nova-agent/src/app/network.rs`

## 现状依据
- `crates/nova-agent/src/prompt/context.rs:299`、`crates/nova-agent/src/prompt/context.rs:378` 当前通过 runtime-aware 读取项目上下文文件。
- `crates/nova-agent/src/prompt/context.rs:404` 使用 `block_in_place + block_on(tokio::fs::read_to_string(...))`。
- `crates/nova-agent/src/skill/registry.rs:384`、`crates/nova-agent/src/skill/registry.rs:516` 读取 skill 文件；`crates/nova-agent/src/skill/registry.rs:945` 同样存在 runtime-aware 文件读取。
- `crates/nova-agent/src/provider/anthropic.rs:24`、`crates/nova-agent/src/provider/openai_compat/mod.rs:44`、`crates/nova-agent/src/tool/builtin/web_fetch.rs:19`、`crates/nova-agent/src/tool/builtin/web_search/mod.rs:27`、`crates/nova-agent/src/provider/health.rs:64` 均各自创建 HTTP client。
- `crates/nova-agent/src/tool/builtin/web_fetch.rs:24` 在自定义 client 构建失败时 fallback 到 `Client::new()`，可能隐藏配置错误。

## 详细设计
### 1. 明确 startup-only 与 runtime-only 路径
- `config/loaders.rs` 这种启动期读取配置文件的同步 I/O 可以保留，但需要在接口命名或文档中明确其使用边界。
- `prompt/context.rs`、`skill/registry.rs` 这种运行时可能频繁触发的路径，不应继续依赖 `read_to_string_runtime_aware()` 作为常规入口。
- 目标状态：
  - 启动期：允许同步读取，但接口职责明确，例如 `load_config_sync_at_startup()` 或文档明确只由 bootstrap 调用。
  - 运行期：统一使用 async 读取与 async 调用链，不再依赖 `block_in_place + block_on` 兼容。
  - 测试辅助：若测试必须同步读取 fixture，应限制在 `#[cfg(test)]` 或明确 helper 中。

### 2. Prompt 构建读路径收敛
- 对项目上下文、开发提示词、会话附加上下文等文件读取，整理出统一 async helper：

```rust
async fn read_optional_text_file(path: &Path) -> Result<Option<String>>;
async fn load_project_context_async(project_dir: &Path) -> Result<ProjectContext>;
```

- 缺失文件应返回 `Ok(None)` 或空上下文，保持当前“缺失不致命”的行为。
- 权限错误、非 UTF-8、目录被当作文件等真实 I/O 问题应返回带路径上下文的错误，除非当前语义明确要求跳过。
- 若某些纯同步调用者仍需要提示词读取能力，提供显式 startup helper，而不是自动 runtime-aware 桥接。

### 3. Skill registry 读取路径收敛
- `skill/registry.rs` 当前存在同步解析 skill 文件与目录遍历逻辑。
- 若 skill discovery 仅在启动期发生，可保留同步实现，但需：
  - 从运行时热路径移除调用。
  - 在类型或方法注释中标注 startup/discovery-only。
  - 移除 runtime-aware 桥接，避免在运行时误用。
- 若 skill discovery 可能在运行中刷新，应改为 async discovery：
  - 使用 `tokio::fs::read_to_string` 读取 `SKILL.md` / `skill.toml`。
  - 对目录遍历使用 `tokio::fs::read_dir`。
  - 将解析逻辑保持为纯函数，便于测试。

### 4. 共享 HTTP Client 设计
- 当前 provider 和 Web 工具分别自行 `Client::new()` 或 `Client::builder()`，导致以下问题：
  - 连接池无法跨模块复用。
  - timeout、header、User-Agent、代理、证书策略可能漂移。
  - 测试时需要分别替换多个 client 构造点。
- 推荐新增一个轻量网络装配类型，不引入新依赖：

```rust
#[derive(Clone)]
pub(crate) struct HttpClients {
    pub provider: reqwest::Client,
    pub web: reqwest::Client,
    pub health: reqwest::Client,
}
```

- client 可以是一个或多个，关键是统一 factory 创建：
  - provider client：适合模型 API，可能需要较长超时，不设置会污染请求的默认鉴权 header。
  - web client：适合抓取/搜索，统一 User-Agent、抓取超时与重定向策略。
  - health client：短超时、少重试，用于健康探测。
- 如果最终保留多个 client，也应由统一 factory 创建，而不是各模块直接 `Client::new()`。

### 5. 横切配置集中化
- 将散落在 `web_fetch`、`provider/health`、provider 实现里的 timeout、重定向和 fallback 策略整理为具名配置或常量。
- 配置归口建议：
  - provider 访问超时与健康检查超时归 provider/network 配置域。
  - Web 抓取与搜索超时归 search/tool 网络配置域。
  - User-Agent 和 redirect policy 归 web client factory。
- 目标不是暴露大量终端用户配置，而是先把 magic number 从业务逻辑中提取出来，方便统一治理。

### 6. Provider 与工具注入方式
- provider client 构造函数保留现有 `new()` / `from_config()` 作为兼容入口，但内部委托到带 client 的构造函数：

```rust
pub fn with_http_client(config: &ProviderConfig, http: reqwest::Client) -> Self;
```

- `app/bootstrap` 负责创建 `HttpClients` 并注入 provider、Web 工具注册流程。
- `web_fetch` 和 `web_search` 不再在 `new()` 内直接创建 client；可提供：
  - `new(config, http)` 用于生产。
  - `new_for_tests(http)` 或测试直接构造，用于注入 mock server client。
- 请求特定 header（如鉴权、Anthropic/OpenAI context headers）应在每次 request builder 上设置，避免作为 client default header 污染其它请求。

### 7. 错误处理与兼容收尾
- `web_fetch.rs` 中对 HTML selector 的解析不应出现 panic/fallback 到不明状态；selector 可在构造时解析并返回 `Result<Self>`，或使用确定不会失败的封装初始化。
- `Client::builder().build()` 失败不应静默 fallback 到 `Client::new()`；应返回错误或在启动阶段 fail fast，避免用户以为自定义超时/代理/redirect 已生效。
- provider 层现有 `#[allow(clippy::single_match)]` 需通过重写控制流消除，而不是压制规则。
- 保留 `anyhow::Result` + `?` 风格，错误上下文包含模块名、URL host 或文件路径等可定位信息。

### 8. 迁移步骤
1. 新增或确定 HTTP client factory 位置，集中创建 provider/web/health client。
2. 给 provider、web_fetch、web_search 增加带 client 注入的构造函数。
3. 修改 `bootstrap` / tool 注册流程注入共享 client。
4. 将 prompt context 文件读取改为 async helper，并更新调用链。
5. 决定 skill registry 是 startup-only 同步还是 runtime async，并删除 runtime-aware 桥接。
6. 清理 `unwrap()`、静默 fallback 和 clippy allow。
7. 更新单元测试和集成测试。

## 测试案例
### 正常路径
- 运行时 prompt 构建所需上下文文件能够通过纯 async 路径成功读取。
- provider 和 Web 工具均能通过注入后的共享 client 正常发起请求。
- 健康检查使用短超时 client，不影响 provider 主请求 client。
- skill discovery 在选定生命周期内正常读取 `SKILL.md` 与 `skill.toml`。

### 边界条件
- 缺失项目上下文文件或开发提示词文件时，行为与当前保持兼容，只是返回空或跳过，不误报致命错误。
- 不同模块共享 client 时，不会互相污染特定请求 header。
- web client redirect 限制生效，provider client 不继承 web 抓取的 User-Agent 或 redirect 限制，除非明确配置。
- 测试中可注入自定义 client，不需要依赖真实外网。

### 异常场景
- 读取文件失败、网络超时、无效 selector、client 构建失败都能返回显式错误上下文，不出现 panic。
- clippy 检查下不再依赖 `#[allow(...)]` 压制实现问题。
- provider 请求构造失败不会被误包装成模型返回失败，应能区分配置/网络/协议错误。

## 验收标准
- 运行时 prompt 相关文件读取不再使用 `block_in_place + block_on` 桥接。
- `skill/registry.rs` 不再提供会被运行时误用的 runtime-aware 同步读取入口。
- provider、web_fetch、web_search、health probe 的 HTTP client 由统一 factory 创建或注入。
- client 构建失败不再静默 fallback 到默认 client。
- 通过 `cargo clippy --workspace -- -D warnings`、`cargo fmt --all`、`cargo test --workspace`。
