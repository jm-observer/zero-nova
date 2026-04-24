# 2026-04-24 tool-skill-capability-enhancement-plan-4

| 章节 | 说明 |
|------|------|
| Plan 编号与标题 | Plan 4：CLI / Gateway / DeskApp 集成、观测与评测 |
| 前置依赖 | Plan 2、Plan 3 |
| 本次目标 | 将 skill/tool 能力系统接入 CLI、gateway、deskapp 的真实链路，补齐事件协议、调试可视化、示例配置和回归测试，让该系统具备交付与持续迭代能力。 |
| 涉及文件 | `crates/nova-cli/src/main.rs`、`crates/nova-app/src/bootstrap.rs`、`crates/nova-app/src/types.rs`、`crates/nova-gateway-core/src/bridge.rs`、`deskapp/src` 相关状态展示模块、`.nova/README.md`、`.nova/examples/interaction-samples.json`、`.nova/examples/workflow-e2e.json`、`docs/todo/2026-04-24-claude-code-usage-analysis.md` |
| 代码验证状态 | 部分确认 (2026-04-24) |

---

## 详细设计

### 1. CLI 集成

#### 1.1 当前 CLI 入口

根据 AGENTS.md，CLI 入口为：
```bash
cargo run --bin nova_cli -- chat
```

CLI 模块位置可能在 `crates/nova-cli/` 或其 `nova_cli` binary。

#### 1.2 CLI 调试命令

CLI 至少补齐以下能力：

| 命令 | 功能 | 数据来源 |
|------|------|----------|
| `/skills` | 列出当前可用 skill 与 active skill | `SkillRegistry::all_candidates()` |
| `/skill <id>` | 手动激活某个 skill，便于调试 | `SkillRouter::route()` |
| `/exit-skill` | 退出当前 skill | `SkillRouter::route()` → Deactivate |
| `/prompt-sections` | 查看当前轮实际组装的 prompt sections | `PromptSectionBuilder::debug_sections()` |
| `/tasks` | 查看当前 session 的 task 状态 | `TaskStore` 快照 |
| `/tools` | 查看当前轮次可见工具视图 | `ToolRegistry::get_turn_view()` |
| `/status` | 显示整体状态（skill/agent/tool-policy） | 综合各组件状态 |

**CLI 命令解析逻辑**：

```rust
pub enum CliCommand {
    Skills,
    SkillActivate(String),
    SkillExit,
    PromptSections,
    Tasks,
    Tools,
    Status,
    Message(String), // 普通用户消息
}

impl CliCommand {
    pub fn parse(input: &str) -> CliCommand {
        if input.starts_with('/ {
            return match input.split_whitespace().next() {
                Some("/skills") => CliCommand::Skills,
                Some("/skill") => CliCommand::SkillActivate(input[6..].trim().to_string()),
                Some("/exit-skill") => CliCommand::SkillExit,
                Some("/prompt-sections") => CliCommand::PromptSections,
                Some("/tasks") => CliCommand::Tasks,
                Some("/tools") => CliCommand::Tools,
                Some("/status") => CliCommand::Status,
                _ => CliCommand::Message(input.to_string()),
            };
        }
        CliCommand::Message(input.to_string())
    }
}
```

---

### 2. Gateway / App 事件映射

#### 2.1 事件协议设计

桥接层需要把新增事件映射到前端可消费协议：

**内部事件（Nova 内部）** → **桥接事件（Gateway Protocol）**

| 内部事件 | 桥接事件 | Payload |
|----------|----------|---------|
| `AgentEvent::SkillActivated` | `SkillActivated` | `{ skill_id, skill_name, sticky }` |
| `AgentEvent::SkillSwitched` | `SkillSwitched` | `{ from_skill, to_skill }` |
| `AgentEvent::SkillExited` | `SkillExited` | `{ skill_id }` |
| `AgentEvent::ToolStart` + deferred | `ToolUnlocked` | `{ tool_name }` |
| `AgentEvent::TaskStatusChanged` | `TaskStatusChanged` | `{ task_id, status, subject }` |
| `AgentEvent::SkillRouteEvaluated` | `SkillRouteEvaluated` | `{ skill_id, confidence }` |

#### 2.2 桥接层位置

bridge.rs 位置假设在 `crates/nova-gateway-core/src/bridge.rs`，负责：

1. 接收 `nova-core` 的 `AgentEvent` 通道
2. 映射到 gateway 协议类型
3. 通过 WebSocket connection 推送到 deskapp

**验证注意事项**：需要先确认 `nova-gateway-core/src/bridge.rs` 是否存在，以及当前的事件协议格式。如果 bridge 层不在该位置，可能需要调整。

#### 2.3 事件协议约束

- 事件协议需要保持**扁平、稳定**，避免把内部结构直接暴露给前端
- 使用 string 枚举表示状态，而非 Rust enum 的整数编码
- 使用 optional 字段，保证前端不因新增字段而崩溃
- （新增）**System-Reminder 透传**：system-reminder 标签应与 agent 事件分别透传，不做结构化映射

---

### 3. DeskApp 展示

#### 3.1 当前 DeskApp 架构

桌面端使用 Tauri（`deskapp/src-tauri` 管理 sidecar 生命周期 + `deskapp/src` 前端 UI）。

#### 3.2 两个轻量展示面

**第一：会话头部或侧栏显示**

```
┌─────────────────────────────────────────────┐
│ Session Header                              │
│ ┌──────────┐ ┌──────────┐ ┌──────────┐     │
│ │ Active   │ │ Agent    │ │ Mode     │     │
│ │ Skill    │ │ (ID)     │ │ (Policy) │     │
│ └──────────┘ └──────────┘ └──────────┘     │
└─────────────────────────────────────────────┘
```

**第二：进度面板显示**

```
┌─────────────────────────────────────────────┐
│ Progress Panel                              │
│ ┌─────────────────────────────────────────┐ │
│ │ Tasks                                   │ │
│ │ ● Task1 (in progress)                   │ │
│ │ ○ Task2 (pending)                       │ │
│ └─────────────────────────────────────────┘ │
│ ┌─────────────────────────────────────────┐ │
│ │ Events (last 5)                         │ │
│ │ [10:01] ToolUnlocked: TaskCreate        │ │
│ │ [10:00] SkillActivated: code-review     │ │
│ │ [09:58] SkillExited: default            │ │
│ └─────────────────────────────────────────┘ │
└─────────────────────────────────────────────┘
```

#### 3.3 实现策略

- **第一阶段只做可读性展示**，不做复杂交互编排器
- 使用 Tauri 的 `window.emit()` + `window.on()` 接收事件
- 前端组件订阅 `SkillsStore`、`TaskStore`、`EventLog` 三个状态

---

### 4. 示例与评测资产

#### 4.1 新增示例文件

在现有 `.nova/examples/` 基础上增加：

| 文件 | 用途 | 内容 |
|------|------|------|
| `skill-routing-samples.json` | 路由命中测试 | 5-10 条消息 + 期望的 skill 激活结果 |
| `tool-unlock-samples.json` | ToolSearch 解锁测试 | ToolSearch 查询 + 期望返回的工具列表 |
| `workflow-skill-e2e.json` | 完整工作流测试 | 多轮对话序列 + skill/任务/工具事件流 |

**`skill-routing-samples.json` 结构示例**：

```json
{
  "version": "1.0",
  "samples": [
    {
      "message": "Create a skill for code review",
      "expected_route": "Activate(skill-creator)",
      "confidence_expected": 0.9,
      "active_skill_after_action": true
    },
    {
      "message": "Review this Python code",
      "expected_route": "KeepCurrent",
      "active_skill_id": "code-review",
      "sticky": true
    }
  ]
}
```

**`workflow-skill-e2e.json` 结构示例**：

```json
{
  "version": "1.0",
  "workflow": "code-review",
  "stages": [
    {
      "name": "setup",
      "skill": "code-review",
      "tools": ["Read", "Write", "Edit"],
      "expected_duration": "short"
    },
    {
      "name": "review",
      "skill": "code-review",
      "tools": ["Bash", "Read", "Skill"],
      "expected_duration": "medium"
    }
  ],
  "events": [
    "SkillActivated", "TaskCreated", "ToolUnlocked", "SkillExited"
  ]
}
```

---

#### 4.3 Memory 200 行限制（新增 - 基于 v1_messages 分析）

**背景**：原始 Claude Code 会话中，`MEMORY.md`（memory 索引文件）有 **200 行截断限制**。超过此行数的条目将被系统提示词丢弃。

**设计影响**：
- `MEMORY.md` 是**索引**，不是 memory 内容本身
- 每行格式：`- [Title](file.md) — one-line hook`
- 短条目（<150 字符）确保在 200 行限制内包含足够多的高质量 memory

**计算示例**：

```
假设平均每行：
- Title 部分：30 字符
- Link 部分：50 字符
- Hook 部分：60 字符
- 总计：~140 字符/行

200 行 × 140 字符 = ~28,000 字符（memory 索引总大小）

Measured in v1_messages：auto memory 段约 35% 的 prompt 占比
```

**约束**：
1. 新 memory 写入前需检查当前行数
2. 超过 200 行时删除最旧或最不相关的条目
3. 不将重复内容写入 `MEMORY.md`（先检查是否存在）
4. Memory 内容存储在独立文件中，不受 200 行限制

---

#### 4.2 更新 `.nova/README.md`

明确以下内容：
- skill 包推荐结构（与 `.nova/skills/<slug>/` 目标格式一致）
- tool / skill 能力关系图
- CLI 调试命令列表
- 配置样例（含新增的 `gateway.skill_routing_enabled` 字段）

---

### 5. 回归测试策略

#### 5.1 测试分层

本 plan 不新增复杂评测框架，优先使用：

| 层级 | 工具 | 覆盖范围 |
|------|------|----------|
| **单元测试** | `nova-core` tests | 数据结构、策略计算、prompt section |
| **集成测试** | `nova-cli` tests | 命令与事件输出 |
| **协议测试** | `nova-app` / `gateway-core` tests | 事件映射正确性 |
| **示例驱动测试** | `.nova/examples/*.json` 读取验证 | 路由与事件流完整性 |

#### 5.2 测试数据格式

**`SkillRouteTestCase`**：

```rust
pub struct SkillRouteTestCase {
    pub input_message: String,
    pub expected_decision: SkillRouteDecision,
    pub confidence_expected: f64,
    pub active_skill_id: Option<String>,
}
```

**`ToolUnlockTestCase`**：

```rust
pub struct ToolUnlockTestCase {
    pub search_query: String,
    pub expected_tools: Vec<String>,
    pub tool_search_enabled: bool,
}
```

**`WorkflowE2ETestCase`**：

```rust
pub struct WorkflowE2ETestCase {
    pub message_sequence: Vec<String>,
    pub expected_events: Vec<EventExpectation>,
}

pub struct EventExpectation {
    pub event_type: String,
    pub index_position: usize,  // 在消息序列中的位置
    pub event_data_check: Option<String>,  // 可选的字段检查
}
```

---

## 测试案例

1. **正常路径**：CLI 能列出 skills、切换 active skill、退出 skill，并看到对应事件。
2. **正常路径**：gateway 将 `SkillActivated`、`ToolUnlocked`、`TaskStatusChanged` 正确桥接到 app 事件。
3. **正常路径**：deskapp 能展示当前 active skill 和任务进度，不因事件缺字段而崩溃。
4. **边界条件**：无 skills 目录时，CLI / gateway 仍能正常运行，仅禁用相关展示。
5. **边界条件**：skill 路由被关闭时，系统仍能通过手动 `/skill` 进入调试模式。
6. **异常场景**：前端收到未知 skill/tool 事件类型时安全忽略并记录日志。
7. **回归场景**：基于示例文件跑完整 workflow，验证 skill 激活、task 更新、tool 解锁顺序符合预期。
8. **新测试**：验证 `CliCommand::parse()` 能正确解析所有 `/skill`、`/tools`、`/status` 等命令。
9. **新测试**：验证事件桥接层不对 payload 做结构破坏（使用 `serde_json::Value` 传递）。
10. **新测试**：验证 deskapp 状态展示组件在事件缺失时的 graceful degradation。
