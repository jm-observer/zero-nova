# Sub Agent 技能提示词动态注入设计文档

**日期**：2026-05-15  
**状态**：设计阶段  
**方案**：Prompt 预编译 + 注入（方案 3）

---

## 1. 项目背景

### 1.1 当前状态

Zero-Nova 已实现：
- **Skill 系统**：`SkillRegistry` 管理技能包，每个 `SkillPackage` 包含 `instructions` 提示词
- **Sub Agent 机制**：`AgentTool` 可以启动子 Agent 执行专门任务
- **动态技能路由**：主 Agent 可以通过 `/skill-name` 激活技能，`SystemPromptBuilder` 动态注入技能提示词

### 1.2 需求目标

实现以下场景：
1. **用户显式指定**：用户明确告诉主 Agent "用技能 X 启动子 Agent 处理任务 Y"
2. **主 Agent 自主决策**：主 Agent 识别任务 → 智能匹配技能 → 启动子 Agent + 注入技能提示词
3. **子 Agent 灵活性**：子 Agent 初始激活指定技能，但可在执行中切换到其他技能
4. **工具策略独立**：技能的 `tool_policy` 仅作建议，不限制子 Agent 的工具访问

---

## 2. 方案概述

### 2.1 核心理念

**Prompt 预编译 + 注入**：在子 Agent 启动前，主 Agent 预先将技能的 `instructions` 编译到 system prompt 中。子 Agent 接收到的是一个"预烘焙"的完整提示词，无需在运行时再解析技能。

### 2.2 数据流

```
用户/主Agent 请求
    ↓
AgentTool.execute() 接收 skill_id 参数
    ↓
SkillMatcher.match_skill() (如果未指定 skill_id)
    ↓
SkillPromptInjector.inject() 从 SkillRegistry 获取 instructions
    ↓
修改 PromptMaterial.agent_prompt (追加技能提示词)
    ↓
SubagentRuntimeFactory.build_runtime() 使用预编译 prompt
    ↓
子 Agent 启动并执行
```

### 2.3 关键组件

| 组件 | 职责 | 状态 |
|------|------|------|
| `SkillPromptInjector` | 将技能 instructions 注入到 agent prompt | 新增 |
| `SkillMatcher` | 智能匹配任务与技能 | 新增 |
| `AgentTool` | 新增 `skill_id` 参数，调用 injector | 修改 |
| `AgentPromptLoader` | 返回的 `PromptMaterial` 包含注入后的 prompt | 修改 |
| `SkillRegistry` | 提供技能查询接口 | 复用 |

---

## 3. 详细设计

### 3.1 AgentTool 参数扩展

**修改文件**：`crates/nova-agent/src/tool/builtin/agent.rs`

**新增参数**：
```rust
{
    "skill_id": {
        "type": "string",
        "description": "技能 ID，用于注入特定技能的提示词到子 Agent。如果指定，子 Agent 将以该技能的上下文启动。"
    }
}
```

**执行逻辑**：
```rust
async fn execute(&self, input: Value, context: Option<ToolContext>) -> Result<ToolOutput> {
    let prompt = input["prompt"].as_str().ok_or(...)?;
    let skill_id = input["skill_id"].as_str(); // 新增
    
    // 如果指定了 skill_id，验证其存在性
    if let Some(sid) = skill_id {
        self.validate_skill_exists(sid)?;
    }
    
    // 传递给 run_subagent
    self.run_subagent(prompt, subagent_type, model_override, skill_id, context).await
}
```

### 3.2 SkillPromptInjector 组件

**新增文件**：`crates/nova-agent/src/skill/injector.rs`

**职责**：
- 从 `SkillRegistry` 获取指定技能的 `instructions`
- 将 instructions 注入到 agent prompt 中
- 处理注入失败的情况

**核心接口**：
```rust
pub struct SkillPromptInjector {
    skill_registry: Arc<SkillRegistry>,
}

impl SkillPromptInjector {
    pub fn new(skill_registry: Arc<SkillRegistry>) -> Self {
        Self { skill_registry }
    }
    
    /// 将技能提示词注入到 agent prompt 中
    pub fn inject(&self, base_prompt: &str, skill_id: &str) -> Result<String> {
        let skill_pkg = self.skill_registry
            .find_package_by_id(skill_id)
            .ok_or_else(|| anyhow::anyhow!("Skill '{}' not found", skill_id))?;
        
        // 注入策略：在 base_prompt 末尾追加技能 instructions
        let injected = format!(
            "{}\n\n---\n\n## Active Skill: {}\n\n{}\n",
            base_prompt,
            skill_pkg.display_name,
            skill_pkg.instructions
        );
        
        Ok(injected)
    }
}
```

**注入格式**：
```
[原始 agent prompt]

---

## Active Skill: [技能显示名]

[技能 instructions 完整内容]
```

### 3.3 SkillMatcher 组件

**新增文件**：`crates/nova-agent/src/skill/matcher.rs`

**职责**：
- 基于 LLM 的语义匹配，将任务描述映射到最合适的技能
- 提供匹配置信度评分
- 处理无匹配或低置信度场景

**核心接口**：
```rust
pub struct SkillMatcher {
    skill_registry: Arc<SkillRegistry>,
    llm_client: Arc<dyn LlmClient>,
}

impl SkillMatcher {
    pub fn new(skill_registry: Arc<SkillRegistry>, llm_client: Arc<dyn LlmClient>) -> Self {
        Self { skill_registry, llm_client }
    }
    
    /// 基于任务描述匹配最合适的技能
    /// 返回 (skill_id, confidence_score)
    pub async fn match_skill(&self, task_description: &str) -> Result<Option<(String, f32)>> {
        let available_skills = self.skill_registry.packages
            .iter()
            .map(|pkg| format!("- {}: {}", pkg.id, pkg.description))
            .collect::<Vec<_>>()
            .join("\n");
        
        let prompt = format!(
            "Given the following task:\n\n\nAvailable skills:\n{}\n\nWhich skill is most appropriate? Return skill_id and confidence (0.0-1.0).",
            task_description,
            available_skills
        );
        
        // LLM 调用逻辑（简化示意）
        // 实际实现需要解析 LLM 响应并提取 skill_id + confidence
        
        Ok(None) // 占位
    }
}
```

**匹配策略**：
- 置信度 >= 0.7：自动匹配
- 0.4 <= 置信度 < 0.7：返回建议但不自动注入
- 置信度 < 0.4：返回 None

---

## 4. 方案三的劣势与权衡

### 4.1 架构一致性问题

**问题描述**：
方案三绕过了现有的 `SystemPromptBuilder` 动态注入机制，在子 Agent 启动前直接修改 `agent_prompt`。这导致：
- **双轨制 prompt 构建**：主 Agent 使用 `SystemPromptBuilder::from_material()` + `SkillInjectionMode`，子 Agent 使用 `SkillPromptInjector::inject()` 直接拼接
- **代码路径分裂**：相同的"技能注入"逻辑在两个地方实现，增加维护成本
- **未来扩展困难**：如果需要统一调整技能注入格式（如添加元数据、版本号），需要同时修改两处

**影响范围**：
- `SystemPromptBuilder` 的 `SkillInjectionMode::ActiveFull` 与 `SkillPromptInjector::inject()` 功能重复
- 新增的 `SkillPromptInjector` 无法复用 `SystemPromptBuilder` 的 section 管理能力

**缓解方案**：
- 短期：在 `SkillPromptInjector` 中明确注释其与 `SystemPromptBuilder` 的关系，避免混淆
- 长期：考虑重构为统一的 prompt 构建管道（方案一或方案二）

### 4.2 子 Agent 技能切换冲突

**问题描述**：
用户需求明确"子 Agent 可在执行中切换到其他技能"（需求 Q2: c），但方案三的预编译策略与此冲突：
- **预编译锁定**：技能 instructions 在子 Agent 启动时已固化到 `agent_prompt` 中
- **切换成本高**：如果子 Agent 需要切换技能，必须：
  1. 检测到切换信号（如 `/other-skill`）
  2. 重新调用 `SkillPromptInjector::inject()` 生成新 prompt
  3. 替换 `TurnContext.system_prompt`（需要修改 runtime 逻辑）
- **历史污染**：旧技能的 instructions 已在历史消息中，可能干扰新技能执行

**影响范围**：
- 子 Agent 的 `decide_active_skill()` 逻辑需要额外处理"prompt 已预编译"的情况
- 技能切换时需要重新裁剪历史（`trim_history()`），增加复杂度

**缓解方案**：
- 方案 A：限制子 Agent 不支持技能切换（与需求冲突）
- 方案 B：在子 Agent 检测到技能切换时，触发"prompt 重编译"流程（需要新增 `AgentRuntime::recompile_prompt()` 方法）
- 方案 C：改用方案二（运行时注入），天然支持技能切换

### 4.3 动态技能路由实现复杂

**问题描述**：
方案三要求在子 Agent 启动前确定 `skill_id`，但 LLM 语义匹配（需求 Q3: b）是异步且不确定的：
- **匹配时机尴尬**：`AgentTool::execute()` 中需要调用 `SkillMatcher::match_skill()`，但此时尚未进入 Agent 执行循环
- **错误传播路径长**：如果匹配失败（需求 Q7: a），错误需要从 `AgentTool` → 主 Agent → 用户，跨越多个层级
- **无法利用上下文**：LLM 匹配只能基于 `prompt` 参数，无法访问主 Agent 的历史消息或环境信息

**影响范围**：
- `SkillMatcher` 需要独立的 LLM 调用，增加延迟和 token 消耗
- 匹配逻辑与 Agent 执行逻辑解耦，难以共享上下文

**缓解方案**：
- 短期：在 `AgentTool` 中缓存匹配结果，避免重复调用
- 长期：考虑将匹配逻辑下沉到 `AgentRuntime::prepare_turn()`，利用现有上下文

### 4.4 Token 与上下文窗口压力

**问题描述**：
方案三采用"完整注入"策略（需求 Q8: a），将技能的全部 `instructions` 注入到 system prompt 中：
- **Token 消耗高**：每个技能的 instructions 通常 500-2000 tokens，完整注入会显著增加每轮请求的 token 消耗
- **上下文窗口挤占**：预编译的 prompt 占用固定空间，压缩历史消息的可用窗口
- **无优化空间**：与方案二的"按需加载"相比，无法根据执行阶段动态调整注入内容

**影响范围**：
- 长对话场景下，子 Agent 的历史裁剪会更激进（`HistoryTrimmer` 提前触发）
- 成本增加：每轮请求多消耗 500-2000 tokens

**数据示例**：
假设技能 instructions 平均 1000 tokens，子 Agent 执行 10 轮对话：
- 方案三：10 轮 × 1000 tokens = 10,000 tokens（固定成本）
- 方案二：仅首轮注入 1000 tokens，后续按需追加（可能 2000-3000 tokens 总计）

**缓解方案**：
- 方案 A：接受成本，作为"简单实现"的代价
- 方案 B：后续优化为"分段注入"（如仅注入当前相关的 section）
- 方案 C：改用方案二

### 4.5 维护负担与技术债

**问题描述**：
方案三引入了与现有架构平行的新组件（`SkillPromptInjector`、`SkillMatcher`），增加维护负担：
- **重复逻辑**：`SkillPromptInjector::inject()` 与 `SystemPromptBuilder::from_material()` 的技能注入逻辑重复
- **测试覆盖**：需要为新组件编写独立的单元测试和集成测试
- **文档同步**：需要在多处文档中说明"主 Agent 用方案 A，子 Agent 用方案 B"

**影响范围**：
- 新增代码量：约 300-500 行（`SkillPromptInjector` + `SkillMatcher` + 测试）
- 未来重构成本：如果需要统一为方案一或方案二，需要移除这些组件

**缓解方案**：
- 在代码注释中明确标注"临时方案"，降低未来重构的心理负担
- 编写充分的测试，确保重构时不遗漏边界情况

### 4.6 劣势总结表

| 劣势 | 严重程度 | 影响范围 | 缓解难度 |
|------|---------|---------|---------|
| 架构一致性问题 | 中 | 代码维护 | 中（需重构） |
| 子 Agent 技能切换冲突 | 高 | 功能完整性 | 高（需新增逻辑） |
| 动态技能路由复杂 | 中 | 实现复杂度 | 中（需优化匹配） |
| Token 与上下文压力 | 低-中 | 成本与性能 | 低（可接受） |
| 维护负担 | 低 | 长期维护 | 低（文档化） |

**建议**：
- 如果"子 Agent 技能切换"是核心需求，建议重新评估方案二（运行时注入）
- 如果可以接受"子 Agent 启动时锁定技能"，方案三可行，但需明确文档化限制

---

## 5. 测试策略

### 5.1 单元测试

#### 5.1.1 SkillPromptInjector 测试

**文件**：`crates/nova-agent/src/skill/injector.rs`

**测试用例**：
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::{SkillPackage, SkillRegistry, ToolPolicy};
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn inject_appends_skill_instructions_to_base_prompt() {
        let mut registry = SkillRegistry::new();
        registry.extend_packages(vec![SkillPackage {
            id: "test-skill".to_string(),
            slug: "test-skill".to_string(),
            display_name: "Test Skill".to_string(),
            description: "A test skill".to_string(),
            instructions: "Do X, then Y, then Z.".to_string(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec![],
            examples: vec![],
            source_path: PathBuf::from("test"),
            compat_mode: false,
        }]).unwrap();

        let injector = SkillPromptInjector::new(Arc::new(registry));
        let base = "You are a helpful assistant.";
        let result = injector.inject(base, "test-skill").unwrap();

        assert!(result.contains("You are a helpful assistant."));
        assert!(result.contains("## Active Skill: Test Skill"));
        assert!(result.contains("Do X, then Y, then Z."));
    }

    #[test]
    fn inject_returns_error_for_nonexistent_skill() {
        let registry = SkillRegistry::new();
        let injector = SkillPromptInjector::new(Arc::new(registry));
        let result = injector.inject("base", "nonexistent");

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn inject_preserves_base_prompt_formatting() {
        let mut registry = SkillRegistry::new();
        registry.extend_packages(vec![SkillPackage {
            id: "skill".to_string(),
            slug: "skill".to_string(),
            display_name: "Skill".to_string(),
            description: "desc".to_string(),
            instructions: "instr".to_string(),
            tool_policy: ToolPolicy::InheritAll,
            sticky: false,
            aliases: vec![],
            examples: vec![],
            source_path: PathBuf::from("s"),
            compat_mode: false,
        }]).unwrap();

        let injector = SkillPromptInjector::new(Arc::new(registry));
        let base = "Line 1\n\nLine 2\n\n## Section";
        let result = injector.inject(base, "skill").unwrap();

        assert!(result.starts_with("Line 1\n\nLine 2\n\n## Section"));
    }
}
```

#### 5.1.2 SkillMatcher 测试

**文件**：`crates/nova-agent/src/skill/matcher.rs`

**测试用例**：
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn match_skill_returns_high_confidence_for_exact_match() {
        // Mock LLM client 返回高置信度匹配
        // 验证 confidence >= 0.7 时返回 Some((skill_id, score))
    }

    #[tokio::test]
    async fn match_skill_returns_none_for_low_confidence() {
        // Mock LLM client 返回低置信度
        // 验证 confidence < 0.4 时返回 None
    }

    #[tokio::test]
    async fn match_skill_handles_empty_skill_registry() {
        // 空 registry 应返回 None
    }
}
```

#### 5.1.3 AgentTool 参数验证测试

**文件**：`crates/nova-agent/src/tool/builtin/agent.rs`

**测试用例**：
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn execute_validates_skill_id_exists() {
        // 传入不存在的 skill_id，验证返回错误
    }

    #[tokio::test]
    async fn execute_accepts_valid_skill_id() {
        // 传入存在的 skill_id，验证不报错
    }

    #[tokio::test]
    async fn execute_allows_missing_skill_id() {
        // skill_id 为 None 时应正常执行
    }
}
```

### 5.2 集成测试

#### 5.2.1 端到端注入测试

**文件**：`crates/nova-agent/tests/subagent_skill_injection.rs`

**测试场景 1：用户显式指定技能**
```rust
#[tokio::test]
async fn subagent_receives_injected_skill_prompt_when_skill_id_specified() {
    // 1. 创建包含技能的 SkillRegistry
    // 2. 主 Agent 调用 AgentTool，传入 skill_id="test-skill"
    // 3. 验证子 Agent 的 system prompt 包含技能 instructions
    // 4. 验证子 Agent 可以访问技能指定的工具
}
```

**测试场景 2：主 Agent 自动匹配技能**
```rust
#[tokio::test]
async fn subagent_receives_matched_skill_prompt_when_auto_matched() {
    // 1. 主 Agent 接收任务描述
    // 2. SkillMatcher 自动匹配到技能
    // 3. AgentTool 自动传入匹配的 skill_id
    // 4. 验证子 Agent 的 system prompt 包含匹配技能的 instructions
}
```

**测试场景 3：技能不存在时返回错误**
```rust
#[tokio::test]
async fn subagent_returns_error_when_skill_not_found() {
    // 1. AgentTool 传入不存在的 skill_id
    // 2. 验证返回错误，错误信息包含 "Skill 'xxx' not found"
    // 3. 验证子 Agent 未启动
}
```

**测试场景 4：注入后的 prompt 格式正确**
```rust
#[tokio::test]
async fn injected_prompt_has_correct_format() {
    // 1. 启动子 Agent 并注入技能
    // 2. 捕获子 Agent 的 system prompt
    // 3. 验证格式：base_prompt + "---" + "## Active Skill: [name]" + instructions
    // 4. 验证没有多余的空行或格式错误
}
```

**测试场景 5：工具策略生效**
```rust
#[tokio::test]
async fn subagent_respects_skill_tool_policy() {
    // 1. 创建技能，tool_policy = AllowList(["Bash", "Read"])
    // 2. 启动子 Agent 并注入该技能
    // 3. 验证子 Agent 的 tool_definitions 仅包含 Bash 和 Read
    // 4. 验证子 Agent 无法调用 Write 工具
}
```

#### 5.2.2 子 Agent 技能切换测试

**测试场景 6：子 Agent 检测到技能切换信号**
```rust
#[tokio::test]
async fn subagent_detects_skill_switch_signal() {
    // 1. 子 Agent 以 skill-a 启动
    // 2. 子 Agent 接收输入 "/skill-b do something"
    // 3. 验证子 Agent 的 active_skill 切换到 skill-b
    // 4. 验证后续 turn 的 system prompt 包含 skill-b 的 instructions
}
```

**测试场景 7：技能切换后历史裁剪**
```rust
#[tokio::test]
async fn subagent_trims_history_after_skill_switch() {
    // 1. 子 Agent 执行 5 轮对话（skill-a）
    // 2. 切换到 skill-b
    // 3. 验证历史消息被裁剪（移除 skill-a 相关内容）
    // 4. 验证新 turn 的上下文窗口未被旧技能污染
}
```

### 5.3 性能测试

**测试场景 8：Token 消耗对比**
```rust
#[tokio::test]
async fn measure_token_overhead_of_full_injection() {
    // 1. 创建包含 1000 tokens instructions 的技能
    // 2. 启动子 Agent 并注入技能
    // 3. 执行 10 轮对话
    // 4. 统计总 token 消耗
    // 5. 与"无注入"基线对比，验证增量在预期范围内（< 20%）
}
```

### 5.4 边界测试

**测试场景 9：空 instructions 技能**
```rust
#[tokio::test]
async fn subagent_handles_empty_instructions() {
    // 1. 创建 instructions = "" 的技能
    // 2. 注入到子 Agent
    // 3. 验证不崩溃，system prompt 仅包含 base_prompt
}
```

**测试场景 10：超长 instructions**
```rust
#[tokio::test]
async fn subagent_handles_large_instructions() {
    // 1. 创建 instructions 长度 > 10,000 chars 的技能
    // 2. 注入到子 Agent
    // 3. 验证注入成功，不截断
    // 4. 验证子 Agent 可以正常执行
}
```

---

## 6. 实施计划

### 6.1 文件变更清单

| 文件路径 | 变更类型 | 说明 |
|---------|---------|------|
| `crates/nova-agent/src/skill/injector.rs` | 新增 | `SkillPromptInjector` 组件 |
| `crates/nova-agent/src/skill/matcher.rs` | 新增 | `SkillMatcher` 组件（LLM 语义匹配） |
| `crates/nova-agent/src/skill/mod.rs` | 修改 | 导出 `injector` 和 `matcher` 模块 |
| `crates/nova-agent/src/tool/builtin/agent.rs` | 修改 | 新增 `skill_id` 参数，调用 injector |
| `crates/nova-agent/src/agent/runtime.rs` | 修改 | 支持子 Agent 技能切换时的 prompt 重编译 |
| `crates/nova-agent/tests/subagent_skill_injection.rs` | 新增 | 集成测试 |

### 6.2 实施步骤

#### Phase 1：核心组件实现（2-3 天）

1. **实现 `SkillPromptInjector`**
   - 编写 `inject()` 方法
   - 添加单元测试（测试场景：正常注入、技能不存在、格式保留）
   - 验证注入格式符合规范

2. **扩展 `AgentTool` 参数**
   - 在 `AgentToolInput` 中新增 `skill_id: Option<String>`
   - 修改 `execute()` 方法，验证 `skill_id` 存在性
   - 在 `run_subagent()` 中调用 `SkillPromptInjector::inject()`

3. **修改 `AgentPromptLoader`**
   - 在 `load_agent_material()` 中接收 `skill_id` 参数
   - 如果 `skill_id` 存在，调用 injector 修改 `agent_prompt`
   - 返回修改后的 `PromptMaterial`

#### Phase 2：智能匹配实现（3-4 天）

4. **实现 `SkillMatcher`**
   - 设计 LLM prompt 模板（输入：任务描述 + 技能列表，输出：skill_id + confidence）
   - 实现 `match_skill()` 方法
   - 添加置信度阈值逻辑（>= 0.7 自动匹配，< 0.4 返回 None）
   - 编写单元测试（Mock LLM client）

5. **集成到 `AgentTool`**
   - 在 `execute()` 中，如果 `skill_id` 为 None，调用 `SkillMatcher::match_skill()`
   - 根据匹配结果决定是否注入技能
   - 处理匹配失败场景（返回错误或降级为无技能执行）

#### Phase 3：技能切换支持（2-3 天）

6. **扩展 `AgentRuntime::decide_active_skill()`**
   - 检测子 Agent 输入中的技能切换信号（如 `/other-skill`）
   - 如果检测到切换，返回新的 `ActiveSkillState`

7. **实现 prompt 重编译**
   - 在 `run_turn_with_context()` 中，检测 `active_skill` 是否变化
   - 如果变化，调用 `SkillPromptInjector::inject()` 重新生成 system prompt
   - 触发历史裁剪（`trim_history()`）

#### Phase 4：测试与文档（2-3 天）

8. **编写集成测试**
   - 实现 5.2 节中的所有测试场景
   - 验证端到端流程（用户指定 → 注入 → 执行 → 切换）

9. **性能与边界测试**
   - 实现 5.3 和 5.4 节中的测试场景
   - 收集 token 消耗数据，验证在可接受范围内

10. **更新文档**
    - 在 `docs/architecture/` 中添加"子 Agent 技能注入"章节
    - 更新 `AgentTool` 的使用文档，说明 `skill_id` 参数
    - 在 README 中添加示例

### 6.3 风险与依赖

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| LLM 匹配不准确 | 自动匹配失败率高 | 调整置信度阈值，提供手动指定兜底 |
| 技能切换逻辑复杂 | 实现延期 | Phase 3 可独立延后，不阻塞 Phase 1/2 |
| Token 消耗超预期 | 成本增加 | 监控实际消耗，必要时优化为分段注入 |

---

## 7. 迁移与兼容性

### 7.1 向后兼容

**现有功能不受影响**：
- 主 Agent 的技能系统（`SystemPromptBuilder` + `SkillInjectionMode`）保持不变
- 未指定 `skill_id` 的 `AgentTool` 调用行为不变（子 Agent 无技能注入）
- 现有测试无需修改

**新增功能为可选**：
- `skill_id` 参数为 `Option<String>`，默认 `None`
- 用户可以逐步采用新功能，无需一次性迁移

### 7.2 配置变更

**无需配置文件变更**：
- 技能定义（`SkillPackage`）无需修改
- `.nova/config.toml` 无需新增配置项

**可选配置（未来扩展）**：
```toml
[skill_matching]
# LLM 匹配的置信度阈值
confidence_threshold = 0.7
# 是否启用自动匹配
auto_match_enabled = true
```

### 7.3 已知限制

1. **子 Agent 技能切换的性能开销**
   - 每次切换需要重新生成 system prompt 并裁剪历史
   - 建议：尽量在子 Agent 启动时确定技能，避免频繁切换

2. **LLM 匹配的延迟**
   - 语义匹配需要额外的 LLM 调用（约 1-2 秒）
   - 建议：对延迟敏感的场景使用显式指定 `skill_id`

3. **与方案二的差异**
   - 方案三无法在子 Agent 执行中动态调整注入内容
   - 如果未来需要"按需加载"能力，需要重构为方案二

### 7.4 未来优化方向

1. **分段注入**
   - 将技能 instructions 拆分为多个 section
   - 根据执行阶段动态注入相关 section，减少 token 消耗

2. **缓存匹配结果**
   - 对相同任务描述的匹配结果进行缓存（基于 hash）
   - 避免重复调用 LLM

3. **统一 prompt 构建管道**
   - 长期目标：将 `SkillPromptInjector` 合并到 `SystemPromptBuilder`
   - 实现主 Agent 和子 Agent 的统一注入逻辑

---

## 8. 总结

### 8.1 方案三的核心优势

- **实现简单**：在子 Agent 启动前预编译 prompt，无需修改 runtime 核心逻辑
- **隔离性好**：新增组件（`SkillPromptInjector`、`SkillMatcher`）与现有系统解耦
- **快速验证**：可以在 2-3 周内完成 MVP，快速验证需求

### 8.2 方案三的核心劣势

- **架构一致性问题**：与现有 `SystemPromptBuilder` 形成双轨制
- **技能切换冲突**：预编译策略与"子 Agent 可切换技能"需求存在矛盾
- **Token 消耗高**：完整注入策略增加每轮请求的 token 成本
- **维护负担**：引入平行组件，增加长期维护复杂度

### 8.3 决策建议

**适用场景**：
- 子 Agent 启动时技能已确定，执行中不需要切换
- 可以接受 10-20% 的 token 消耗增量
- 需要快速上线，后续可以重构

**不适用场景**：
- 子 Agent 需要频繁切换技能
- 对 token 成本极度敏感
- 追求架构一致性和长期可维护性

**建议**：
如果"子 Agent 技能切换"是核心需求，建议重新评估**方案二（运行时注入）**，虽然实现复杂度更高，但能更好地支持动态切换和按需加载。

如果可以接受"子 Agent 启动时锁定技能"的限制，方案三可以作为 MVP 快速验证需求，后续根据实际使用情况决定是否重构。

---

**文档版本**：v1.0  
**最后更新**：2026-05-15  
**状态**：待用户审核
