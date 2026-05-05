# Plan 1: 协议与数据模型

## 前置依赖
无

## 本次目标
1. 为“会话文件树”定义统一后端响应结构与 gateway 消息。
2. 明确该接口与 `session.runtime.project_dir`、`@relative/path` 语义之间的关系。
3. 让前后端共享类型能够表达“无项目目录”“越界”“目录不存在”等状态。

## 涉及文件
- `crates/nova-protocol/src/observability.rs`
- `crates/nova-protocol/src/envelope.rs`
- `crates/nova-protocol/src/schema.rs`
- `deskapp/scripts/generate-schemas.js`
- `deskapp/src/generated/*`（由生成流程产出）
- 如需补前端手写类型桥接：`deskapp/src/core/types.ts`

## 详细设计

### 1. 新增消息类型
建议新增会话级消息：

- `session.file_tree.list`
- `session.file_tree.list.response`
- 可选后续事件：`session.file_tree.invalidated`

请求结构：

- `session_id: String`
- `relative_path: Option<String>`

响应结构：

- `entries: Vec<SessionFileTreeEntry>`
- `base_relative_path: String`
- `project_dir_present: bool`
- `updated_at: i64`

其中 `base_relative_path` 表示当前返回的是哪一级目录列表，根目录时为空字符串。

### 2. 目录项结构
`SessionFileTreeEntry` 建议字段：

- `name: String`
- `relative_path: String`
- `is_dir: bool`

预留可选字段：

- `size: Option<u64>`
- `modified_at: Option<i64>`
- `has_children_hint: Option<bool>`

本期前端 UI 只强依赖前三个字段，其余字段用于未来丰富展示。

### 3. 错误语义
不要把所有失败都压成通用 `internal_error`。建议区分：

- `session_project_dir_not_set`
- `session_file_tree_path_not_found`
- `session_file_tree_access_denied`
- `session_file_tree_invalid_path`

这些错误可以继续通过现有 error envelope 返回，但要在 `code` 或 message 中保持稳定文案，便于前端分别展示：

- “当前会话未设置项目目录”
- “目录不存在”
- “路径越界”

### 4. `SessionRuntimeSnapshot` 与文件树接口的关系
`SessionRuntimeSnapshot.project_dir` 应继续存在，但只用于：

- 显示当前会话工作根目录
- 作为调试信息
- 作为缓存失效比较依据

不再作为前端自行枚举目录的前置输入。

### 5. 路径格式规范
协议层统一要求：

- `relative_path` 永远使用 `/`
- 根目录使用 `""`
- 不允许前缀 `@`
- 不允许绝对路径

前端插入输入框时自行转成：

- `@${relative_path}`

这样协议本身保持纯文件树语义，不混入 prompt 引用语法。

## 测试案例
1. 正常路径：根目录请求返回 `entries` 与空 `base_relative_path`。
2. 正常路径：子目录请求返回对应相对路径列表，响应中的 `relative_path` 仍相对 session 根目录。
3. 边界路径：session 没有 `project_dir`，返回稳定错误码 `session_project_dir_not_set`。
4. 边界路径：传入 `../x`、绝对路径、空白非法片段时，返回 `session_file_tree_invalid_path`。
5. 兼容路径：schema 导出后，DeskApp 生成类型包含新消息，旧消息不回归。
