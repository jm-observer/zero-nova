# Plan 1: Tauri 目录枚举命令与安全边界

## Plan 编号与标题
Plan 1: Tauri 目录枚举命令与安全边界

## 前置依赖
无

## 本次目标
- 提供前端可调用的目录枚举命令，输入一个相对路径，返回该路径下的文件/目录列表。
- 明确 project dir 根路径计算策略。
- 建立路径校验，确保无法访问 project dir 之外的内容。

## 涉及文件
- `deskapp/src-tauri/src/commands/file.rs`（新增目录枚举命令与数据结构）
- `deskapp/src-tauri/src/lib.rs`（注册新 tauri invoke handler）

## 详细设计
### 1. 命令定义
新增命令（命名建议）：
- `project_dir_list(relativePath?: string)`

返回结构：
- `name: string`：条目名
- `relativePath: string`：相对 project dir 的路径（统一 `/`）
- `isDir: boolean`：是否目录

### 2. project dir 根路径策略
优先级建议：
1. 若 sidecar 配置中存在 workspace，且尾段为 `.nova`，则 project dir = workspace 的父目录。
2. 若 sidecar workspace 存在但不以 `.nova` 结尾，则 project dir = workspace。
3. 回退到当前 config 目录对应的 project 推导路径（与上面规则一致）。

说明：该策略与现有 DeskApp 启动参数和配置结构兼容，避免引入新配置项。

### 3. 路径安全策略
- `relativePath` 必须是相对路径，拒绝绝对路径。
- 拒绝含 `..` 的路径片段，防止目录穿越。
- 目标路径 canonicalize 后必须以 project dir canonicalize 结果为前缀。
- 非目录路径调用时返回明确错误（便于前端兜底提示）。

### 4. 输出排序策略
- 目录优先，文件其次。
- 同类型按 `name` 不区分大小写排序。

### 5. 错误与日志策略
- 错误通过 `Result<_, String>` 返回到前端，消息使用可读文本。
- 不在多层重复打日志，由调用侧决定是否提示 UI。

## 测试案例
### 正常路径
1. `relativePath` 为空，返回 project dir 根目录条目。
2. `relativePath = src`，返回 `src` 下条目且 `isDir` 正确。

### 边界条件
1. 空目录返回空数组。
2. 含中英文、空格、特殊字符文件名时返回正常。
3. 大目录（>1000 项）响应可用且排序正确。

### 异常路径
1. `relativePath` 为绝对路径，返回错误。
2. `relativePath` 包含 `..`，返回错误。
3. 目录不存在、无权限、路径不是目录时返回错误。
