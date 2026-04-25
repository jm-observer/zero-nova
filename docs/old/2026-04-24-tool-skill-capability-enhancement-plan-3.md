# 2026-04-24 tool-skill-capability-enhancement-plan-3

| 章节 | 说明 |
|------|------|
| Plan 编号与标题 | Plan 3：Tool 暴露策略、Task 编排与 ToolSearch 协同 |
| 前置依赖 | Plan 1、Plan 2 |
| 本次目标 | 把当前零散存在的 loaded/deferred tool、task store、skill tool、agent tool 串成统一的能力暴露策略，减少无关工具噪音，并让复杂 skill 能基于 Task 与 ToolSearch 形成稳定工作流。 |
| 涉及文件 | `crates/nova-core/src/tool.rs`、`crates/nova-core/src/tool/builtin/mod.rs`、`crates/nova-core/src/tool/builtin/tool_search.rs`、`crates/nova-core/src/tool/builtin/task.rs`、`crates/nova-core/src/tool/builtin/skill.rs`、`crates/nova-core/src/tool/builtin/agent.rs`、`crates/nova-core/src/event.rs`、`crates/nova-core/src/prompt.rs` |
| 代码验证状态 | 已确认 (2026-04-24) |

---

## 详细设计

### 1. ToolRegistry 与 CapabilityPolicy 对接

#### 1.1 当前状态（已验证 `crates/nova-core/src/tool.rs`）

**现有 `ToolRegistry` 结构**：

```rust
pub struct ToolRegistry {
    tools: Mutex<Vec<Arc<dyn Tool>>>,           // loaded tools
    deferred: Mutex<Vec<DeferredToolEntry>>,    // deferred tools (仅条目，尚未加载)
}
```

**关键方法**：
- `tool_definitions() -> Vec<ToolDefinition>` — 当前返回 **所有** loaded tools + 一个 `ToolSearch` entry（line 115-138）
- `deferred_definitions() -> Vec<ToolDefinition>` — 返回 **所有** deferred 工具（line 145-155）
- `execute()` — 有 legacy 名称映射（`bash` → `Bash` 等，line 189-196）
- `resolve_deferred()` — 按名称从 deferred 提升到 loaded（line 162-177）

**当前 `register_builtin_tools()` 是静态注册**：启动时根据 `tool_whitelist` 决定是否注册。

#### 1.2 目标：注册 vs 暴露 两层分离

核心思想：**所有 tool 都注册，但每轮根据 `CapabilityPolicy` 生成"可见视图"**

```rust
// 新增接口
impl ToolRegistry {
    /// 获取当前轮次可见的 tool 定义视图
    pub async fn get_turn_view(
        &self,
        policy: &CapabilityPolicy,
    ) -> Result<TurnToolView> {
        let loaded = self.lock_tools().clone();
        let deferred = self.lock_deferred().clone();

        // 1. 总是暴露 loaded tools
        let mut loaded_for_turn: Vec<ToolDefinition> = loaded
            .iter()
            .map(|t| t.definition().into_sys_tools_def())
            .collect();

        // 2. 根据策略过滤 deferred tools
        let deferred_for_turn: Vec<DeferredToolDefinition> = deferred
            .into_iter()
            .filter(|d| policy.is_tool_enabled(&d.name))
            .map(|d| DeferredToolDefinition {
                name: d.name,
                description: d.description,
                input_schema: d.input_schema,
            })
            .collect();

        Ok(TurnToolView {
            loaded: loaded_for_turn,
            deferred: deferred_for_turn,
            tool_search_enabled: policy.tool_search_enabled,
            skill_tool_enabled: policy.skill_tool_enabled,
            task_tools_enabled: policy.task_tools_enabled,
        })
    }
}
```

**`TurnToolView` 结构**：

```rust
pub struct TurnToolView {
    pub loaded: Vec<ToolDefinition>,     // 已加载且始终可见的工具
    pub deferred: Vec<DeferredToolDefinition>, // 当前轮次可见的 deferred 工具
    pub tool_search_enabled: bool,       // 是否暴露 ToolSearch
    pub skill_tool_enabled: bool,
    pub task_tools_enabled: bool,
}
```

**影响**：provider 看到的是 `TurnToolView`，整个 registry 不需要为不同入口重复构造多个实例，也便于 skill 切换时动态调整。

#### 1.3 策略到视图的映射逻辑

```rust
impl CapabilityPolicy {
    pub fn get_enabled_tools(&self) -> ToolStatus {
        // 根据 strategy 决定哪些工具对该 session 可用
        match &self.strategy {
            CapabilityStrategy::Minimal => ToolStatus {
                system: true,
                read_write: true,
                search: true,
                task: false,
                skill: true,
                agent: true,
                tool_search: true,
            },
            CapabilityStrategy::Flow => ToolStatus {
                system: true,
                read_write: true,
                search: false,
                task: true,
                skill: true,
                agent: true,
                tool_search: true,
            },
            CapabilityStrategy::Agent => ToolStatus {
                system: true,
                read_write: true,
                search: true,
                task: true,
                skill: false,
                agent: true,
                tool_search: false,
            },
            _ => ToolStatus::full(),
        }
    }
}
```

---

### 2. ToolSearch 的职责强化

#### 2.1 当前状态（已验证）

**`tool_search.rs` 现有实现**：
- 仅支持 `select:Name` 快速路径（line 29-33）
- 支持通用搜索（line 35-39），但结果是 **名称字符串数组**
- `handle_selection()`：按名称查找并 `resolve_deferred()`（line 41-63）
- `handle_search()`：返回 "Found matching deferred tools: X, Y"（line 65-86）

**问题**：
- 搜索结果只返回名称，**不返回 schema**
- 支持 `category:xxx` 查询模式，但 query 匹配逻辑是字符串 `case_insensitive().starts_with()` 而非语义匹配
- 缺少详细 `match_reason` 字段

#### 2.2 增强：按类别过滤

**新增查询模式**：

```
type:task      — 仅返回 Task 相关工具
type:skill     — 仅返回 Skill 相关工具
type:search    — 仅返回搜索相关工具
type:list      — 返回当前轮次所有可用工具
```

**`ToolSearch` 增强返回结构**：

```rust
pub struct DeferredToolRepresentation {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub category: DeferredToolCategory,
}

pub enum DeferredToolCategory {
    Task,        // TaskCreate, TaskList, TaskUpdate
    Skill,       // Skill
    Search,      // WebSearch, WebFetch
    System,      // Bash, Read, Write, Edit
}
```

**增强后的查询处理**：

```rust
async fn handle_search(&self, registry: &ToolRegistry, query: &str) -> Result<String> {
    // 1. 尝试按类别解析
    if let Some(category) = query.strip_prefix("type:").and_then(|s| {
        match s.to_lowercase().as_str() {
            "task" => Some(DeferredToolCategory::Task),
            "skill" => Some(DeferredToolCategory::Skill),
            "search" => Some(DeferredToolCategory::Search),
            _ => None,
        }
    }) {
        // 过滤指定类别
        let tools = registry
            .lock_deferred()
            .iter()
            .filter(|d| matches!(d.category, Category))
            .map(|d| format!("{}: {}", d.name, d.description))
            .collect::<Vec<_>>();

        return Ok(format!(
            "Deferred tools in '{}':\n{}\n\nType 'select:<name>' to load a specific tool.",
            category,
            if tools.is_empty() { "None found." } else { &tools.join("\n") }
        ));
    }

    // 2. 使用现有 query 匹配逻辑
    // ...
}
```

#### 2.3 增强：返回 "为什么推荐这个工具"

`handle_search` 的返回值需要包含 `match_reason` 字段，供 LLM 理解为何推荐某个工具：

```rust
// 例如：
"TaskCreate: Creates a new task for tracking work items (category: task,
 reason: user mentioned 'create a task' and 'track progress')"

"Read: Reads file content from disk (category: system,
 reason: user mentioned reading configuration or files)"
```

---

### 3. SkillTool 的定位调整

#### 3.1 当前状态

**`skill.rs:77-85`** — `SkillTool` 当前是"读取 skill 文本"：

```rust
impl Tool for SkillTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Skill".to_string(),
            description: "Load skill instructions for current context".to_string(),
            input_schema: json!({"type": "object", "properties": {"skill": {"type": "string"} ...
        }
    }
}
```

#### 3.2 新定位 — 补充机制，不是主激活机制

**三层次 Tool 模型**：

| 层次 | 工具 | 作用 |
|------|------|------|
| 主路径 | Skill Router | 通过 `TurnContext` 自动激活 active skill |
| 补充路径 | `SkillTool` | 模型在 skill 内部想加载外部专用说明时调用 |
| 回退路径 | `tool_whitelist` | agent 规格中的硬限制 |

**`SkillTool` 改进**：

1. **输出带结构化元数据**，不再只返回一段拼接文本
2. **不覆盖 `active_skill` 状态**，仅作为技能内容补充（模型可以"借用"skill 的指令但 main session 仍保持原 skill）
3. 支持通过 `select:<skill_name>` 或 `type:system` 条件指定加载范围

```rust
// 新的 SkillTool 输出格式
pub struct SkillToolOutput {
    pub skill_name: String,
    pub description: String,
    pub instructions: String,  // 主指令
    pub tools: Vec<String>,    // 该 skill 支持的工具列表
    pub source: String,        // 源头文件路径
}
```

这样可以避免"skill 系统"和"Skill tool"争夺主流程控制权。

---

### 4. Task 工具与工作流编排

#### 4.1 当前 Task 工具（已验证）

**`task.rs` 现有结构**：

```rust
pub struct TaskStore {
    tasks: Arc<std::sync::RwLock<HashMap<String, Task>>>,
    event_tx: mpsc::Sender<AgentEvent>,
}

pub struct Task {
    pub id: String,
    pub subject: String,
    pub content: String,
    pub status: TaskStatus,
    pub progress: TaskProgress,
}
```

**事件**：通过 `event_tx` 发射 `TaskCreated`、`TaskStatusChanged`、`TaskCompleted`。

#### 4.2 任务工具动态暴露规则

**变更核心**：从"静态暴露"改为"ToolSearch 动态加载"。

**依据**：原始会话数据显示 Task 工具在会话中途（约第 15 条消息）通过 ToolSearch 才加载，说明"到达性加载"而非"一次性全部暴露"更适合实际场景。

```rust
pub struct TaskDynamicExposure {
    // 检测器：判断当前消息是否需要 Task 工具
    pub keyword_detector: TaskKeywordDetector,

    // 暴露策略：检测到关键词后的行为
    pub exposure_mode: TaskExposureMode,
}

pub enum TaskExposureMode {
    // 模式 1：检测到后，Task 工具保持到 session 结束
    SessionScoped,

    // 模式 2：每轮根据 capability policy 重新评估
    PerTurn,

    // 模式 3：在 ToolSearch 中可见，模型按需选择
    ToolSearchOnly,
}
```

**关键词检测器**（用于判断当前消息是否需要加载 Task 工具）：

```
关键词示例：
- "拆分任务"、"帮我计划"、"分成步骤"
- "任务"、"todo"、"里程碑"
- "检查进度"、"更新任务"
- "列出所有任务"
```

**暴露策略流程**：

```
用户消息 ──► Keyword Detector ──► 匹配度评分
                                 │
                    ┌────────────┼────────────┐
                    ▼            ▼            ▼
              <0.5 失配     0.5-0.8 弱匹配    >0.8 强匹配
              No exposure  ToolSearch visible  Auto-unlock
```

**对比静态 vs 动态暴露**：

| 维度 | 静态暴露 | 动态暴露 |
|------|----------|----------|
| Prompt 开销 | 所有轮次可见 | 仅需要时展示 |
| 模型理解复杂度 | 较少 | 稍微增加 |
| ToolSearch 依赖 | 否 | 是 |
| 适用场景 | 已知 workflow skill | 通用场景 |

#### 4.3 编排约束

1. **一个 session 内同一时刻最多一个 `in_progress` 主任务**
   - 当前 `Task` 已有 `status: TaskStatus` 字段
   - 新增 `is_main_task: bool` 标记主任务
   - `TaskCreate` 检查时若已有主任务则返回警告

2. **子任务可挂在 `metadata.parent_id` 下**
   - 当前 `Task` 已支持 `metadata: HashMap<String, Value>`
   - 使用 `metadata["parent_id"]` 存放父任务 id
   - `TaskList` 支持 `?flat=true` 参数返回扁平列表或嵌套树

3. **若 skill 为 sticky workflow，则任务状态与 skill 生命周期关联**
   - `AgentEvent::SkillExited` 时，若 skill 有 `sticky=true` 且 `task_status=in_progress`，则自动 pause 但不清除
   - 后续 re-activating skill 时恢复任务状态

---

### 5. Agent 工具与 skill/工具策略联动

#### 5.1 当前状态（已验证 `agent.rs`）

**子代理工厂创建逻辑**：

```rust
impl AgentFactory {
    pub fn create_subagent(
        &self,
        runtime: Arc<dyn Runtime>,
        spec: &AgentSpec,
        ctx: ToolContext,
    ) -> Arc<dyn Agent> {
        let task_store = ctx.task_store.clone();
        let skill_registry = ctx.skill_registry.clone();

        let (runtime, prompt) = runtime.create_subagent(spec, task_store, skill_registry);
        Agent::new(runtime, spec.clone(), prompt)
    }
}
```

#### 5.2 子代理工具集计算

**默认继承当前 skill 的工具策略**：

```rust
impl TurnToolView {
    pub fn get_agent_tool_subset(&self, policy: &CapabilityPolicy) -> ToolSubset {
        let mut subset = self.clone();

        // 1. 先默认继承父 skill 的策略
        if let Some(parent_skill) = &policy.active_skill {
            match &parent_skill.tool_policy {
                ToolPolicy::InheritAll => { /* 保持父级全部 */ }
                ToolPolicy::AllowList(whitelist) => {
                    subset.loaded.retain(|t| whitelist.contains(&t.name));
                }
                ToolPolicy::AllowListWithDeferred(whitelist) => {
                    subset.loaded.retain(|t| whitelist.contains(&t.name));
                }
            }
        }

        // 2. 若 agent spec 自带更窄的 whitelist，则进一步收缩
        if let Some(agent_whitelist) = &policy.agent_tool_whitelist {
            subset.loaded.retain(|t| agent_whitelist.contains(&t.name));
        }

        // 3. 子代理不自动继承父级 active skill（除非显式指定）
        if !policy.agent_inherit_skill {
            subset.skill_tool_enabled = false;
        }

        subset
    }
}
```

这样可以避免父会话中的高权限 skill 或大工具集泄漏给子代理。

---

## 测试案例

1. **正常路径**：默认对话只看到基础工具与 `ToolSearch`，不会一次性暴露全部 deferred tool。
2. **正常路径**：调用 `ToolSearch` 后，指定 deferred tool 被加载并出现在后续轮次工具定义中。
3. **正常路径**：workflow skill 激活后，Task 工具可见并能正常创建、更新、列出任务。
4. **边界条件**：普通闲聊 skill 下不暴露 Task 工具，模型无法误创建任务。
5. **边界条件**：`SkillTool` 在 active skill 已存在时仍可作为补充说明加载，但不能覆盖 active skill 状态。
6. **异常场景**：请求不存在的 deferred tool 时，`ToolSearch` 返回可诊断错误并列出接近候选。
7. **异常场景**：子代理工具策略计算失败时，回退到最小权限集合，而不是继承全部工具。
8. **新测试**：验证 `ToolRegistry::get_turn_view()` 返回不同 `CapabilityPolicy` 时结果不同。
9. **新测试**：验证 `ToolSearch` 新增的 `type:task` / `type:skill` 查询能正确过滤。
10. **新测试**：验证 `SkillTool` 调用后 `active_skill` 仍未变更（仅补充说明）。
11. **新测试**：验证 `TaskStore::create_task()` 在已有主任务时返回警告。
12. **新测试**：验证 `TaskStore::list_tasks(flat=false)` 正确构建 parent-child 树结构。
