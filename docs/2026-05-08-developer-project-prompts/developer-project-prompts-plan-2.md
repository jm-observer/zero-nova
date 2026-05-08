# Plan 2: 提示词加载与拼装

## 前置依赖

Plan 1

## 本次目标

实现“开发项目提示词”从项目根目录读取、命中内容合并，并作为独立 section 追加到系统提示词中的能力。

## 涉及文件

1. `crates/nova-agent/src/prompt.rs`
2. 必要时新增提示词加载辅助模块
3. 提示词构建相关测试文件

## 详细设计

### 1. 新增加载结果字段

在 `PromptConfig` 中增加：

```rust
pub developer_project_prompt_content: Option<String>
```

该字段只承载已合并完成的最终文本，不把目录扫描状态泄漏到 builder 内部。

### 2. 新增加载函数

建议增加独立加载函数，职责只做项目根文件读取与合并，例如：

```rust
async fn load_developer_project_prompt_async(
    project_dir: Option<&Path>,
    files: &[String],
) -> Option<String>
```

处理规则：

1. `project_dir` 为空则直接返回 `None`
2. 按 `files` 顺序逐个检查 `<project_dir>/<file>`
3. 文件不存在则跳过
4. 文件存在但内容为空白则跳过
5. 文件读取失败则记录 `warn!` 并继续
6. 命中多个文件时按顺序拼接

### 3. 合并格式

建议在合并内容中保留来源标识，形成稳定文本：

```text
### Source: AGENTS.md
<file content>

---

### Source: .nova/developer-prompt.md
<file content>
```

这样做的目的不是让模型理解“优先级”，而是方便人类排查 prompt 来源。

### 4. 新增独立 section

在 `SectionName` 中增加一项：

```rust
DeveloperProjectPrompt
```

对应标题建议为：

```text
Developer Project Instructions
```

并在 `SystemPromptBuilder` 中新增便捷方法，例如：

```rust
pub fn developer_project_prompt_section(self, content: impl Into<String>) -> Self
```

### 5. 拼装顺序

`SystemPromptBuilder::from_config` 中新增如下顺序：

1. Base
2. BehaviorGuards
3. Skill
4. DeveloperProjectPrompt
5. ProjectContext
6. Environment
7. Workflow

这样能够保证开发项目提示词在项目上下文之前出现，但不会压过基础身份与行为约束。

### 6. 与现有 `project_context` 的关系

本设计不替换也不复用 `project_context` section，原因：

1. `project_context` 表示项目说明文档
2. `developer_project_prompt` 表示开发人员附加指令
3. 两者语义不同，混在同一 section 会削弱后续调试能力

## 测试案例

1. `project_dir` 为空时不加载任何开发项目提示词
2. 单个命中文件可被完整读取
3. 多个命中文件按配置顺序合并
4. 空文件会被跳过
5. 不存在文件不会报错
6. 读取失败的单个文件不会阻断其他文件合并
7. `from_config` 在存在内容时会输出 `Developer Project Instructions` section
