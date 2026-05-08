# 开发项目提示词加载设计

- **时间**：2026-05-08
- **状态**：Plan 1 / Plan 2 / Plan 3 待开始

---

## 项目现状

当前 `nova-agent` 的系统提示词主要由以下几部分组成：

1. `prompts/agent-*.md` 提供 agent 基础身份提示词
2. `SkillRegistry` 生成可用 skill 提示词
3. `PROJECT.md` / `NOVA.md` 作为项目上下文文件注入
4. `EnvironmentSnapshot` 与 `workflow-stages.md` 提供环境和工作流补充信息

现有链路已经支持“读取单个项目上下文文件并拼接到系统提示词”，但还不支持：

1. 从开发项目根目录读取多个指定配置文件
2. 将多个命中文件内容合并后作为独立提示词来源
3. 由 agent 自身配置决定是否启用这类开发项目提示词

---

## 整体目标

在不引入复杂扫描规则、不增加模式切换和长度裁剪配置的前提下，新增“开发项目提示词”能力：

1. 仅扫描当前项目根目录
2. 使用独立顶层配置项声明候选文件列表，不挂在 `tool` 配置下
3. 若命中多个文件，则按稳定顺序读取并合并内容
4. 合并结果整体以 append 方式注入系统提示词
5. 在 `agent` 配置中增加开关，按 agent 决定是否启用该能力

---

## 核心约束

本次设计明确采用以下固定约束，不再提供可选模式：

1. 不做长度上限限制
2. 不提供 `append/replace` 等模式配置，统一使用 append
3. 不做目录递归扫描，也不沿祖先目录向上查找
4. 文件内容不做 section 提取，整文件内容直接参与合并

---

## 设计结论

### 1. 使用独立顶层配置项声明文件列表

建议在 `AppConfig` 顶层新增独立配置项，例如：

```toml
developer_prompt_files = [
  "AGENTS.md",
  "DEVELOPER.md",
  ".nova/developer-prompt.md",
]
```

设计理由：

1. 该能力不属于工具配置，而是提示词来源配置
2. 顶层配置语义更直接，避免和 `tool.prompts_dir`、`tool.project_context_file` 混杂
3. 后续若扩展为更多提示词源，也更容易继续在顶层演进

### 2. 在 `AgentSpec` 增加显式开关

建议在 `[[gateway.agents]]` 下新增布尔字段，例如：

```toml
[[gateway.agents]]
id = "developer"
display_name = "Developer"
enable_project_developer_prompt = true
```

语义：

1. `true`：允许该 agent 读取开发项目提示词文件
2. `false`：该 agent 不读取这些文件，即使项目根存在命中文件也不注入

设计理由：

1. 不是所有 agent 都需要开发项目提示词
2. 该开关属于 agent 行为策略，应跟随 agent 配置，而不是全局硬编码
3. 这也为后续区分 `nova` / `developer` / `reviewer` 等 agent 提供边界

### 3. 仅扫描项目根目录

扫描范围固定为当前 session 的 `project_dir` 根目录，不做扩展：

1. 若 session 未设置 `project_dir`，则不读取开发项目提示词
2. 若设置了 `project_dir`，仅检查 `<project_dir>/<configured_file>`
3. 不递归子目录
4. 不扫描父目录

设计理由：

1. 规则简单，可预期
2. 避免大型仓库递归扫描带来的性能和噪音问题
3. 与“开发项目”概念一致，提示词来源明确归属当前项目根

### 4. 多文件命中后直接合并

若 `developer_prompt_files` 中多个文件均存在，则：

1. 按配置列表顺序逐个读取
2. 跳过不存在文件和空文件
3. 使用稳定分隔符将内容拼接为一个整体字符串
4. 该整体字符串作为单一 section 注入系统提示词

建议分隔方式：

```text
### Source: AGENTS.md
<content>

---

### Source: .nova/developer-prompt.md
<content>
```

设计理由：

1. 保留来源信息，便于调试和提示词排查
2. 作为一个整体 section 注入，比为每个文件单独建 section 更简单
3. 配置顺序可直接表达优先阅读顺序

### 5. 作为独立提示词 section 追加注入

建议在 `SystemPromptBuilder` 中新增独立 section，例如：

1. `SectionName::DeveloperProjectPrompt`
2. 标题为 `Developer Project Instructions`

拼装顺序建议为：

1. Base
2. BehaviorGuards
3. Skill
4. DeveloperProjectPrompt
5. ProjectContext
6. Environment
7. Workflow

设计理由：

1. 它是项目级开发约束，优先级应高于普通项目上下文
2. 但它不应覆盖基础身份和硬性行为约束
3. 独立 section 比混入 `Project Context` 更利于后续观测和调试

---

## 运行时链路

### 1. 启动期

`build_application` 在构造 `AgentDescriptor` 时，只保存：

1. 全局 `developer_prompt_files`
2. 各 agent 的 `enable_project_developer_prompt`

启动期不应主动读取项目根文件，因为此时没有可靠的 session 级 `project_dir`。

### 2. 会话执行期

`ConversationService` 在每轮开始前已经能拿到 session 的 `project_dir`。新增逻辑应放在这里：

1. 读取当前 agent 的开关
2. 若开关关闭，直接跳过
3. 若开关开启且存在 `project_dir`，则按配置列表读取项目根目录文件
4. 合并内容后写入 `PromptConfig`
5. `SystemPromptBuilder::from_config` 负责追加该 section

### 3. Prompt Reload

`AgentWorkspaceService::reload_session_system_prompt` 也需要走同一套逻辑，确保：

1. reload 前后的提示词来源一致
2. UI 的 prompt preview 能看到该 section
3. 开发项目提示词变更后可重新加载生效

### 4. CLI 路径

CLI 当前没有稳定的 session `project_dir` 管理能力。建议首版保持以下策略：

1. 若 CLI 运行时有明确项目目录上下文，则可复用同一加载器
2. 若没有项目目录，则不注入开发项目提示词

首版实现重点仍应放在应用主链路，不把 CLI 特判做复杂。

---

## 数据模型建议

### `AppConfig`

新增顶层字段：

```rust
pub developer_prompt_files: Vec<String>
```

约束：

1. 默认为空列表
2. 空字符串应在校验期拒绝或清理
3. 相对路径相对于 `project_dir` 根目录解析

### `AgentSpec`

新增字段：

```rust
pub enable_project_developer_prompt: bool
```

约束：

1. 默认值建议为 `false`
2. 仅控制“是否读取并注入开发项目提示词”
3. 不影响现有 `project_context_file` 与 agent 基础 prompt 逻辑

### `PromptConfig`

新增字段：

```rust
pub developer_project_prompt_content: Option<String>
```

必要时可补充：

```rust
pub developer_project_prompt_sources: Vec<PathBuf>
```

其中 `sources` 主要用于调试和 preview，可作为次要增强项。

---

## 建议实现边界

### 1. 首版不做的内容

以下内容明确不纳入本次设计：

1. 文件内容长度截断
2. section 抽取或结构化解析
3. 项目目录递归扫描
4. 多目录根配置
5. 提示词优先级/覆盖模式切换

### 2. 错误处理原则

建议采用“单文件失败不影响整体会话”的容错策略：

1. 文件不存在：跳过
2. 文件为空：跳过
3. 单个文件读取失败：记录 `warn!` 并继续读取其他文件
4. 全部失败或全部为空：等价于未配置开发项目提示词

这样更符合开发项目辅助信息的定位，不应因附加提示词失败而阻断主对话链路。

---

## Plan 拆分

| Plan | 标题 | 职责 | 依赖 | 状态 |
|---|---|---|---|---|
| **Plan 1** | 配置模型扩展 | 新增顶层文件列表配置与 agent 开关，补齐默认值和校验 | 无 | 待开始 |
| **Plan 2** | 提示词加载与拼装 | 实现项目根文件读取、内容合并、PromptConfig 与 SystemPromptBuilder 扩展 | Plan 1 | 待开始 |
| **Plan 3** | 会话链路接入与验证 | 将加载逻辑接入会话执行、reload、必要的 CLI/测试路径 | Plan 1, Plan 2 | 待开始 |

执行顺序：Plan 1 → Plan 2 → Plan 3

---

## 风险与待定项

| 类型 | 描述 | 当前结论 |
|---|---|---|
| **项目根定义** | `project_dir` 是否始终等价于项目根 | 当前按 session 设定值视为项目根，后续若有“子目录工作区”需求再调整 |
| **CLI 一致性** | CLI 没有完整 session project_dir 生命周期 | 首版优先主应用链路，CLI 仅在具备项目目录上下文时复用 |
| **提示词重复** | 开发项目提示词与 `PROJECT.md` 可能表达重复约束 | 保持两个独立 section，后续通过文档约定减少重复 |
| **来源可观测性** | 合并后不易直接判断具体命中文件 | 建议在合并文本中保留 `Source:` 标记 |
