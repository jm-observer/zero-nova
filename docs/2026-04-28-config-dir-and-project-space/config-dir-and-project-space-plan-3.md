# Plan 3: `@` 路径解析器与调用链接入

## 前置依赖
- Plan 1
- Plan 2

## 本次目标
- 建立统一的 `@` 路径解析逻辑，支持文件和目录。
- 将解析器接入读取、检索、上下文注入等调用链。
- 在非法输入时提供可解释错误。

## 涉及文件
- `crates/nova-agent/src/tool/builtin/read.rs`
- `crates/nova-agent/src/tool/builtin/write.rs`
- `crates/nova-agent/src/tool/builtin/edit.rs`
- `crates/nova-agent/src/prompt.rs`
- `crates/nova-agent/src/app/conversation_service.rs`
- `crates/nova-agent/src/path_resolver.rs`（新增）

## 详细设计
- 解析器职责边界：
  - 输入：`raw_ref`（如 `@src/lib.rs`、`@D:/repo`）、`project_dir`、`config_dir`。
  - 输出：标准化 `ResolvedPathRef`：
    - `target_path: PathBuf`
    - `is_dir: bool`
    - `origin: RelativeToProject | Absolute`
  - 注意：`project_dir` 始终有值（默认 cwd），不再需要 `cwd` fallback 逻辑。
- 规则定义：
  - `@绝对路径`：直接解析。
  - `@相对路径`：始终相对 `project_dir` 解析（project_dir 永不为空，无需特殊分支）。
  - `@.`、`@..`、路径穿越需安全检查，禁止越界到未授权根（如工具层有 root_dir 限制则复用）。
- 调用链接入：
  - 在进入 tool 前统一解析，避免每个工具重复实现。
  - prompt 上下文装配中若检测到 `@` 引用，也走同一解析器，确保语义一致。
  - `PromptConfig.workspace_path` 重命名为 `project_dir`：
    - 用于加载项目上下文（PROJECT.md/NOVA.md），从 `project_dir` 目录加载。
    - 这与 Plan 2 的 `EnvironmentSnapshot` 改造配合：Git 信息和项目上下文均来自 `project_dir`。
- 错误模型：
  - `PathNotFound`
  - `PathAccessDenied`
  - `InvalidPathSyntax`
  - 错误信息要包含原始输入与建议修复动作。
  - 注意：不再需要 `ProjectSpaceRequired` 错误，因为 `project_dir` 始终有值。

## 测试案例
- 正常路径：
  - `@relative/file` 相对 project_dir 成功解析。
  - `@absolute/path` 成功解析。
- 边界路径：
  - `@目录`、`@文件` 均支持。
  - 含空格、中文、Windows 盘符路径。
- 异常路径：
  - 路径不存在、越界访问、语法非法分别返回对应错误。
- 一致性路径：
  - read/write/edit 对同一输入解析结果一致。
