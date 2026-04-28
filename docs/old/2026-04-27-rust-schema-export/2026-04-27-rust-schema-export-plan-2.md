# Plan 2：Schema 工件布局与版本化约束

## 前置依赖
- Plan 1

## 本次目标
- 重整 `schemas/` 目录，使其成为 Rust 导出产物而非手工维护目录。
- 明确 registry、snapshot、fixture 与域目录的职责边界。
- 为新增消息类型建立“必须导出、必须登记、必须测试”的约束机制。

## 涉及文件
- `schemas/registry.json`
- `schemas/root/schema-root.json`
- `schemas/domains/**`
- `schemas/fixtures/**`
- `schemas/domains_snapshot.txt`
- `crates/nova-protocol/src/schema.rs`

## 详细设计

### 1. 目录职责重定义
- `schemas/domains/`：仅保存 Rust 自动导出的 domain schema。
- `schemas/root/`：保存聚合入口，例如完整的 root registry / bundle 信息。
- `schemas/fixtures/`：保存可跨 Rust 与前端共用的协议样例消息。
- `schemas/registry.json`：记录导出的根类型、文件路径、领域分类、是否允许前端直接消费。
- `schemas/domains_snapshot.txt`：作为人类可读的变更快照，服务于 code review。

### 2. `registry.json` 设计
- 每个条目至少包含：
  - `name`：Rust 根类型名
  - `domain`：`gateway` / `chat` / `session` / `observability` / `system`
  - `path`：schema 相对路径
  - `kind`：`message` / `payload` / `event` / `request` / `response`
  - `frontend`：布尔值，标明是否供前端生成使用
- 前端生成脚本只读取 `frontend = true` 的条目，避免把内部中转 DTO 一并暴露到 TypeScript。

### 3. Fixture 策略
- fixture 继续保留“有效消息”和“无效消息”两类：
  - 有效样例用于 Rust / 前端双端 contract test；
  - 无效样例用于验证 schema 会在预期位置报错。
- 本次需要新增至少以下 fixture：
  - `agent_inspect.json`
  - `workspace_restore.json`
  - `invalid_agent_inspect_missing_session_id.json`
  - `invalid_workspace_restore_missing_payload.json`
- fixture 的 JSON 结构必须与 `GatewayMessage` 根 schema 对齐，而不是只保留 payload 片段。

### 4. 版本化策略
- 本阶段采用“跟随主干、提交工件”的仓库内版本策略，不额外引入 `v1/`、`v2/` 多版本目录。
- 原因：当前协议仍处于快速收敛期，多版本目录会放大维护成本，且前后端同仓协作更适合单线演进。
- 若后续出现外部 SDK 或多客户端并存，再单独演进为语义版本化目录结构。

### 5. 漂移检测
- `export-schema` 每次运行后必须稳定输出相同顺序的字段与文件，避免无意义 diff。
- CI 中以“重新导出后 git diff 为空”为准检测 schema 漂移。
- 对 registry 增加一致性检查：
  - registry 中的 path 必须存在；
  - 标记 `frontend = true` 的 schema 必须能被前端生成脚本解析；
  - fixture 引用的消息类型必须出现在 registry 中。

## 测试案例
- 正常路径：运行导出命令后生成完整目录、registry、snapshot。
- 边界条件：新增 schema 条目时文件排序稳定，不因 HashMap 顺序产生噪音 diff。
- 异常场景：registry 指向不存在文件、fixture 对应未知消息类型时，验证脚本应失败。
- 回归场景：`agent.inspect` 与 `workspace.restore` 的有效/无效样例均被纳入 fixture 体系。

