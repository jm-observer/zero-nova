# Plan 2: 会话态 project_dir 设计与运行中切换机制

## 前置依赖
- Plan 1

## 本次目标
- 增加 `project_dir: PathBuf` 会话状态（**非可选，默认值为进程 cwd**）。
- 支持用户在对话过程中动态设置、变更、重置 `project_dir`。
- 明确 project_dir 的读取优先级与生命周期。
- 将 `project_dir` 注入系统提示词 Environment 部分。

## 涉及文件
- `crates/nova-agent/src/conversation/model.rs`
- `crates/nova-agent/src/conversation/service.rs`
- `crates/nova-agent/src/app/conversation_service.rs`
- `crates/nova-agent/src/app/types.rs`
- `crates/nova-agent/src/app/application.rs`
- `crates/nova-agent/src/prompt.rs`（`EnvironmentSnapshot` 扩展）
- `crates/nova-agent/src/agent.rs`（提示词重建触发）

## 详细设计
- 状态建模：
  - 在会话实体（session/conversation metadata）增加字段：`project_dir: PathBuf`。
  - 默认值为进程 `cwd`（`std::env::current_dir()`）。
  - 该字段与消息历史同生命周期持久化（若当前 session 已落库，则同步入库；若暂未落库，则先内存态）。
- 赋值来源与优先级：

  | 优先级 | 来源 | 说明 |
  |--------|------|------|
  | 1（最高） | 用户运行中 `set_project_dir(path)` | 显式切换 |
  | 2 | CLI 启动参数 / 配置 | `--project-dir` 或 config.toml |
  | 3（默认） | 进程 `cwd` | 兜底，永不为空 |

- 运行中切换接口：
  - 建议统一定义应用层命令（非硬编码提示词）：
    - `set_project_dir(path)` — 设置为指定路径
    - `reset_project_dir()` — 重置回进程 cwd 默认值（替代原来的 `clear`，语义更准确）
    - `get_project_dir()` — 查询当前值
  - 命令执行需做路径规范化（`canonicalize` 失败时保留原路径但标记 warning）。
- 并发与一致性：
  - 会话态更新通过会话级锁保护，保证一次更新原子可见。
  - 读取时优先读取最新 session snapshot，避免跨请求读到陈旧值。
- 可观测性：
  - 更新 project_dir 时记录 `info!`，包含会话 ID 与新值。
  - 避免在低层重复日志，只在状态变更边界打点。
- **提示词 Environment 注入**：
  - `EnvironmentSnapshot` 新增字段 `project_dir: String`（非 Option，始终有值）。
  - `EnvironmentSnapshot::collect()` 改为接受 `config_dir: &Path, project_dir: &Path` 两个参数。
  - Git 信息（`git_branch`、`git_status_summary`、`recent_commits`）始终在 `project_dir` 目录下采集。这样 LLM 看到的 Git 上下文与用户实际项目一致。
  - `to_prompt_text()` 输出格式：
    ```
    Config directory: D:\git\zero-nova\.nova
    Project directory: D:\projects\my-app
    Platform: windows
    Shell: C:\WINDOWS\system32\cmd.exe
    Date: 2026-04-28
    Git branch: feature/login                  ← 取自 project_dir
    Git status: 3 changed files                ← 取自 project_dir
    ```
  - **切换时重建提示词**：`project_dir` 变更时需触发 `EnvironmentSnapshot` 重新采集并重建系统提示词。在 `set_project_dir()` 命令执行后，标记当前会话的 prompt 缓存为 dirty，下一轮对话前重建。

## 测试案例
- 正常路径：新建会话时 project_dir 默认为进程 cwd。
- 设置路径：设置 project_dir 后，后续请求读取到新值。
- 覆盖路径：多次切换 project_dir，始终以最后一次为准。
- 重置路径：执行 reset 后，project_dir 恢复为进程 cwd。
- 并发路径：并发 set 与读请求下无 panic、无部分写入。
- 异常路径：设置不可访问路径时返回明确错误或 warning（按策略约定）。
- **提示词注入路径**：
  - 默认状态下 `to_prompt_text()` 输出中 `Project directory:` 为 cwd。
  - 设置 project_dir 后，`Project directory:` 更新为新值。
  - Git 信息始终来自 project_dir 目录。
  - 切换 project_dir 后下一轮对话的系统提示词包含更新后的环境信息。
