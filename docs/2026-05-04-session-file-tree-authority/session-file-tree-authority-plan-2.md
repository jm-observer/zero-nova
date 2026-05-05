# Plan 2: 后端会话文件树服务

## 前置依赖
Plan 1

## 本次目标
1. 在 `nova-agent` 应用层增加基于当前 session `project_dir` 的目录枚举服务。
2. 让 gateway 可通过标准消息转发该能力给 DeskApp。
3. 统一路径校验、目录排序和错误分类，避免与 `Read` / `Write` / `Edit` 出现分叉语义。

## 涉及文件
- `crates/nova-agent/src/app/agent_workspace_service.rs`
- `crates/nova-agent/src/app/application.rs`
- `crates/nova-agent/src/path_resolver.rs`
- `crates/nova-gateway-core/src/router.rs`
- `crates/nova-gateway-core/src/handlers/sessions.rs`
- `crates/nova-agent/tests/integration/` 下新增或扩展测试

## 详细设计

### 1. 应用层新增会话文件树接口
在 `AgentApplication` / `AgentWorkspaceService` 中新增：

- `list_session_file_tree(session_id, relative_path)`

职责：

1. 读取 session 当前 `project_dir`
2. 校验 `relative_path`
3. 在 project root 内部枚举目录项
4. 按统一规则排序并序列化为协议结构

### 2. 安全边界复用现有路径解析能力
建议不要在文件树接口里重新手写 `..` / 绝对路径检查，而是复用 `path_resolver` 中已有规则，或抽取公共辅助函数：

- 输入：`relative_path`
- 根：session `project_dir`
- 行为：只允许目录路径，且必须位于 root 内

与工具层一致的原因：

- 用户在 `@` 列表里看到的路径，随后极大概率会进入 `Read` / `Edit` / `Write`
- 若 UI 可见但工具不可读，会造成严重行为偏差

### 3. 枚举实现策略
后端枚举只做“单层目录 listing”，不做递归树展开：

- 根请求：列出 project root 下单层目录项
- 子目录请求：列出 `relative_path` 对应目录下单层目录项

排序规则沿用现有 Tauri 逻辑：

1. 目录优先
2. 名称忽略大小写排序
3. 同名时按原始名称排序

这能降低前后端 UI 切换带来的行为变化。

### 4. 错误处理原则
以下情况返回业务错误而不是空数组：

- session 不存在
- session 没有 `project_dir`
- `relative_path` 非法
- 请求目标不存在
- 请求目标不是目录
- 路径越界

只有“目录合法但当前层无子项”时，才返回空数组。

### 5. gateway 路由接入
在 `nova-gateway-core` 的 session handler 中增加：

- `handle_session_file_tree_list`

职责只做：

- 解析 payload
- 调用 `app.list_session_file_tree(...)`
- 返回 response envelope

不在 gateway 层重复目录校验逻辑。

## 测试案例
1. 正常路径：session 已设置 `project_dir`，请求根目录返回目录优先排序列表。
2. 正常路径：请求子目录 `src/ui`，返回该目录子项，`relative_path` 仍相对根目录。
3. 错误路径：session 未设置 `project_dir`，返回明确错误。
4. 错误路径：请求文件路径而不是目录，返回“目标不是目录”。
5. 错误路径：请求 `../secret` 或绝对路径，返回越界/非法路径错误。
6. 一致性路径：同一个 `@src/lib.rs` 在文件树可见后，工具层 `Read` 仍能以同样根目录解析成功。
