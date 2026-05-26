# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 语言

- 与用户沟通使用中文。
- 需求不明确时先澄清，不要自行假设。

## 常用开发命令

### Rust 工作区

```bash
# 构建
cargo build --workspace --release

# 全量检查循环（任务完成前必须全部通过）
cargo clippy --workspace -- -D warnings
cargo fmt --all
cargo test --workspace

# 运行 CLI REPL
cargo run -p nova-cli --bin nova_cli -- chat

# 运行一次性 prompt
cargo run -p nova-cli --bin nova_cli -- run "your prompt"

# 启动 Gateway（WebSocket）
cargo run -p nova-server-ws --bin nova-server-ws

# 启动 Gateway（stdio）
cargo run -p nova-server-ws --bin nova_gateway_stdio
```

### 单元测试 / 定位测试

```bash
# 运行某个 crate 的全部测试
cargo test -p nova-agent

# 运行单个测试（按测试名过滤）
cargo test -p nova-agent test_parse_skills_command

# 运行某个测试文件的相关测试（名称过滤）
cargo test -p nova-cli cli
```

### 桌面端（Tauri + 前端）

```bash
# 在 deskapp/ 目录执行
cd deskapp

# 前端开发
pnpm dev

# 完整桌面开发（含 tauri shell）
pnpm tauri dev

# 前端单测
pnpm test

# 运行单个前端测试文件
pnpm test -- src/__tests__/chat-service.test.ts

# E2E
pnpm test:e2e

# 桌面构建
pnpm tauri build
```

## 修复流程（必须）

每次代码修改后，必须按以下循环执行，**全部通过才视为完成**：

1. `cargo clippy --workspace -- -D warnings`
2. `cargo fmt --check --all`
3. `cargo test --workspace`

若任一步骤失败，继续修复并重新执行完整循环，直到三项全部通过。  
**禁止在循环未完成时停下来，不得以"请你测试一下"结束任务。**

> 所有命令均在 workspace 根目录执行，覆盖全部 crate。

### Schema 同步（改协议 DTO 后必须执行）

只要改动了 `nova-protocol` 中导出 schema 的类型（payload / envelope / 工具相关 DTO 等），必须按顺序重新生成两份契约文件并一起提交，否则 CI 的 `schema-check` 或 `frontend-check` 会失败：

1. `cargo run -p nova-protocol --bin export-schema --features export-schema -- --root .`  
   → 更新 `schemas/registry.json`、`schemas/root/`、`schemas/domains/`、`schemas/domains_snapshot.txt`
2. `cd deskapp && pnpm generate:schemas`  
   → 更新 `deskapp/src/generated/{schema-types,schema-validators,generated-types}.ts`
3. 校验：`git diff --exit-code -- schemas/ deskapp/src/generated/` 应无输出
4. 与本次协议变更放进同一个 commit 提交（不要单独留到下一次）

## 架构速览（Big Picture）

Zero-Nova 是三层结构的 Agent Runtime：

1. **Gateway Sidecar（Rust）**
   - 对外提供 stdio / WebSocket 通道，处理协议路由、会话事件、工具执行编排。
   - 入口：`crates/nova-server/src/bin/nova_gateway_stdio.rs`、`crates/nova-server/src/bin/nova_gateway_ws.rs`。
   - 路由与分发核心：`nova-gateway-core`（`GatewayHandler` + router/bridge/push center）。

2. **Agent Core（Rust）**
   - 核心能力集中在 `nova-agent`：
     - turn 执行与 runtime（`agent/`）
     - prompt 组装与裁剪（`prompt/`）
     - tool registry 与内置工具（`tool/`）
     - skill 注册与策略（`skill/`）
     - 会话与持久化（`conversation/`，SQLite）
     - provider 适配（`provider/`）
     - orchestrator 编排（`orchestrator/`：planner / scheduler / reviewer）
   - `nova-protocol` 定义 Gateway 的消息 DTO 与 envelope，是前后端协议契约。

3. **桌面壳 + 前端（Tauri + TS/Vite）**
   - `deskapp/src-tauri`：负责 sidecar 生命周期管理、系统能力暴露（tauri commands）、托盘与窗口控制。
   - `deskapp/src`：通过 `GatewayClient` 走 WebSocket 协议收发消息，组织 UI 状态、会话与进度事件渲染。

## 关键 crate 职责

- `nova-agent`：Agent 主循环、工具系统、技能系统、会话与状态。
- `nova-protocol`：前后端共享协议类型（消息 envelope / payload）。
- `nova-gateway-core`：把通道层请求分发到 AgentApplication，并做事件桥接。
- `nova-server-ws`（路径 `crates/nova-server`）：统一 server 入口（stdio + ws 二进制）。
- `channel-core`：通道抽象与 stdio/ws 传输适配。
- `nova-cli`：本地 REPL/one-shot 调试入口。
- `deskapp/src-tauri`：桌面容器，管理 sidecar 生命周期。

## 配置与运行时关系

- 工作区配置文件：`.nova/config.toml`。
- `nova-cli` 与 `nova-server-ws` 都通过 `resolve_workspace + AppConfig::load_from_file` 读取配置。
- 桌面端通过 tauri command `get_gateway_config` 获取 sidecar 网关地址，再由前端 `GatewayClient` 建连。

## 代码约束

**错误处理**：
- lib 代码使用 `anyhow::Result` + `?`；需上下文时加 `.context("...")`。
- `unwrap/expect` 仅限 `main.rs` 和测试。
- 禁止用 `#[allow(...)]` 压制警告；确有必要时须在注释中说明理由。
- **禁止隐式吞错**：不得使用空 `match` 分支或 `if let Err(_) = ...` 忽略错误，除非有明确注释。

**异步**：基于 tokio；禁止在 async 上下文调用阻塞 I/O（使用 tokio API 或 `spawn_blocking`）。

**HTTP**：`reqwest` 必须禁用默认特性并使用 rustls（禁止 OpenSSL）。

**日志**：使用 `log` 宏；应用日志禁止 `println!`；同一错误不得在多层重复打印。

**依赖**：未经用户明确同意不得新增依赖；workspace 成员通过 `{ workspace = true }` 引用。

**`unsafe`**：默认禁止；确需使用必须写清安全性证明。

**可维护性**：
- **可见性最小化**：模块、结构体、函数默认私有，仅在需要外部访问时标记 `pub(crate)` 或 `pub`。
- **函数长度**：尽量在 60 行以内；超过 100 行时必须拆分或注明理由。
- **参数数量**：建议不超过 4 个；超过时优先引入结构体参数。
- **嵌套层级**：避免超过 3 层；通过提前返回或辅助函数降低复杂度。
- **路径引用**：禁止函数体内使用全限定路径，应在文件顶部 `use` 导入后使用短名称。
- **避免无意义 `.clone()`**：优先使用借用，仅在确需所有权转移时克隆。

## 设计与计划文档（必需）

进行功能计划、方案设计或架构调整时，**必须先**在 `docs/` 下编写文档，再实施代码变更。

### 文档结构

每次设计任务创建一个以 `<日期>-<主题>` 命名的子目录，并将总览文档和所有 Plan 子文档放在该目录中：

```text
docs/
└── <YYYY-MM-DD>-<topic>/
    ├── <topic>.md              # 总览文档
    ├── <topic>-plan-1.md
    ├── <topic>-plan-2.md
    └── ...
```

长期设计资产与设计影响记录：

```text
docs/
├── design/                     # 长期系统设计基线（稳定结构、核心流程）
│   ├── system-overview.md      # 必须存在的统一入口文档
│   └── <module>-design.md
└── adr/                        # 设计影响记录（每次架构变更补充一条）
    └── <YYYY-MM-DD>-<topic>.md
```

### 总览文档应包含

- 时间（创建/更新）
- 项目现状
- 整体目标
- Plan 拆分（依赖关系、执行顺序、完成状态）
- 风险与待定项（可选）

### 每个 Plan 子文档应包含

| 章节 | 说明 |
|------|------|
| Plan 编号与标题 | 如 `Plan 1: 数据模型定义` |
| 前置依赖 | 无则标注"无" |
| 任务目标 | 可验证的完成结果 |
| 执行范围 | 必须修改 / 允许修改 / 禁止修改的文件 |
| Agent 执行步骤 | 按顺序列出文件级修改动作，具体到函数/字段/分支 |
| 目标数据结构 / 接口契约 | 直接给出最终 enum、struct、函数签名或协议格式 |
| 行为规则 | 用表格列出输入、处理路径、期望输出 |
| 禁止事项 | 明确本 Plan 不允许做的事 |
| 测试要求 | 测试文件、测试名、输入、期望断言及验证命令 |
| 完成条件 | 可检查的 checklist |

**Agent 执行步骤**必须使用命令式描述（"必须、禁止、保留、新增、删除、修改、验证"），不得使用开放式表述（"建议、可以、视情况"）。

**完成条件**示例：
- [ ] 目标 enum / struct 已定义
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo test --workspace` 通过

### 执行流程

1. 编写总览文档，明确目标和 Plan 拆分
2. 按顺序编写每个 Plan 子文档
3. 提交用户评审
4. 按 Plan 顺序实施，每完成一个 Plan 执行一次修复流程
5. 若变更影响长期设计资产：更新 `docs/design/` 对应文档，并在 `docs/adr/` 新增影响记录
6. 更新总览文档中对应 Plan 的状态为「已完成」，并执行 commit

## CI / 发布

- 目标平台：`x86_64-pc-windows-msvc`、`aarch64-unknown-linux-gnu`。
- 推送 `v*` tag 会触发发布流程。
- 推送前必须本地完成修复流程全部通过（clippy + fmt + test）。
