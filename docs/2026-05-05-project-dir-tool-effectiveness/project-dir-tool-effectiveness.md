# ProjectManager 工具目录切换有效性修复

## 时间

2026-05-05

## 项目现状

当前 `ProjectManager` 工具存在功能性缺陷：

1. **BashTool 不使用 session 的 project_dir**：`BashTool` 构造时 `workspace = None`，执行命令时不从 `ToolContext.environment.project_dir` 获取 CWD，始终使用 gateway 进程的 CWD。
2. **同 turn 内 EnvironmentSnapshot 不刷新**：`EnvironmentSnapshot` 在每个 turn 开头收集一次。如果 LLM 在同一个 turn 内先调用 `ProjectManager` 修改了 `project_dir`，再调用 `Bash`/`Read`/`Write`/`Edit`，后者仍使用旧的 snapshot。
3. **工具返回信息具有误导性**：`ProjectManager` 返回 `"Project directory updated to: ..."` 让 LLM 和用户认为切换已完全生效，但实际上 Bash 命令仍在旧目录执行。

### 现象复现

```
Turn N:
  1. LLM 调用 ProjectManager(action="set", path="D:\git\streaming-speech")
  2. 工具返回 "Project directory updated to: \\?\D:\git\streaming-speech" ← 看似成功
  3. LLM 调用 Bash(command="ls docs/...")
  4. 命令在 D:\git\zero-nova\docs\... 执行 ← 实际未切换
```

### 影响的工具

| 工具 | project_dir 使用情况 | 问题 |
|------|---------------------|------|
| Read/Write/Edit | 在 `preprocess_file_tool_input()` 中使用 `env.project_dir` 解析相对路径 | 同 turn 内不刷新 |
| Bash | **完全不使用** `project_dir`，`workspace` 字段始终为 None | 根本性缺失 |
| ProjectManager | 正确更新数据库 | 返回信息误导 |

## 整体目标

1. **Bash 工具使用 session project_dir 作为命令执行的工作目录**
2. **同 turn 内 ProjectManager 修改 project_dir 后，后续工具调用能感知最新值**
3. **ProjectManager 返回信息更准确，反映实际影响范围**

## Plan 拆分

| Plan | 标题 | 简要描述 | 前置依赖 |
|------|------|---------|---------|
| Plan 1 | BashTool 读取 project_dir 作为 CWD | 修改 BashTool 执行时从 ToolContext.environment.project_dir 设置 current_dir | 无 |
| Plan 2 | 同 turn 内 EnvironmentSnapshot 实时刷新 | ProjectManager 修改 project_dir 后触发 snapshot 局部刷新，使后续工具获取最新值 | Plan 1 |
| Plan 3 | ProjectManager 返回信息增强 | 返回更精确的状态描述，说明变更范围和生效时机 | Plan 2 |

### 执行顺序

```
Plan 1 ──→ Plan 2 ──→ Plan 3
```

Plan 1 可独立实施，使 Bash 工具在新 turn 中能正确使用 project_dir。Plan 2 解决同 turn 内的及时性问题。Plan 3 属于体验优化。

## 风险与待定项

- **安全边界**：Bash 命令的 CWD 设为 project_dir 后，是否需要像 Read/Write/Edit 一样做路径越界检查？当前评估不需要，Bash 本身就能执行任意命令。
- **并发工具调用**：LLM 可能在同一 turn 内并发调用多个工具（`FuturesUnordered`），如果 ProjectManager 和 Bash 同时执行，刷新 snapshot 的时序需要考虑。Plan 2 将给出解决方案。
- **向后兼容**：`BashTool::with_workspace()` 已存在但未使用，新方案是否需要保留该 API。
