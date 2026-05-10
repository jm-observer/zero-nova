# Plan 2: prompt.rs 同步加载入口下沉

## 前置依赖
- 建议先完成 Plan 1（非强依赖）

## 本次目标
- 清理 `crates/nova-agent/src/prompt.rs` 中 async 调用路径上的同步文件读取。
- 建立“异步主入口 + 同步兼容入口”的清晰边界，避免在 Tokio runtime worker 上执行阻塞 I/O。

## 涉及文件
- `crates/nova-agent/src/prompt.rs`
- `crates/nova-agent/src/app/bootstrap.rs`（若涉及 workflow/project context 调用链）
- `crates/nova-agent/src/prompt.rs` 测试模块

## 详细设计
1. 现状分层与问题点
- 已异步：`load_developer_project_prompt_async`、`load_prompt_file`（tokio）。
- 同步风险点：
  - `load_single_project_context`（`std::fs::read_to_string`）
  - `load_developer_project_prompt`（同步版本）
  - `WorkflowStagePrompts::load_from_file`（同步读取）

2. 设计原则
- 原则 A：async 业务路径优先调用 async I/O 函数。
- 原则 B：同步 API 仅作兼容层，不允许在核心 async 主链路中直接使用。
- 原则 C：保留函数语义与日志边界，不改变业务选择规则（按文件顺序、空文件跳过、不存在跳过）。

3. 具体改造方案
- 项目上下文加载：
  - 新增 `async fn load_single_project_context_async(path: &Path) -> Option<String>`，内部使用 `tokio::fs::read_to_string`。
  - 新增 `pub async fn load_project_context_with_config_async(...) -> Option<String>`，替代 async 链路中的同步版本调用。
  - 同步版保留，必要时内部通过 `std::fs`（仅非 async 使用）或明确注释限制。

- Developer prompt 加载：
  - 保留 `load_developer_project_prompt_async` 作为主实现。
  - 同步 `load_developer_project_prompt` 转为兼容层，文档标注“仅非 async 场景”。
  - 若存在 async 场景误用同步版，统一切换调用点到 async 版本。

- Workflow stage 加载：
  - 新增 `pub async fn load_from_file_async(path: &Path) -> anyhow::Result<Self>`。
  - 解析逻辑抽取为纯内存函数 `fn parse_workflow_stage_content(content: &str) -> Self`，避免同步/异步双份逻辑漂移。
  - 原 `load_from_file` 保留并复用同一解析函数。

4. 调用链落点
- 检查 `SystemPromptBuilder::from_config` 与 bootstrap 初始化流程中是否触达同步加载函数。
- 若触达，将读取流程提前到 async 环节并把内容注入配置对象，避免 builder 内部触发同步 I/O。

## 测试案例
1. 正常路径
- `load_project_context_with_config_async`：
  - 指定路径存在且非空，返回内容；
  - 超长内容保持截断规则（含 `truncated` 尾标记）。
- `WorkflowStagePrompts::load_from_file_async`：
  - 多阶段 markdown（含 fenced code block）能正确提取各阶段模板。

2. 边界条件
- 文件不存在、空文件、仅空白文件：返回 `None` 或空集合，行为与当前一致。
- 多文件 developer prompt：按配置顺序拼接，分隔符 `\n\n---\n\n` 不变。

3. 异常路径
- 读取失败（权限、编码异常）：
  - developer prompt：记录 `warn` 并继续；
  - workflow stage：返回 `Err` 并带上下文。

4. 兼容回归
- 同步 API 现有单测保持通过，新增异步单测覆盖等价行为，确保迁移不破坏外部调用。
