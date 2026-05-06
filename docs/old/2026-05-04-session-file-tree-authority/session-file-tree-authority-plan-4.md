# Plan 4: 兼容策略、测试与后续扩展

## 前置依赖
Plan 1、Plan 2、Plan 3

## 本次目标
1. 明确“会话文件树”和“本地文件命令”的边界，避免未来回归到双权威源。
2. 补充跨层测试、E2E 约束与回归清单。
3. 为后续远程文件预览 / 打开能力定义可扩展方向，但不在本期实现。

## 涉及文件
- `deskapp/e2e/tests/` 下与聊天输入区相关测试
- `deskapp/src-tauri/src/commands/file.rs`
- `deskapp/src/ui/modals.ts`
- `deskapp/src/preview.ts`
- `docs/2026-04-29-at-project-dir-picker/*`（后续如需追加实现说明，可引用）

## 详细设计

### 1. 兼容边界声明
本期完成后，系统内文件能力分成三层：

1. 会话文件树
   - 数据源：后端 session
   - 用途：`@` 路径选择
2. 工具文件访问
   - 数据源：后端 session + tool path resolver
   - 用途：`Read` / `Write` / `Edit`
3. 本地桌面文件命令
   - 数据源：DeskApp 所在机器
   - 用途：本地预览、系统打开、本地 artifact 交互

任何新功能都必须先判断自己属于哪一层，不能再拿 Tauri 文件命令替代会话级文件树。

### 2. `remote` 模式下的行为说明
在 `remote` 模式中：

- `@` 选择器应该可用
- 目录浏览应该正确
- 相对路径插入应该正确
- 但“本地直接打开远端文件”不保证可用

这不是缺陷，而是明确的能力边界。UI 如需远程预览，应通过后端新增会话文件读取接口解决。

### 3. 后续扩展建议
后续若要支持远程预览，可新增：

- `session.file.read`
  - 输入：`session_id + relative_path`
  - 输出：文本内容 / 二进制元数据 / mime 类型

甚至进一步支持：

- `session.file.stat`
- `session.file.open_link`（若映射为下载 URL）

但这些能力应独立设计，避免在本次 `@` 迁移中混入。

### 4. 回归测试要求
除了单测，还应覆盖：

- gateway 合约测试：新消息的 schema / envelope
- 集成测试：session 切换目录后，文件树返回值随之更新
- 前端交互测试：`@` 输入、下钻、过滤、插入
- 远程模式回归：即使 Tauri 本地目录命令不可用，`@` 选择器仍可工作

### 5. 旧设计文档的后续处理
`docs/2026-04-29-at-project-dir-picker/` 中“project dir 来源建议优先取 sidecar workspace”在新设计下已过期。

建议在实施完成后：

- 保留旧文档历史记录
- 在实现 PR 或后续 review 文档中明确标注：`@` 选择器的权威根目录已经迁移为后端 session file tree

## 测试案例
1. 正常路径：`embedded` 模式下，`@` 选择器与工具层读取相同根目录，切换 project 后同步更新。
2. 正常路径：`remote` 模式下，本地不具备远端路径，`@` 选择器仍可展示目录树。
3. 回归路径：禁用或绕过 Tauri `project_dir_list` 后，`@` 选择器功能不受影响。
4. 错误路径：后端 session 目录失效时，前端显示错误态，而不是展示旧缓存或本地目录。
5. 扩展路径：将来新增 `session.file.read` 时，不需要再次改动 `@` 目录浏览协议。
