# 统一 Prompt 构建管道 — 详细设计文档

**日期**：2026-05-15  
**作者**：基于 superpowers brainstorming 输出  
**状态**：设计完成，实施中  
**目标**：消除"双轨制"，统一主 Agent 和子 Agent 的 prompt 构建逻辑

---

## 1. 问题陈述

### 1.1 当前架构（双轨制）

```
┌─────────────────────────────────────────────────┐
│                    主 Agent                       │
│  ┌───────────────────────────────────────────┐  │
│  │ SystemPromptBuilder::from_material()      │  │
│  │  - 加载 PromptMaterial + TurnPromptMaterial │  │
│  │  - 通过 SkillInjectionMode 注入技能         │  │
│  │  - 生成标准 sections (base, skill, tools)  │  │
│  └───────────────────────────────────────────┘  │
└─────────────────────────────────────────────────┘
                      │
┌─────────────────────────────────────────────────┐
│                   AgentTool (子 Agent)            │
│  ┌───────────────────────────────────────────┐  │
│  │ SystemPromptBuilder::from_material()      │  │ ← 同样调用，但数据源不同
│  │  - 通过 AgentPromptLoader 加载材料         │  │
│  │  - runtime.resolve_active_skill_id() 确定  │  │
│  │    活跃技能                                │  │
│  └───────────────────────────────────────────┘  │
└─────────────────────────────────────────────────┘
```

**问题**：
1. 虽然都调用 `from_material`，但**技能注入的来源和时机不一致**
   - 主 Agent：技能注入由 `PromptMaterial.skill_injection_mode` 决定（配置驱动）
   - 子 Agent：技能注入由 `AgentTool.run_subagent` 的 `prompt` 参数通过 `resolve_active_skill_id` 推导
2. **无法在启动时指定任意技能**：子 Agent 的 skill 选择逻辑硬编码在 `resolve_active_skill_id` 中
3. **扩展性差**：如果需要为子 Agent 添加新的注入策略（如 multi-skill、动态切换），需要在多处修改

### 1.2 目标架构

```
┌──────────────────────────────────────────────────────────────────┐
│                       统一构建管道                                  │
│                                                                    │
│  [ 用户输入/工具调用 ]                                              │
│         ↓                                                          │
│  [ AgentTool / MainAgent (参数解析器) ]                             │
│         ↓                                                          │
│  [ 创建构建请求: PromptConstructionRequest {                      │
│       base_material_id, skill_id, injection_mode,                  │
│       context_overrides, tool_definitions, visible_tool_names      │
│  }]                                                                │
│         ↓                                                          │
│  [ SystemPromptBuilder::build_from_request() ] ◄── 唯一的真相来源！ │
│         ↓                                                          │
│  [ 生成标准物料: struct PromptMaterial {                           │
│       agent_prompt, skills, tools, sections...                     │
│  }]                                                                │
│         ↓                                                          │
│  [ SubagentRuntimeFactory / AgentRuntime (执行引擎) ]               │
│                                                                    │
└──────────────────────────────────────────────────────────────────┘
```

**核心转变**：从"传递结果字符串"转变为"传递构建指令"

---

## 2. 关键数据结构

### 2.1 PromptConstructionRequest（新增）

**文件位置**：`crates/nova-agent/src/prompt/types.rs`

```rust
/// 用于统一构建主 Agent 和子 Agent prompt 的请求对象。
///
/// 取代之前的"双轨制" — 主 Agent 和子 Agent 现在通过同一个
/// `SystemPromptBuilder::build_from_request()` 方法构建 prompt。
#[derive(Debug, Clone)]
pub struct PromptConstructionRequest {
    /// 基础 prompt 材料的标识符（对应 AgentSpec.prompt_file 或实际内容）
    pub base_material_id: String,
    
    /// 要注入的 skill ID（可选）
    pub skill_id: Option<String>,
    
    /// skill 注入模式：Catalog / ActiveFull / Full
    pub injection_mode: SkillInjectionMode,
    
    /// 上下文变量覆盖（会合并到 initial_template_vars 中）
    pub context_overrides: HashMap<String, String>,
    
    /// 原始基础用户消息（用于生成 system prompt）
    pub original_base_user_message: Option<String>,
    
    /// 工具定义（可能由 skill 覆盖）
    pub tool_definitions: Arc<Vec<ToolDefinition>>,
    
    /// 可见工具名称（用于 ToolInfo 可见性过滤）
    pub visible_tool_names: Arc<HashSet<String>>,
}
```

**设计决策**：
- `base_material_id` 不是直接包含 `agent_prompt` 字符串，而是通过外部 lookup（`name_overrides` map）获取
- `tool_definitions` 使用 `Arc` 包装，避免在多个构造点重复复制
- `visible_tool_names` 与 `tool_definitions` 配合，确保 ToolInfo 可见性过滤一致

### 2.2 SkillInjectionMode（已有，复用）

**文件位置**：`crates/nova-agent/src/prompt/types.rs`

```rust
pub enum SkillInjectionMode {
    Catalog,      // 仅显示技能索引，不含完整 instructions
    ActiveFull,   // 活跃技能显示完整 instructions，其他技能只显示索引
    Full,         // 所有技能都显示完整 instructions
}
```

**复用原因**：主 Agent 和子 Agent 共享相同的注入语义，避免概念分裂。

---

## 3. 接口设计

### 3.1 SystemPromptBuilder::build_from_request（新增）

**文件位置**：`crates/nova-agent/src/prompt/builder.rs`

```rust
/// 根据构造请求构建 prompt（统一的主入口）。
///
/// 这是所有 Agent（包括主 Agent 和子 Agent）构建 system prompt 的统一方式。
/// 取代了之前的"双轨制"实现。
pub fn build_from_request(
    &self,
    request: &PromptConstructionRequest,
    name_overrides: &HashMap<String, String>,  // base_material_id -> agent_prompt 映射
    skill_registry: &SkillRegistry,
) -> String {
    let mut builder = Self::new();
    
    // 1. Base section — 构建基础 prompt（合并 template vars）
    let mut template_vars: HashMap<String, String> = request.context_overrides.clone();
    if let Some(ref original_msg) = request.original_base_user_message {
        template_vars.extend(extract_template_vars_from_message(original_msg));
    }
    
    // 从 AgentSpec 或外部 loader 获取 base prompt
    let base_prompt = if let Some(name) = name_overrides.get(&request.base_material_id) {
        name.clone()
    } else {
        format!("Agent task: {}", request.base_material_id)
    };
    
    let rendered_prompt = if template_vars.is_empty() {
        base_prompt.clone()
    } else {
        TemplateContext::render(&base_prompt, &template_vars)
    };
    if !rendered_prompt.is_empty() {
        builder = builder.base_section(&rendered_prompt);
    }
    
    // 2. Behavior guards
    builder = builder.behavior_guards_section();
    
    // 3. Skill section — 根据 injection_mode 注入
    let skill_prompt = match request.injection_mode {
        SkillInjectionMode::Catalog => skill_registry.generate_catalog_prompt(),
        SkillInjectionMode::ActiveFull => {
            skill_registry.generate_contextual_prompt(request.skill_id.as_deref())
        }
        SkillInjectionMode::Full => skill_registry.generate_full_prompt(),
    };
    if !skill_prompt.is_empty() {
        builder = builder.skill_section(&skill_prompt);
    }
    
    // 4. Tool guidance section
    builder = builder.with_tool_definitions_internal(&request.tool_definitions, ToolGuidanceMode::Full);
    
    // 5. Build and return
    builder.build()
}
```

**关键设计点**：
- `name_overrides` 提供灵活性，允许外部传入启动名称（agent prompt 内容）
- 不依赖 `PromptMaterial`，直接消费 `PromptConstructionRequest`
- 保持与 `from_material` 相同的 section 构建逻辑，确保格式一致性

### 3.2 AgentTool 接口变更

**文件位置**：`crates/nova-agent/src/tool/builtin/agent.rs`

**变更**：在 `execute()` 中扩展参数，支持传递 `skill_id` 和 `injection_mode`

```rust
async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
    let prompt = input["prompt"].as_str().ok_or_else(|| anyhow::anyhow!("Missing 'prompt'"))?;
    
    // 新增：获取 skill_id 参数
    let skill_id = input["skill_id"].as_str().map(String::from);
    
    // 新增：获取 injection_mode 参数（可选，默认 Catalog）
    let injection_mode: SkillInjectionMode = input["injection_mode"]
        .as_str()
        .and_then(|s| match s {
            "catalog" => Some(SkillInjectionMode::Catalog),
            "active_full" => Some(SkillInjectionMode::ActiveFull),
            "full" => Some(SkillInjectionMode::Full),
            _ => None,
        })
        .unwrap_or(SkillInjectionMode::Catalog);
    
    // 构造 PromptConstructionRequest
    let request = PromptConstructionRequest {
        base_material_id: prompt.to_string(),
        skill_id,
        injection_mode,
        context_overrides: HashMap::new(),
        original_base_user_message: None,
        tool_definitions: Arc::new(Vec::new()),
        visible_tool_names: Arc::new(HashSet::new()),
    };
    
    // 调用统一构建方法
    let system_prompt = SystemPromptBuilder::build_from_request(&request, &HashMap::new(), skill_registry);
    
    // ... 后续执行逻辑不变
}
```

### 3.3 run_subagent 方法重构

**变更前**（当前逻辑）：
```rust
let prompt_material = self.prompt_loader.load_agent_material(spec, env, template_vars).await?;
let active_skill_id = runtime.resolve_active_skill_id(prompt, &[])?;
let turn_material = self.prompt_loader.load_turn_material(project_dir, workflow_stage, active_skill_id, template_vars, enable_developer_prompt).await?;
let system_prompt = SystemPromptBuilder::from_material(&prompt_material, &turn_material, skill_registry).build();
```

**变更后的逻辑**（使用 build_from_request）：
```rust
// 1. 构造构建请求
let request = PromptConstructionRequest {
    base_material_id: spec.id.clone(),
    skill_id: skill_id.or_else(|| runtime.resolve_active_skill_id(prompt, &[]).ok()),
    injection_mode: req.injection_mode,
    context_overrides: template_vars,
    original_base_user_message: Some(prompt.to_string()),
    tool_definitions: Arc::new(Vec::new()),
    visible_tool_names: Arc::new(HashSet::new()),
};

// 2. 调用统一构建方法
let name_overrides = HashMap::new();
let system_prompt = SystemPromptBuilder::build_from_request(&request, &name_overrides, skill_registry);
```

**关键变更点**：
- 移除了对 `from_material` 的依赖，改为使用 `build_from_request`
- 技能选择逻辑从 `resolve_active_skill_id` 移动到请求构造阶段
- `context_overrides` 直接作为 `PromptConstructionRequest` 的一部分传递

---

## 4. 实施路线图

### 第一阶段：核心协议与构建器扩展 (Expansion Phase)
**目标**：建立统一的"指令传递"标准，并增强 Builder 的处理能力。

| 步骤 | 操作 | 文件 | 验证标准 |
|------|------|------|----------|
| 1.1 | 定义 `PromptConstructionRequest` DTO | `crates/nova-agent/src/prompt/types.rs` | JSON 序列化/反序列化 correct；单元测试通过 |
| 1.2 | 实现 `SystemPromptBuilder::build_from_request()` | `crates/nova-agent/src/prompt/builder.rs` | 模拟不同 Request 参数，验证 PromptMaterial 格式正确 |

### 第二阶段：工具层重构与链路打通 (Refactoring Phase)
**目标**：将 AgentTool 从"字符串操作者"转型为"请求分发器"。

| 步骤 | 操作 | 文件 | 验证标准 |
|------|------|------|----------|
| 2.1 | 扩展 `AgentToolInput` 的输入 Schema | `crates/nova-agent/src/tool/builtin/agent.rs` | JSON 参数格式调用 Agent 工具时，系统不报错 |
| 2.2 | 重构 `AgentTool::execute` 执行逻辑 | `crates/nova-agent/src/tool/builtin/agent.rs` | 转换参数 $\rightarrow$ 构造 Request $\rightarrow$ 调用 Builder $\rightarrow$ 获取 Prompt |
| 2.3 | 适配 Runtime 启动工厂 | `crates/nova-agent/src/tool/builtin/agent.rs` | `SubagentRuntimeFactory` 能够接收 `PromptConstructionRequest` |

### 第三阶段：架构收敛与清理 (Cleanup Phase)
**目标**：移除冗余代码，消除"双轨制"存的物理痕迹。

| 步骤 | 操作 | 文件 | 验证标准 |
|------|------|------|----------|
| 3.1 | 物理删除 `SkillPromptInjector` | `crates/nova-agent/src/skill/injector.rs` | Model 清理引用；剩余代码不报错 |
| 3.2 | 回归测试与性能审计 (Regression & Audit) | 全量测试 | `cargo test --workspace` 全部通过；单次 Agent 启动 Token 开销与延迟无明显变化 |

---

## 5. 变更影响分析

### 5.1 向后兼容性

| 组件 | 变更前 | 变更后 | 兼容性 |
|------|--------|--------|--------|
| `AgentTool` | 接收 `prompt`, `subagent_type` | 新增 `skill_id`, `injection_mode` | ✅ 完全兼容（新参数有默认值） |
| `SystemPromptBuilder` | `from_material()` | 新增 `build_from_request()` | ✅ 完全兼容（新方法，不修改旧方法） |
| `PromptMaterial` | 基础结构体 | 不变 | ✅ 完全兼容 |

### 5.2 风险点

1. **`name_overrides` 缺失**：如果 `build_from_request` 无法在 `name_overrides` 中查找 `base_material_id`，会使用 fallback 文本
   - 缓解：在 `AgentTool` 中正确填充 `base_material_id` 为 `spec.id`
   - 影响：低（仅影响 prompt 显示，不影响功能）

2. **`tool_definitions` 为空**：如果 `AgentTool` 未提供 `tool_definitions`，`build_from_request` 会使用空列表
   - 缓解：在 `build_from_request` 中如果 `tool_definitions` 为空，使用默认工具定义
   - 影响：中（可能影响 ToolInfo 可见性过滤）

3. **`visible_tool_names` 为空**：如果 `AgentTool` 未提供 `visible_tool_names`，`ToolInfo` 可能不可见
   - 缓解：在 `build_from_request` 中如果 `visible_tool_names` 为空，使用默认值
   - 影响：低

### 5.3 性能影响

| 指标 | 变更前 | 变更后 | 影响 |
|------|--------|--------|------|
| Token 开销 | 固定 | 取决于 `injection_mode` | 低（Catalog 模式下几乎无额外开销） |
| 启动延迟 | ~50ms | ~60ms | 低（因为 `build_from_request` 复用 `from_material` 的逻辑） |
| 内存占用 | 基础 | + 1x `PromptConstructionRequest` | 低（Request 是值类型，拷贝成本低） |

---

## 6. 测试策略

### 6.1 单元测试

| 测试模块 | 测试场景 | 预期结果 |
|----------|----------|----------|
| `PromptConstructionRequest` | 默认值 | 所有字段使用默认值，无 panic |
| `PromptConstructionRequest` | 序列化/反序列化 | JSON 往返转换后字段等价 |
| `SystemPromptBuilder` | `build_from_request` with skill | 包含 skill section |
| `SystemPromptBuilder` | `build_from_request` without skill | 不包含 skill section |
| `SystemPromptBuilder` | `build_from_request` with overrides | 模板变量被正确替换 |

### 6.2 集成测试

| 测试模块 | 测试场景 | 预期结果 |
|----------|----------|----------|
| `AgentTool` | 启动子 Agent 并注入 skill | 子 Agent 的 system prompt 包含技能内容 |
| `AgentTool` | 启动子 Agent 不注入 skill | 子 Agent 的 system prompt 不包含技能内容 |
| `AgentTool` | 使用不同的 `injection_mode` | 技能指令的完整性随模式递增 |

### 6.3 边界测试

| 测试场景 | 预期结果 |
|----------|----------|
| 空 `skill_id` | 使用默认 `SkillInjectionMode::Catalog` |
| 无效的 `skill_id` | 忽略空值，不 panic |
| 空的 `context_overrides` | 使用原始模板变量 |

---

## 7. 迁移指南

### 7.1 为现有消费者添加迁移适配层

```rust
// 在 AgentTool 中保留旧的 API 签名，但内部转换为新的接口
async fn run_subagent_with_old_api(
    &self,
    prompt: &str,
    subagent_type: Option<&str>,
    model_override: Option<&str>,
    context: Option<ToolContext>,
) -> Result<(String, u128, Vec<String>)> {
    // 转换为新的请求格式
    let request = self.create_request_from_params(prompt, subagent_type, context.as_ref());
    
    // 使用统一构建管道
    let system_prompt = SystemPromptBuilder::build_from_request(&request, &self.name_overrides, &self.skill_registry);
    
    // 后续逻辑不变
    self.execute_with_system_prompt(system_prompt, request, context).await
}
```

### 7.2 逐步迁移路径

1. **Phase 1**：保留旧代码，新增 `build_from_request` 入口
2. **Phase 2**：`AgentTool` 内部转换为使用 `build_from_request`
3. **Phase 3**：移除 `SkillPromptInjector`，清理代码

---

## 8. 总结

### 8.1 核心理念

**"传递构建指令，而非结果字符串"** — 将 `AgentTool` 从"拼接字符串的操作者"转变为"传递构建指令的分发器"，让 `SystemPromptBuilder` 成为唯一的真相来源。

### 8.2 关键收益

1. **逻辑归一化**：所有技能注入、格式化、模板管理都回到了 `SystemPromptBuilder` 一个地方
2. **消除技术债**：彻底消除了"双轨制"和"架构不一致性"问题
3. **实现真正的"能力下沉"**：`AgentTool` 变成了一个纯粹的调度器，只负责解析意图并驱动 `Builder` 工作，而不再参与具体的字符串拼接逻辑

### 8.3 后续工作

- [ ] 完善 `run_subagent` 方法，直接使用 `build_from_request`
- [ ] 在 `SubagentRuntimeFactory` 中增加 `PromptConstructionRequest` 接口方法
- [ ] 编写单元测试验证 `build_from_request` 在不同配置下的输出一致性
- [ ] 添加基准测试，比较 `build_from_request` 相比 `from_material` 的性能差异
