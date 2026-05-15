# Plan 3: 用 enum 收缩封闭后端集合

## 前置依赖

Plan 1

## 任务目标

把 `BashTool` 和 `WebSearchTool` 内部对封闭后端集合的表达从 `dyn trait` 收缩为 `enum`。完成后：

- `ShellBackend` trait 不再存在。
- `SearchBackend` trait 不再作为工具内部的主分发机制。
- 平台差异和后端差异通过 `match` 明确表达，减少装箱与动态分发。

## 执行范围

| 类别 | 路径 | 说明 |
| --- | --- | --- |
| 必须修改 | `crates/nova-agent/src/tool/builtin/bash/mod.rs` | 改为 enum 后端 |
| 必须修改 | `crates/nova-agent/src/tool/builtin/bash/bash_windows.rs` | 返回具体 enum variant 或构建参数 |
| 必须修改 | `crates/nova-agent/src/tool/builtin/bash/bash_linux.rs` | 返回具体 enum variant 或构建参数 |
| 必须修改 | `crates/nova-agent/src/tool/builtin/web_search/mod.rs` | 改为 enum 后端 |
| 必须修改 | `crates/nova-agent/src/tool/builtin/web_search/types.rs` | 删除或瘦身 trait 类型定义 |
| 允许修改 | `crates/nova-agent/src/tool/builtin/web_search/duckduckgo.rs` | 适配 enum 调用 |
| 允许修改 | `crates/nova-agent/src/tool/builtin/web_search/google.rs` | 适配 enum 调用 |
| 允许修改 | `crates/nova-agent/src/tool/builtin/web_search/tavily.rs` | 适配 enum 调用 |
| 禁止修改 | `crates/nova-agent/src/tool/registry.rs` | 不改 Tool registry 抽象 |
| 禁止修改 | `crates/nova-agent/src/network.rs` | 不改 HTTP client 构造 |

## Agent 执行步骤

1. 在 `bash` 模块中删除 `ShellBackend` trait。
2. 为 `bash` 模块新增封闭后端 enum，明确覆盖当前支持的 shell 类型。
3. 将 `select_shell` 改为返回 enum variant 或等价具体类型，禁止继续返回 `Box<dyn ShellBackend>`。
4. 在 `BashTool` 中把 `shell.name()`、`shell.build_command()` 改为 `match` 分发。
5. 保留当前跨 shell 嵌套命令检查逻辑，但输入来源改为 enum 提供的稳定 shell 名称。
6. 在 `web_search` 模块中删除内部主分发对 `Box<dyn SearchBackend>` 的依赖。
7. 为 `web_search` 模块新增封闭后端 enum，覆盖 `Google`、`Tavily`、`DuckDuckGo`。
8. 将后端选择逻辑保留在 `WebSearchTool::with_client`，但最终持有值改为 enum variant。
9. 将 `definition()` 与 `execute()` 中的 backend name / search 调用改为 `match` 分发。
10. 保留现有显式 backend 选择优先级和 fallback 规则，不得修改用户可见配置语义。

## 目标数据结构 / 接口契约

目标方向示意：

```rust
enum ShellBackend {
    PowerShell(PowerShellBackend),
    Cmd(CmdBackend),
    UnixSh(UnixSh),
    UnixBash(UnixBash),
}

enum SearchBackend {
    Google(GoogleBackend),
    Tavily(TavilyBackend),
    DuckDuckGo(DuckDuckGoBackend),
}
```

若后端结构体本身已无独立存在必要，也可以继续下沉为构建参数加内部辅助函数，但必须保持集合封闭、分发显式。

## 行为规则

| 输入 / 场景 | 处理路径 | 期望结果 |
| --- | --- | --- |
| Windows 默认 shell | `select_shell` 返回 Windows variant | `BashTool` 正常执行 |
| Linux 默认 shell | `select_shell` 返回 Linux variant | `BashTool` 正常执行 |
| 检测跨 shell 嵌套 | 使用 enum 对应名称判断 | 行为保持一致 |
| 显式配置 `google` 且 key 完整 | 选择 `SearchBackend::Google` | 行为保持一致 |
| 显式配置 `tavily` 但缺 key | 进入 fallback | 行为保持一致 |
| 无显式配置但有 Google key | 按优先级选 Google | 行为保持一致 |
| 无其他 key | fallback 到 DuckDuckGo | 行为保持一致 |

## 禁止事项

- 不要把后端 enum 再包一层新的 trait。
- 不要修改搜索结果格式。
- 不要修改 shell 命令执行协议字段。
- 不要修改 `BashTool` 的超时、日志 flush 和输出截断语义。
- 不要新增依赖。

## 测试要求

| 测试文件 | 测试名称 | 输入 | 期望断言 |
| --- | --- | --- | --- |
| `crates/nova-agent/src/tool/builtin/bash/mod.rs` | `cross_shell_nesting_is_detected` | 现有输入 | 继续通过 |
| `crates/nova-agent/src/tool/builtin/bash/mod.rs` | `test_shell_execution` | `echo hello` | 继续通过 |
| `crates/nova-agent/src/tool/builtin/web_search/mod.rs` | 新增 `selects_google_when_explicit_backend_and_keys_present` | Google 配置完整 | 返回 Google variant |
| `crates/nova-agent/src/tool/builtin/web_search/mod.rs` | 新增 `falls_back_to_duckduckgo_when_selected_backend_is_unavailable` | 显式 Tavily 但缺 key | 返回 DuckDuckGo variant |

必须执行的验证命令：

```powershell
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
cargo test --workspace
```

## 完成条件

- [ ] `ShellBackend` trait 已删除
- [ ] `BashTool` 改为 enum 后端分发
- [ ] `select_shell` 不再返回 trait object
- [ ] `WebSearchTool` 不再持有 `Box<dyn SearchBackend>`
- [ ] backend 选择与 fallback 规则保持一致
- [ ] 现有 bash 相关测试继续通过
- [ ] 新增 web search 后端选择测试
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过
