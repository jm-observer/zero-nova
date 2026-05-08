# Plan 1: 配置模型扩展

## 前置依赖

无

## 本次目标

在不影响现有提示词链路的前提下，补齐“开发项目提示词”所需的配置模型：

1. 新增独立顶层文件列表配置
2. 在 agent 配置中新增是否启用该能力的开关
3. 明确默认值、相对路径语义和校验规则

## 涉及文件

1. `crates/nova-agent/src/config.rs`
2. `.nova/config.toml`
3. 相关配置解析与校验测试文件

## 详细设计

### 1. 顶层新增文件列表配置

在 `AppConfig` 增加独立字段：

```rust
pub developer_prompt_files: Vec<String>
```

该字段不放入 `ToolConfig`，理由如下：

1. 该配置描述的是系统提示词来源，而不是工具行为
2. 与 `prompts_dir` 或 `project_context_file` 混在 `tool` 下会弱化语义边界
3. 顶层字段更利于后续扩展其他全局提示词源

### 2. Agent 新增开关

在 `AgentSpec` 中增加字段：

```rust
pub enable_project_developer_prompt: bool
```

默认值建议为 `false`，这样不会改变现有 agent 行为。只有显式开启的 agent 才会读取项目根目录中的开发提示词文件。

### 3. 路径解析语义

`developer_prompt_files` 中的每个元素都按“相对于 `project_dir` 根目录”的路径解释：

1. `AGENTS.md` => `<project_dir>/AGENTS.md`
2. `.nova/developer.md` => `<project_dir>/.nova/developer.md`

本设计不支持基于 `config_dir` 解析，也不支持多根目录。

### 4. 校验规则

建议新增以下校验：

1. `developer_prompt_files` 允许为空
2. 非空项不能是全空白字符串
3. 路径中不做文件存在性校验，避免配置加载依赖项目目录实时状态
4. `enable_project_developer_prompt` 仅作为布尔开关，不增加额外互斥约束

### 5. 配置示例

```toml
developer_prompt_files = [
  "AGENTS.md",
  ".nova/developer-prompt.md",
]

[[gateway.agents]]
id = "developer"
display_name = "Developer"
description = "开发任务子代理"
provider = "local"
llm = "local_default"
enable_project_developer_prompt = true
```

## 测试案例

1. 未配置 `developer_prompt_files` 时可成功加载，默认为空列表
2. `developer_prompt_files` 配置多个路径时可成功反序列化
3. `developer_prompt_files` 含空白字符串时校验失败
4. `enable_project_developer_prompt` 未设置时默认为 `false`
5. `enable_project_developer_prompt = true` 时可正确反序列化到 `AgentSpec`
