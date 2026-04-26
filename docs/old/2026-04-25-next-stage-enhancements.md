# Zero-Nova 下一阶段完善与增强方向

> 生成日期：2026-04-25
> 基于仓库代码全景分析

---

## 一、现有系统概况

### 1.1 架构总览

Zero-Nova 是 AI Agent 框架，运行时采用三层架构：

```
┌─────────────────────────────────────────────────────────┐
│                    桌面层 (Deskapp)                        │
│  Tauri Shell (Rust) + TypeScript/Vite Frontend           │
└─────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────────────────────────────────┐
│                   网关层 (Gateway)                        │
│  WebSocket (18801) / stdio NDJSON / CLI REPL             │
│  nova-server-ws / nova_gateway_stdio / nova_cli          │
└─────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────────────────────────────────┐
│                   核心层 (Core)                           │
│  nova-core · nova-protocol · nova-gateway-core           │
└─────────────────────────────────────────────────────────┘
```

### 1.2 Crates 清单

| Crate | 类型 | 职责 |
|-------|------|------|
| `nova-core` | lib | 核心代理循环、LLM 客户端、工具、MCP |
| `nova-protocol` | lib | JSON DTO 类型定义（20+ 消息类型） |
| `nova-gateway-core` | lib | 网关路由与编排 |
| `nova-app` | lib | 应用门面、Tauri 后端入口 |
| `nova-conversation` | lib | 会话状态与历史管理 (SQLite) |
| `nova-cli` | binary | CLI REPL |
| `nova-server-stdio` | binary | NDJSON over stdio 服务器 |
| `nova-server-ws` | binary | WebSocket 服务器 |
| `channel-core` | lib | 通道 trait 定义 |
| `channel-stdio` | lib | stdio 通道实现 |
| `channel-websocket` | lib | 通用 WebSocket 通道 |

### 1.3 测试覆盖统计

| 位置 | 测试数量 | 类型 |
|------|----------|------|
| `nova-protocol` | 5 | `#[test]` — 序列化验证 |
| `nova-core/src/config.rs` | 11 | `#[test]` — 配置加载 |
| `nova-core/src/skill.rs` | 6 | `#[test]` — Skill 策略 |
| `nova-core/src/prompt.rs` | 5 | `#[test]` — 提示词构建 |
| `nova-core/src/agent_catalog.rs` | 4 | `#[test]` — Agent 注册 |
| `nova-core/src/mcp/tests.rs` | 3 | `#[test]` — JSON-RPC |
| `nova-core/src/tool.rs` | 2 | `#[tokio::test]` |
| `nova-core/src/tool/builtin/bash.rs` | 3 | 混合 |
| `nova-core/src/tool/builtin/task.rs` | 2 | `#[test]` |
| `nova-cli/src/main.rs` | 13 | `#[test]` — 路径/配置 |
| `nova-app/tests/bootstrap_paths.rs` | 6 | `#[test]` — 集成测试 |
| **总计** | **58** | 混合 |

---

## 二、现状深度分析

### 2.1 配置系统

**.nova/config.toml 配置段：**

| 配置段 | 关键字段 | 说明 |
|--------|----------|------|
| `[llm]` | api_key, base_url, model, max_tokens, temperature, top_p, thinking_budget | LLM 连接 |
| `[search]` | backend, tavily_api_key | 搜索后端 |
| `[gateway]` | host, port, max_iterations, tool_timeout_secs | 网关参数 |
| `[[gateway.agents]]` | id, display_name, description, aliases, system_prompt_template | Agent 注册 |
| `[gateway.interaction]` | default_ttl_seconds, timeout_action | 挂起交互超时 |
| `[gateway.interaction.risk]` | low/medium/high_min_confidence | 风险等级 |
| `[gateway.trimmer]` | max_history_tokens, preserve_recent | 历史裁剪 |
| `[gateway.workflow]` | enabled_types, max_lifetime_seconds | 工作流 |
| `[sidecar]` | mode, name, command | Sidecar 管理 |

**问题：双份配置定义**
- `AppConfig` 在 `nova-core/src/config.rs` 和 `deskapp/src-tauri/src/config.rs` 中各有一份定义
- 来源不同（TOML vs Tauri 配置），可能导致不一致

### 2.2 工具系统

**已实现工具清单：**

| 工具名 | 类型 | 源码文件 | 是否有测试 |
|--------|------|----------|------------|
| Bash | builtin | bash.rs | 有 (3个) |
| Read | builtin | read.rs | 无单独测试 |
| Write | builtin | write.rs | 无单独测试 |
| Edit | builtin | edit.rs | 无单独测试 |
| Agent | builtin | agent.rs | 无单独测试 |
| WebSearch | builtin | web_search/mod.rs | 无单独测试 |
| WebFetch | builtin | web_fetch.rs | 无单独测试 |
| ToolSearch | builtin | tool_search.rs | 有集成测试 |
| TaskCreate/TaskList/TaskUpdate | deferred | task.rs | 有 (2个) |
| Skill | deferred | skill.rs | 无单独测试 |

**问题：**
1. 工具注册系统缺少 `unregister()` 方法（仅启动时一次性加载）
2. `TaskCreate` 输入 schema 使用驼峰 `activeForm`，但与 `Task` 内部结构体 `active_form` 命名不一致

### 2.3 内存系统

**数据库表结构：**

```sql
-- 会话表
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    title TEXT,
    agent_id TEXT,
    created_at INTEGER,
    updated_at INTEGER
);

-- 消息表
CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
```

**问题：**
1. **向量搜索缺失** — SQLite 没有向量列或 FTS 全文搜索索引
2. **迁移机制简单** — 只有 `migrate_messages_timestamp_column()`，没有版本号表和迁移日志
3. **前端 Memory/Distillation 视图未实现** — 只有 API 接口

### 2.4 MCP 集成

**已实现：**

| MCP 功能 | 实现状态 |
|----------|----------|
| JSON-RPC 2.0 | 完整 |
| initialize / notifications/initialized | 完整 |
| tools/list | 完整 |
| tools/call | 完整 |
| stdio 传输 | 完整 |
| WebSocket 传输 | 条件编译 `mcp-websocket` |
| 错误处理 | 基本 |

**缺失：**

| 缺失特性 | 位置 |
|----------|------|
| `notifications/resources/update*` | `mcp/client.rs` |
| `notifications/sampling/createMessage` | `mcp/client.rs`（注释掉的部分） |
| 工具变更通知 | `mcp/transport.rs` |
| 类型化 ServerCapabilities | `mcp/types.rs`（当前用 `Option<Value>`） |

### 2.5 前端实现 vs 协议

| 类别 | 前端已实现 | 备注 |
|------|-----------|------|
| Chat | ✅ | chat(), stopTask(), onProgress() |
| Sessions | ✅ | CRUD + 复制 |
| Agents | ✅ | CRUD + 切换 |
| Config | ✅ | 获取/更新 |
| MCP | ✅ | 注册/注销/调用 |
| Memory | ❌ 空 | API 完整但无视图 |
| Distillation | ❌ 空 | API 完整但无视图 |
| Evolution | ✅ | 完整的确认/锻造流程 |
| Browser | ⚠️ 半 | 接口预留但无面板 |
| Voice | ✅ | 录音 + 流式 TTS + 打断 |

### 2.6 代码标准遵循情况

**已遵循：**
- ✅ `anyhow::Result` + `?` 在 lib 代码中
- ✅ tokio 异步，禁止阻塞 API
- ✅ reqwest with rustls-tls（无 OpenSSL）
- ✅ `log` 宏（`println!` 仅用于 main.rs）
- ✅ `pub(crate)` 默认私有
- ✅ Clippy + fmt 集成

**待改进：**
- ⚠️ 前端 TypeScript 大量 `as any` 断言
- ⚠️ `cargo-macne check` 不含 `cargo test`
- ⚠️ 无 `rust-toolchain.toml` 版本锁定

---

## 三、下一阶段完善方向

### 3.1 🟢 高优先级 — 关键缺失

#### 3.1.1 前端文件预览命令缺失

**问题：** `deskapp/src/ui/modals.ts:116-125` 调用了三个 Tauri 命令，但 `deskapp/src-tauri/src/lib.rs` 未全部注册：

```typescript
// modals.ts 调用
await invoke<any>('file_stat', { filePath });        // ❌ 未注册
await invoke<any>('file_read', { filePath });        // ⚠️ 已注册但接口不一致
await invoke<any>('file_read_text', { filePath });   // ❌ 未注册
```

**方案：** 在 `deskapp/src-tauri/src/lib.rs` 中补充 `file_stat` 和 `file_read_text` 命令。

---

#### 3.1.2 向量语义搜索缺失

**问题：** 前端 `gateway-client.ts` 已有 `memory_search()`, `distillation_graph()` 等 API 方法，但后端 SQLite **没有向量列或 FTS 全文搜索索引**。

**方案：**
1. 引入 `libsql` 或 `sqlite-vss` 扩展
2. 为 `messages` 表添加 embedding 列
3. 实现 embedding 生成（可复用现有 LLM 客户端）
4. 补齐前端 Memory/Distillation 视图

---

#### 3.1.3 Gateway Sidecar 日志轮转

**问题：** `deskapp/src-tauri/src/commands/gateway.rs:255` 中日志每次启动都截断：

```rust
.truncate(true)  // 每次启动覆盖
```

**方案：** 实现基于时间或大小的日志轮转，支持多文件保留。

---

### 3.2 🟡 中优先级 — 功能完善

#### 3.2.1 Skill System 深化

**当前状态标记：**

| 标记 | 位置 | 内容 |
|------|------|------|
| Phase 4a | openai_compat.rs:150 | stream_options to get usage |
| Phase 4b | openai_compat.rs:157 | generic reasoning toggle |
| Phase 1 | agent.rs:509 | Sticky + LLM 路由 |
| Phase 1 | agent.rs:581 | 历史切片 |
| Plan | skill.rs:537 | SkillRouter 辅助方法 |

**方案：**
1. 实现 Sticky 期间子代理隔离
2. 实现 `<skill_exit/>` 标记退出机制（design doc §7）
3. 补充 LLM 分类路由（当前纯规则匹配 `skill.rs:537`）
4. 实现 `stream_options` 注入（Phase 4a）
5. 完善 reasoning toggle provider 适配（Phase 4b）

---

#### 3.2.2 MCP 协议扩展

**缺失特性：**

| 缺失特性 | 优先级 | 位置 |
|----------|--------|------|
| `notifications/resources/update*` | 中 | mcp/client.rs |
| `notifications/sampling/createMessage` | 中 | mcp/client.rs |
| 工具变更通知 | 低 | mcp/transport.rs |
| 类型化 ServerCapabilities | 中 | mcp/types.rs |

**方案：** 按 MCP 规范补全通知流，替换 `Option<Value>` 为结构化类型。

---

#### 3.2.3 工具系统生命周期管理

**问题：** 当前工具注册缺少 `unregister()` 方法，仅支持启动时一次性加载。

**方案：** 在 `ToolRegistry` 中添加 `unregister()` 和 `reload()` 方法，支持动态工具管理。

---

#### 3.2.4 多 Provider 支持

**问题：** 当前 `LlmConfig` 仅支持单一 provider，但 `SearchConfig` 已支持 DuckDuckGo/Google/Tavily 多后端。

**方案：** 扩展 LLM provider 管理，支持按 Agent/Prompt 切换 LLM 提供商。

---

### 3.3 🔵 架构与可扩展性

#### 3.3.1 异步 vs 阻塞 API 统一

**问题：** 部分异步上下文调用了阻塞 API（std::fs, std::thread::sleep, sync I/O）。

**方案：** 全局扫描，替换为 tokio 等效项或使用 `spawn_blocking`。

---

#### 3.3.2 前端 TypeScript 类型安全

**问题：** `gateway-client.ts` 中大量使用 `as any` 断言：

```typescript
// deskapp/src/main.ts:77
state.setAgents(agents as any);
state.setSessions(sessions as any);
```

**方案：** 定义完整的数据类型引用协议 DTO，消除 `as any`。

---

#### 3.3.3 跨平台支持补全

| 平台 | 问题 | 位置 |
|------|------|------|
| Linux | `file_reveal` 是 `todo` | file.rs:179 |
| macOS | `USERPROFILE` 环境变量使用 | config.rs:64 |

---

### 3.4 🟣 代码质量与 CI

#### 3.4.1 测试覆盖率提升

**当前状态：** 58 个测试，主要覆盖配置加载和 Skill 策略。

**方案：**
1. 各工具内置加单元测试（当前 Read/Write/Edit/Agent/WebSearch/WebFetch 无独立测试）
2. 补充 MCP transport 测试
3. 前端 E2E 测试（Vitest + Puppeteer）
4. `cargo-make check` 加入 `cargo test`

---

#### 3.4.2 CI/CD 完善

**当前状态：** `release.yml` 仅支持 `v*` 标签触发，仅构建 Windows x86_64 和 Linux ARM64。

**方案：**
1. 增加 PR 触发（完整 check cycle）
2. 补充 Windows ARM64 构建
3. 增加 macOS 桌面构建
4. 添加 `rust-toolchain.toml` 版本锁定

---

#### 3.4.3 文档补全

| 缺失 | 建议 |
|------|------|
| API 文档 | OpenAPI/Swagger 为 Gateway 协议生成 |
| 工具清单 | 自动生成工具描述文档 |
| 部署指南 | 各平台部署手册（含 Linux ARM64 交叉编译） |

---

## 四、实施建议

### 阶段一（基础完善）
1. 补齐文件预览 Tauri 命令
2. 修复 `file_reveal` Linux TODO
3. 日志轮转实现
4. 工具命名一致性修复

### 阶段二（功能增强）
1. 向量搜索引入 + 前端 Memory 视图
2. Skill Sticky 机制 + 退出标志
3. MCP 通知扩展
4. LLM Provider 多提供商支持

### 阶段三（架构优化）
1. 前端 TypeScript 类型补全
2. 异步 API 统一
3. 测试覆盖提升到 70%+
4. CI/CD 完善
