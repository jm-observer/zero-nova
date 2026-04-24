# Tool System 待修复总览

## 时间
- 创建时间：2026-04-24
- 最后更新：2026-04-24

## 关联文档
- 设计文档：[docs/design/tool-system-enhancement.md](/D:/git/zero-nova/docs/design/tool-system-enhancement.md)
- 参考目录：`docs/tool_definitions/`

## 背景
本轮工作围绕工具系统增强设计展开，目标是把现有工具能力逐步收敛到设计文档定义的目标状态，包括：
- 统一工具命名和参数约定
- 补齐 `Edit`、`Skill`、`Task*`、`ToolSearch` 等新增工具
- 增强 `Agent`、`Bash`、`Read`、`Write`
- 支持延迟加载、任务事件、前置 Read 校验、子 agent 能力隔离等行为

设计文档给出的目标状态是“从旧的基础工具集合，演进到更完整的一套 10 个工具的体系”，并且要求兼顾一段时间内的兼容迁移。

## 当前目标
当前阶段不是重新设计，而是继续收尾和修复实现与设计之间的差距，重点是：
- 修掉 review 已指出的 bug 和行为偏差
- 保证工具接口、事件字段、兼容映射与设计文档一致
- 把“已搭骨架但未闭环”的能力明确列成后续待办

## 当前状况
根据最近一轮实现与修复结果，工具系统主骨架已经基本成型，整体可以认为：
- `Edit`、`Read`（文本）、`Write`、`Skill`、`TaskCreate/TaskList/TaskUpdate`、`ToolSearch` 已经有可用实现
- `Bash` 已支持更多参数，并完成了工具名大写化
- `Agent` 已接入子 agent 类型、事件转发和部分参数，但仍有功能缺口
- `Read` 的 PDF / 图片 / Jupyter Notebook 能力仍未真正落地

目前代码侧已经补过一轮关键修复，包括：
- `web_fetch` 截断逻辑改为 UTF-8 安全
- `Read/Edit/Write` 补了 `path -> file_path` 兼容
- `Read` 的“已读文件”记录改为在确认文件存在后再写入
- `Edit` 的错误顺序改为先报 `file not found`，再做 pre-read 校验
- `TaskStatusChanged` / `BackgroundTaskComplete` 事件字段补齐
- `WebFetch` / `WebSearch` 已切换为设计文档要求的大写命名，并保留 legacy 映射
- 子 agent 的 `tool_whitelist` 已实际生效
- 子 agent 的 `max_iterations` 不再硬编码 `15`
- `Skill.args` 不再完全无效，已经进入工具输出
- `TaskStatus` 事件字符串改为显式映射，而不是依赖脆弱的序列化裁剪

## 已完成与设计基本对齐的部分

### 1. 工具命名与兼容迁移
- 已支持旧名到新名的兼容映射
- 已处理 `path` 到 `file_path` 的过渡兼容

### 2. 文件类工具
- `Read` 文本分页、行号格式、pre-read 跟踪已具备
- `Write` 已有“已读后再写”校验
- `Edit` 已实现唯一性校验、replace_all 和 pre-read 约束

### 3. 任务系统
- `TaskCreate` / `TaskList` / `TaskUpdate` 已可用
- 任务依赖跟踪、自动解锁和事件发送已接通

### 4. 延迟加载与技能
- `ToolSearch` 已实现
- `Skill` 已实现基础加载与输出
- deferred tool schema 已从“实例化工具取 schema”调整为静态 schema 接口

### 5. 子 agent 行为
- 子 agent 工具白名单已真正应用
- 事件转发不再依赖 `50ms sleep + abort` 的脆弱退出方式

## 仍待修复 / 待补完事项

### 高优先级

#### 1. `Agent` 仍硬编码 `OpenAiCompatClient`
- 现状：子 agent 没有复用父级实际使用的 LLM client 抽象。
- 影响：如果父 agent 后续支持其他 provider，子 agent 行为会和主链路不一致。
- 建议：把子 agent client 的创建改成可注入/可复用，而不是在工具内部固定 new `OpenAiCompatClient`。

#### 2. `Read` 的多模态读取仍是 stub
- 现状：PDF、图片、Notebook 设计里要求支持，但当前仍只有占位逻辑或未实现逻辑。
- 影响：设计目标里最明显的一块功能缺口仍未补齐。
- 建议：按类型拆分为 `read_pdf`、`read_image`、`read_notebook` 私有函数，再补测试。

#### 3. `Agent.run_in_background` / `isolation: worktree` 仍未真正实现
- 现状：参数存在，但当前只是返回 warning 或同步执行。
- 影响：接口表面上可用，实际行为和 schema 含义不一致。
- 建议：优先决定这两个能力是否本轮必须交付；如果必须，应做真实实现；如果暂缓，应明确在 schema 或文档中标注当前限制。

### 中优先级

#### 4. `SkillRegistry` 仍是 `Vec` + 线性搜索
- 现状：与设计文档里的 `HashMap<String, SkillDefinition>` 不一致。
- 影响：查找效率和按名称管理能力都偏弱，也不利于后续别名 / namespace 扩展。
- 建议：改成 map 结构，同时梳理技能元数据模型。

#### 5. 技能加载仍使用同步 `std::fs`
- 现状：当前加载路径在 async bootstrap 流程中间接调用同步文件 IO。
- 影响：不符合项目“不要阻塞 async runtime”的约束。
- 建议：迁移到 `tokio::fs`，或使用 `spawn_blocking` 明确隔离阻塞读取。

#### 6. `TaskUpdate` 事件粒度仍可继续优化
- 现状：当前更新任务时仍倾向于发 `TaskStatusChanged`。
- 影响：如果只改 `subject` / `active_form` 等非状态字段，前端刷新语义并不精确。
- 建议：后续判断是否需要拆出更细的任务更新事件，或至少在发送前区分是否真的改了 `status`。

### 低优先级

#### 7. `ToolSearch` 搜索能力仍偏基础
- 现状：已有实现，但距离设计文档里的更复杂查询语义还有差距。
- 影响：不影响主流程，但高级检索体验一般。

#### 8. `Bash.description` / sandbox 仍偏弱语义
- 现状：schema 已有参数，但没有形成真正的审计或沙箱控制闭环。
- 影响：更像“参数补齐”，不是“能力完整”。

## 推荐后续修复顺序
1. `Agent` 的 LLM client 抽象问题
2. `Read` 的 PDF / 图片 / Notebook 真正实现
3. `Agent.run_in_background` 与 `isolation: worktree`
4. `SkillRegistry` 结构和技能加载 IO 模型
5. `TaskUpdate` 事件语义优化
6. `ToolSearch` 与 `Bash` 的剩余增强项

## 实施注意事项
- 本目录文档是“待修复总览”，不是新的架构设计替代品；详细目标仍以设计文档为准
- 后续继续动工具系统时，应以“小步修复”为主，不把大规模重构混入单个问题修复
- 每完成一轮代码修改后，仍需按项目要求执行完整修复流程：
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --check --all`
  - `cargo test --workspace`

## 备注
当前工作区还存在一批未提交源码修改，说明工具系统修复工作仍在进行中。后续继续处理时，应在现有修改基础上增量推进，避免覆盖尚未提交的本地变更。
