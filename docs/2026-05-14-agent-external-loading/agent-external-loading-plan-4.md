# Plan 4: 调用链统一

## 前置依赖

Plan 2、Plan 3

## 本次目标

统一 app/bootstrap、conversation turn、workspace inspect、Agent tool 动态创建中的加载路径，确保 skill 和 prompt 外部资源只通过统一 loader/factory 进入 `nova-agent`。

完成后，运行时调用点不再直接读取 skill 或 prompt 文件，也不重复实现 prompt fallback 规则。

## 当前调用点与目标对照

| 调用点 | 当前实现 | 目标实现 |
|--------|---------|---------|
| `bootstrap.rs:37` | `skill_registry.load_from_dir_async()` | Plan 2 已迁移 |
| `bootstrap.rs:118` | 调用本地 `load_agent_prompt` | `PromptMaterialLoader::load_agent_prompt` |
| `bootstrap.rs:146` | `SystemPromptBuilder::from_config_async` | `SystemPromptBuilder::from_material` |
| `conversation_service.rs:300-304` | 直接调用 `load_project_context_with_config_async` | `PromptMaterialLoader::load_turn_material` |
| `conversation_service.rs:359` | 直接调用 `load_developer_project_prompt_async` | `PromptMaterialLoader::load_turn_material` |
| `conversation_service.rs:383` | `runtime.prepare_turn` 使用 `PromptConfig` | 改为传入 `PromptMaterial` + `TurnPromptMaterial` |
| `agent_workspace_service.rs:127` | 调用本地 `load_agent_prompt_for_reload` | `PromptMaterialLoader::load_agent_prompt` |
| `agent_workspace_service.rs:148-155` | 直接调用 `load_developer_project_prompt_async` | `PromptMaterialLoader::load_turn_material` |
| `agent_workspace_service.rs:160` | `SystemPromptBuilder::from_config_async` | `SystemPromptBuilder::from_material` |
| `tool/builtin/agent.rs:244` | 调用 `self.load_agent_prompt(spec)` | `PromptMaterialLoader::load_agent_prompt` |
| `tool/builtin/agent.rs:261-272` | 直接调用 `load_project_context_with_config_async` + `load_developer_project_prompt_async` | `PromptMaterialLoader` |
| `tool/builtin/agent.rs:329` | `runtime.prepare_turn` 使用 `PromptConfig` | 改为传入 material |

## 涉及文件

| 文件 | 变更类型 | 说明 |
|------|---------|------|
| `crates/nova-agent/src/app/bootstrap.rs` | 重写 | 使用 `PromptMaterialLoader` + `from_material` |
| `crates/nova-agent/src/app/conversation_service.rs` | 重写 turn 构建 | 使用 `PromptMaterialLoader::load_turn_material` + `from_material` |
| `crates/nova-agent/src/app/agent_workspace_service.rs` | 重写 reload | 使用 `PromptMaterialLoader` + `from_material` |
| `crates/nova-agent/src/tool/builtin/agent.rs` | 重写 prompt 加载 | 使用 `PromptMaterialLoader` |
| `crates/nova-agent/src/agent/runtime.rs` | 可能调整 | `prepare_turn` 签名可能需要适配 material 模型 |
| app 层 `PromptMaterialLoader` | 复用 | Plan 3 已建立 |

## 详细设计

### 统一服务

外层提供两个协作对象：

```text
PromptMaterialLoader        (Plan 3 已建立)
    -> load_agent_prompt    (agent base prompt)
    -> load_agent_material  (启动期 PromptMaterial)
    -> load_turn_material   (turn 级 TurnPromptMaterial)

AgentDescriptorFactory      (本 Plan 新建或内联到 bootstrap)
    -> build AgentDescriptor from AgentSpec + PromptMaterial + provider binding
```

### 4.1 build_application 调用链

目标流程：

```rust
// bootstrap.rs（简化）
pub async fn build_application(config: AppConfig) -> Result<Arc<dyn AgentApplication>> {
    // Skill loading (Plan 2)
    let skill_registry = load_and_create_skill_registry(&config).await;

    // Environment
    let env_snapshot = EnvironmentSnapshot::collect(&config.config_dir, None).await;

    // Prompt loader (Plan 3)
    let prompt_loader = PromptMaterialLoader::from_config(&config);

    // Agent descriptors
    let catalog_text = build_agent_catalog_section(&config.gateway.agents, &primary_id);
    let mut agents = Vec::new();
    for agent_spec in &config.gateway.agents {
        let binding = config.resolve_agent_binding(agent_spec)?;
        let template_vars = build_initial_template_vars(agent_spec);

        // 通过 loader 加载 prompt
        let material = prompt_loader.load_agent_material(
            agent_spec,
            Some(env_snapshot.clone()),
            if !catalog_text.is_empty() { Some(catalog_text.clone()) } else { None },
            template_vars.clone(),
        ).await?;

        // 构建启动期 system prompt
        let system_prompt = SystemPromptBuilder::from_material(
            &material,
            &TurnPromptMaterial::default(),
            &skill_registry,
        ).build();

        agents.push(AgentDescriptor {
            id: agent_spec.id.clone(),
            system_prompt_template: system_prompt,
            system_prompt_base: material.agent_prompt.clone(),
            initial_template_vars: template_vars,
            // ... 其余字段
        });
    }
    // ... 注册 agents
}
```

**关键变化**：`build_application` 不直接调用 `tokio::fs::read_to_string` 读取 agent prompt。

### 4.2 conversation_service 调用链

每轮 turn 构建 prompt 时：

```rust
// conversation_service.rs（简化）
async fn execute_agent_turn(&self, ...) -> Result<TurnResult> {
    let prompt_loader = PromptMaterialLoader::from_config(&self.app_config);

    // 构建 PromptMaterial（可复用 agent descriptor 中已有的 base prompt）
    let material = PromptMaterial {
        agent_id: agent_descriptor.id.clone(),
        agent_prompt: system_prompt_base, // 从 descriptor 获取
        agent_catalog: None, // turn 级不需要重建 catalog
        environment_snapshot: Some(env),
        initial_template_vars: agent_descriptor.initial_template_vars.clone(),
        skill_injection_mode: ...,
        project_instruction_profile: ...,
        tool_guidance: ...,
    };

    // 通过 loader 加载 turn 素材
    let turn_material = prompt_loader.load_turn_material(
        project_dir.as_deref(),
        Some("idle"), // workflow_stage
        active_skill_id,
        turn_vars,
        agent_descriptor.enable_project_developer_prompt,
    ).await?;

    // 纯内容构建 prompt
    let builder = SystemPromptBuilder::from_material(&material, &turn_material, &skill_registry);

    // 后续 prepare_turn 使用 builder
    // ...
}
```

**关键变化**：conversation service 不直接调用 `load_developer_project_prompt_async`、`load_project_context_with_config_async`。

### 4.3 agent_workspace_service 调用链

workspace inspect/reload 使用 `PromptMaterialLoader`：

```diff
 // agent_workspace_service.rs reload_session_system_prompt
-    let prompt_base = load_agent_prompt_for_reload(&agent_spec, &reloaded_config).await?;
+    let prompt_loader = PromptMaterialLoader::from_config(&reloaded_config);
+    let prompt_base = prompt_loader.load_agent_prompt(&agent_spec).await?;
     // ... 后续构建 PromptMaterial + from_material
```

**关键变化**：删除 `load_agent_prompt_for_reload` 函数（L620-642）。

### 4.4 tool/builtin/agent 调用链

Agent tool 动态创建 agent 时：

```diff
 // tool/builtin/agent.rs run_subagent
-    let mut prompt_config = PromptConfig::new(
-        spec.id.clone(),
-        self.load_agent_prompt(spec).await?,
-        PromptLoadContext { ... },
-    );
+    let prompt_loader = PromptMaterialLoader::from_config(&self.config);
+    let material = prompt_loader.load_agent_material(
+        spec,
+        Some(environment.clone()),
+        None,
+        prompt_template_vars.clone(),
+    ).await?;
+    let turn_material = prompt_loader.load_turn_material(
+        project_dir.as_deref(),
+        Some("idle"),
+        None,
+        HashMap::new(),
+        spec.enable_project_developer_prompt,
+    ).await?;
```

**关键变化**：
- 删除 `AgentTool::load_agent_prompt` 方法（L377-397）。
- 删除 `AgentTool` 中直接调用 `load_project_context_with_config_async` 和 `load_developer_project_prompt_async` 的代码。

### 4.5 `prepare_turn` 签名调整

当前 `AgentRuntime::prepare_turn` 接收 `&PromptConfig`。迁移后需要改为接收 material 或 builder：

**选项 A**：`prepare_turn` 接收 `&PromptMaterial` + `&TurnPromptMaterial`，内部调用 `from_material`。
**选项 B**：调用方先构建 `SystemPromptBuilder`，`prepare_turn` 接收已构建的 builder 或 system prompt string。

**推荐选项 B**：调用方构建 prompt，传入 system prompt string。这样 `AgentRuntime` 不感知 material 模型，更解耦。

### 4.6 日志边界

- 文件不存在、读取失败、解析失败：`PromptMaterialLoader` 记录或返回带路径上下文的错误。
- 调用方只记录业务动作失败，例如"创建 agent 失败"。
- agent prompt section 是否注入、section 大小、diagnostics 仍由 `nova-agent` 记录，但不包含文件读取路径。
- 同一错误只在 `PromptMaterialLoader` 层记录一次，避免 bootstrap、builder、service 多层重复打印。

### 4.7 错误语义统一

| 场景 | 统一行为 |
|------|---------|
| 显式 `prompt_file` 读取失败 | 返回错误（包含 agent id 和路径） |
| 默认 `agent-{id}.md` 不存在 | warn 后降级为空 prompt |
| `prompt_file` 与 `prompt_inline` 同时配置 | 返回配置错误 |
| developer prompt 文件读取失败 | 按现有语义 warn-skip 继续 |
| project context 不存在 | 降级为 `None` |
| workflow stage 为 `idle` | 不加载 workflow prompt |

## 迁移步骤

1. 在 `AgentRuntime::prepare_turn` 或调用链适配 material 模型（选项 B：传入 system prompt string）。
2. 迁移 `build_application`：使用 `PromptMaterialLoader` + `from_material` 替代 `load_agent_prompt` + `from_config_async`。
3. 迁移 `conversation_service`：使用 `load_turn_material` + `from_material` 替代直接 IO。
4. 迁移 `agent_workspace_service`：使用 `PromptMaterialLoader::load_agent_prompt` 替代 `load_agent_prompt_for_reload`。
5. 迁移 `tool/builtin/agent`：使用 `PromptMaterialLoader` 替代 `AgentTool::load_agent_prompt`。
6. 删除各调用点的重复 helper 函数：
   - `bootstrap::load_agent_prompt`（L225-262）
   - `agent_workspace_service::load_agent_prompt_for_reload`（L620-642）
   - `AgentTool::load_agent_prompt`（L377-397）
7. 增加跨调用点一致性测试。

## 测试案例

- 正常路径：bootstrap、workspace inspect、Agent tool 针对同一个 agent spec 生成一致的 base prompt。
- 正常路径：conversation turn 使用 project dir 后，developer prompt 和 project context 正确注入。
- 边界条件：agent 使用 `prompt_inline` 时不触发 prompt file 读取。
- 边界条件：默认 prompt 文件缺失时，bootstrap 和 workspace inspect 降级行为一致（均返回空字符串）。
- 异常场景：显式 `prompt_file` 不存在时，bootstrap、workspace inspect、Agent tool 返回一致错误信息。
- 异常场景：developer prompt 文件读取失败时，所有 turn 构建路径遵循同一策略（warn-skip）。
- 回归测试：迁移后 `build_application` 能正常启动并注册 agent。

## 验收标准

- `app/bootstrap.rs` 不直接读取 agent prompt 文件（无 `tokio::fs::read_to_string` 读 prompt）。
- `conversation_service.rs` 不直接调用 `load_developer_project_prompt_async`、`load_project_context_with_config_async`。
- `agent_workspace_service.rs` 中 `load_agent_prompt_for_reload` 已删除。
- `tool/builtin/agent.rs` 中 `AgentTool::load_agent_prompt` 已删除。
- 同一 agent spec 在不同调用点下的 prompt 解析结果一致。
- 所有调用点使用同一个 `PromptMaterialLoader` 实例或从同一 config 构建。
