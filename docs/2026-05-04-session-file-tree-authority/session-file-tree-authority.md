# 会话文件树权威源设计

| 章节 | 说明 |
|-----------|------|
| 时间 | 创建：2026-05-04<br>最后更新：2026-05-04 |
| 项目现状 | 1. `deskapp/src/ui/chat-view.ts` 的 `@` 选择器当前通过 Tauri `project_dir_list` 直接读取桌面端本地文件系统。<br>2. 该能力最初以 sidecar workspace 为根目录，后续临时修正为“从当前会话 prompt preview 里解析 `Project directory:` 后，再让 Tauri 读本地目录”。<br>3. `nova-agent` 会话层已经有 `project_dir`、`ProjectManagerTool`、相对路径解析与 prompt 环境一致性能力，但尚未提供“按当前会话目录列出文件树”的后端 API。<br>4. 当前架构同时支持 `embedded` / `remote` 两种 gateway 模式；当 DeskApp 与 gateway 不在同一台设备时，前端本地文件系统与后端会话 `project_dir` 不再天然一致。 |
| 整体目标 | 1. 将 `@` 选择器的权威数据源更正为“后端当前会话文件树”，不再依赖 Tauri 本地目录推断。<br>2. 保证 `@` 选择器看到的目录、Agent 工具实际解析的目录、prompt 中显示的 `project_dir` 三者完全一致。<br>3. 在 `remote` / 异机构型下仍然正确工作：前端只展示后端返回的相对路径树，不假设本地能访问后端路径。<br>4. 为后续远程文件预览 / 打开能力预留清晰协议边界。 |
| 非目标 | 1. 本次不实现完整远程文件预览协议。<br>2. 本次不把聊天附件、Artifact、`file_open`、`file_read` 等所有桌面文件能力一并迁移到后端。<br>3. 本次不引入多项目工作区管理，也不改变 `ProjectManagerTool` 的会话级目录语义。 |

## Plan 拆分

| 状态 | Plan | 说明 | 依赖 | 执行顺序 |
|------|------|------|------|----------|
| 待开始 | Plan 1: 协议与数据模型 | 为“会话文件树”定义后端响应结构、gateway 消息、前端共享类型，并明确与现有 `session.runtime` / `project_dir` 的关系。 | 无 | 1 |
| 待开始 | Plan 2: 后端会话文件树服务 | 在 `nova-agent` / `nova-gateway-core` 增加基于当前 session `project_dir` 的目录枚举 API，统一安全边界和错误语义。 | Plan 1 | 2 |
| 已完成 | Plan 3: DeskApp `@` 选择器迁移 | 前端彻底改为调用后端会话文件树接口，移除 `@` 选择器对 Tauri `project_dir_list` 的依赖，并处理实时更新、缓存和 UI 状态。 | Plan 1, Plan 2 | 3 |
| 已完成 | Plan 4: 兼容策略、测试与后续扩展 | 明确本地/远程边界、保留 Tauri 文件命令的适用范围，补充端到端测试、回归清单和后续远程预览扩展点。 | Plan 1, Plan 2, Plan 3 | 4 |

## 现状分析

### 1. 当前 `@` 选择器存在双重不一致
- 第一层不一致：UI 展示根目录可能来自 sidecar workspace，而工具实际解析根目录来自会话 `project_dir`。
- 第二层不一致：即使 UI 已读取 prompt preview 中的 `project_dir`，真正枚举目录的动作仍发生在桌面本地 Tauri 进程，而不是后端会话所在环境。

### 2. 异机构型下当前实现会系统性失真
- 当 gateway 运行在另一台机器、容器、WSL、远程主机或网络侧边车中时，后端返回的 `project_dir` 只是“远端路径字符串”。
- 桌面端即使拿到这个字符串，也未必能访问对应目录，更不能假设该路径在本机有同名副本。
- 因此“前端拿到 `project_dir` 后自己读本地目录”的做法，从设计上无法支持 `remote` 模式。

### 3. 项目中已经具备会话级文件边界的核心基础
- `SessionService::get_project_dir()` 已能稳定读取当前会话根目录。
- `ToolContext.environment.project_dir` 已参与 `Read` / `Write` / `Edit` 的相对路径解析。
- `ConversationService` 会在 turn 准备时按 session 重建 prompt 环境。
- 这意味着“以后端会话文件树为准”并不是新增概念，而是把已有会话边界能力补齐到 UI 文件浏览入口。

## 关键设计决策

### 1. `@` 选择器的唯一权威源是后端会话文件树
`@` 选择器后续只消费后端返回的数据：

- 根目录：当前 session 的 `project_dir`
- 子路径：由后端在该根目录下解析
- 排序与过滤：以后端返回相对路径列表为基础，前端仅做 UI 层轻量过滤

前端不再：

- 从 prompt 文本正则解析 `Project directory:` 作为目录枚举依据
- 调用 Tauri 本地 `project_dir_list` 作为 `@` 入口
- 使用 sidecar workspace 推断项目根目录

### 2. 目录树返回相对路径，而非绝对路径
后端响应结构以相对路径为核心：

- `name`
- `relative_path`
- `is_dir`

可选扩展：

- `parent_relative_path`
- `has_children_hint`
- `size`
- `modified_at`

原因：

- 绝对路径会泄露远端环境细节，对前端也缺少可移植性价值。
- `@` 插入本来就需要 `@relative/path` 语义。
- 相对路径天然适合工具层 `@path` 引用和跨设备显示。

### 3. 绝对路径只作为“会话状态信息”存在，不作为列表主数据
- `SessionRuntimeSnapshot` 中可继续暴露 `project_dir`，用于状态展示、调试和控制台。
- 但 `@` 列表接口不依赖前端先拿绝对路径再自行枚举。
- 文件树接口内部直接读取 session 的 `project_dir`，前端只传 `session_id` 与可选 `relative_path`。

### 4. Tauri 文件命令与会话文件树能力解耦
本次将两类能力严格分层：

1. `session file tree`
   - 权威源：后端 session
   - 用途：`@` 路径选择、会话级目录浏览
2. `desktop local file commands`
   - 权威源：DeskApp 本机
   - 用途：本地文件预览、系统打开、另存为、图片展示等

这样可以避免当前“用本地文件命令误承担远程会话浏览职责”的架构混淆。

### 5. 后端接口必须复用工具层相同的路径安全规则
会话文件树接口的安全边界必须与工具解析保持一致：

- 相对路径只能在当前 session `project_dir` 内部展开
- 禁止 `..`、绝对路径、路径逃逸
- 当 session 无 `project_dir` 时，返回明确错误，而不是静默回退到 workspace

推荐复用或抽取现有 `path_resolver` / `project_dir` 规范化逻辑，避免一套给工具、一套给 UI。

### 6. 前端缓存从“路径字符串缓存”改为“按 session + relative_path 的目录项缓存”
现有前端缓存聚焦于 `project_dir` 字符串。迁移后应改为：

- key: `session_id + relative_path`
- value: `SessionFileTreeEntry[]`

失效策略：

- 收到 `session.runtime.updated` 且 `project_dir` 变化时，清空该 session 的目录缓存
- 切换 session 时不复用其他 session 的缓存
- 目录下钻时仅缓存相对路径对应的列表，不缓存绝对路径推断结果

### 7. 对远程文件预览保持显式能力缺口
本次不假装远程文件预览已经完成：

- `@` 列表会正确显示远端会话文件树
- 选中文件后会插入 `@relative/path`
- 但若 UI 需要“点击预览真实内容”，仍需单独设计会话级文件内容接口

文档中必须明确这一点，避免后续实现者误以为“列表改后端”就天然解决了预览问题。

## 目标数据流

### 1. `@` 目录浏览
1. 用户在输入框输入 `@`
2. DeskApp 根据当前 `session_id` 请求 `session.file_tree.list`
3. gateway 转发到应用层
4. 应用层读取 session 当前 `project_dir`
5. 后端在 `project_dir` 内枚举目标目录并返回相对路径列表
6. 前端渲染结果；继续输入关键词时仅在当前列表上过滤
7. 若用户进入子目录，则继续请求同一接口并传 `relative_path`

### 2. 项目目录切换联动
1. Agent 通过 `ProjectManagerTool` 修改当前 session `project_dir`
2. 后端会话状态更新，`session.runtime.updated` 广播新 `project_dir`
3. DeskApp 收到后清理该 session 目录缓存
4. 用户再次输入 `@` 时，从后端新目录重新拉取根列表

## 风险与待定项

### 已知风险
- 远端大目录递归浏览可能带来明显延迟，需要在接口层保留分页 / 限流扩展位。
- 若前端过滤逻辑和后端排序逻辑不稳定，UI 下钻体验会出现跳动。
- 如果继续保留旧的 `project_dir_list` 路径给 `@` 选择器兜底，会让 bug 在某些模式下隐性回归。

### 待定项
- 是否在本期引入 `session.file_tree.read`，用于异机远程预览文本文件。建议不放入本次实现，但在 Plan 4 中预留协议位。
- 是否对目录项返回 Git 忽略/隐藏文件提示。建议本期不做，先保证权威源切换正确。
