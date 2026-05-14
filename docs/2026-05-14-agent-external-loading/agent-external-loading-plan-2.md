# Plan 2: Skill Loader 与 nova-agent 脱钩

## 前置依赖

Plan 1

## 本次目标

完成 skill discovery/parse 与 `nova-agent` 的依赖脱钩。`nova-skill-loader` 继续负责目录扫描和格式解析，但运行时调用链必须由外层调用 loader，再把 `SkillPackage` 注入 `SkillRegistry`。

### 当前耦合点

| 文件 | 耦合方式 | 代码位置 |
|------|---------|---------|
| `Cargo.toml` | `nova-skill-loader = { workspace = true }` | L29 |
| `registry/discovery.rs` | `use nova_skill_loader::{load_skills_from_dir, load_skills_from_dir_async}` | L3 |
| `registry/parser.rs` | `use nova_skill_loader::{load_single_skill, load_single_skill_async, LoadedSkill, ...}` + `From<LoadedSkillPackage>` impl | L5-6, L58-84 |
| `app/bootstrap.rs` | `skill_registry.load_from_dir_async(&skill_dir)` | L37 |

## 涉及文件

| 文件 | 变更类型 | 说明 |
|------|---------|------|
| `crates/nova-skill-loader/src/lib.rs` | 无改动 | API 保持不变 |
| `crates/nova-agent/Cargo.toml` | 删除依赖 | 移除 `nova-skill-loader` |
| `crates/nova-agent/src/skill/registry.rs` | 审查 | 确认 `from_packages` 已就绪（Plan 1） |
| `crates/nova-agent/src/skill/registry/discovery.rs` | 删除 | 整个文件删除 |
| `crates/nova-agent/src/skill/registry/parser.rs` | 删除 | 整个文件删除（`From` impl 迁出到 app adapter） |
| `crates/nova-agent/src/app/bootstrap.rs` | 重写加载链 | 改为外层调用 loader + 转换 + 注入 |

## 详细设计

### 最终依赖方向

```text
app/bootstrap
    -> nova_skill_loader::load_skills_from_dir_async(config.skills_dir())
    -> app adapter: LoadedSkill -> SkillPackage (转换)
    -> SkillRegistry::from_packages(packages)  (注入)
    -> register_builtin_tools(..., Arc<SkillRegistry>, ...)
```

`nova-agent` 不依赖 `nova-skill-loader`，也不暴露从目录加载 skill 的运行时 API。

### nova-skill-loader 职责

loader 负责：

- 判断 skill 根目录是否存在，不存在时返回空列表。
- 递归扫描 skill 目录。
- 优先解析 `skill.toml`，兼容解析 `SKILL.md`。
- 返回中立 loaded model（`LoadedSkillPackage` / `LoadedCompatSkill` / `LoadedSkill`）。
- 保留 source path，供外层日志和诊断使用。
- 将格式解析失败作为 error 返回，不在 agent 内吞错。

loader 不负责：

- 生成 agent prompt。
- 派生 tool policy 的 runtime enable/deferred 视图。
- 决定 warn-skip 还是 fail-fast。
- 注册到 `SkillRegistry`。

### app/bootstrap 职责

bootstrap 或外层 adapter 负责：

- 从 `AppConfig` 获取 `skills_dir`。
- 调用 `nova-skill-loader`。
- 根据配置或固定策略处理错误：启动期建议 warn 后注入空 registry，测试或严格模式可 fail-fast。
- 把 loaded model 转换成 `SkillPackage`。
- 调用 `SkillRegistry::from_packages` 注入。
- 记录加载成功数量、失败上下文和目录路径。

### nova-agent 职责

`SkillRegistry` 只负责消费（已有方法在 `registry/filter.rs`）：

- `find_by_slug` / `find_by_name` / `find_by_alias` 查询
- `match_skill_by_input`
- `generate_catalog_prompt`
- `generate_contextual_prompt`
- `generate_full_prompt`
- `policy_from_skill` / `get_tool_view` 派生

`registry/discovery.rs`、`registry/parser.rs` 中对 `nova_skill_loader` 的引用应**整体删除**，不保留兼容 API。

### 类型转换边界

当前 `parser.rs` 中的 `From<LoadedSkillPackage> for SkillPackage` 和 `From<LoadedToolPolicy> for ToolPolicy` 必须从 `nova-agent` 迁出。

转换函数建议放在 app 层新增模块 `app/skill_adapter.rs`：

```rust
// crates/nova-agent/src/app/skill_adapter.rs

use nova_skill_loader::{LoadedSkill, LoadedSkillPackage, LoadedToolPolicy};
use crate::skill::types::{SkillPackage, ToolPolicy};

/// 将 nova-skill-loader 的 LoadedSkill 转换为 agent 内部 SkillPackage。
pub fn convert_loaded_skills(loaded: Vec<LoadedSkill>) -> Vec<SkillPackage> {
    loaded.into_iter().filter_map(|skill| {
        match skill {
            LoadedSkill::Package(pkg) => Some(convert_package(pkg)),
            LoadedSkill::Compat { package, .. } => Some(convert_package(package)),
        }
    }).collect()
}

fn convert_package(pkg: LoadedSkillPackage) -> SkillPackage {
    SkillPackage {
        id: pkg.id,
        slug: pkg.slug,
        display_name: pkg.display_name,
        description: pkg.description,
        instructions: pkg.instructions,
        tool_policy: convert_tool_policy(pkg.tool_policy),
        sticky: pkg.sticky,
        aliases: pkg.aliases,
        examples: pkg.examples,
        source_path: pkg.source_path,
        compat_mode: pkg.compat_mode,
    }
}

fn convert_tool_policy(policy: LoadedToolPolicy) -> ToolPolicy {
    match policy {
        LoadedToolPolicy::InheritAll => ToolPolicy::InheritAll,
        LoadedToolPolicy::AllowList(tools) => ToolPolicy::AllowList(tools),
        LoadedToolPolicy::AllowListWithDeferred(tools) => ToolPolicy::AllowListWithDeferred(tools),
    }
}
```

> **注意**：`app/skill_adapter.rs` 位于 `nova-agent` 的 app 层而非 skill 核心层。这样 `nova-agent` 仍需在 `Cargo.toml` 中依赖 `nova-skill-loader`——除非把 adapter 移到外部 crate。
>
> **决策**：由于当前只有 `app/bootstrap.rs` 调用 loader，且 adapter 逻辑很薄，可先保留 `nova-skill-loader` 作为 **仅 app 层使用的依赖**，通过代码约定（而非 Cargo 层）确保 `src/skill/` 核心模块不 import `nova_skill_loader`。后续若要在 Cargo 层彻底脱钩，可把 bootstrap + adapter 移到 `nova-cli` / `nova-server` 层。

**最终目标校正**：严格方案是把 bootstrap 和 adapter 提到 `nova-cli` / `nova-server`。但当前 `build_application` 在 `nova-agent::app` 中，移出需要较大重构。本 Plan 采用**折中方案**：

1. 删除 `registry/discovery.rs` 和 `registry/parser.rs`（核心 skill 模块不再 import loader）。
2. 在 `app/skill_adapter.rs` 放置转换函数。
3. `Cargo.toml` 保留 `nova-skill-loader` 依赖，但通过验证命令确认 `src/skill/` 中无 loader import。
4. 总览验收标准"不依赖 `nova-skill-loader`"改为"`src/skill/` 不引用 `nova_skill_loader`"。

### bootstrap 迁移 diff

```diff
 // app/bootstrap.rs
-    let mut skill_registry = SkillRegistry::new();
-    let skill_dir = config.skills_dir();
-    if let Err(err) = skill_registry.load_from_dir_async(&skill_dir).await {
-        log::warn!("Failed to load skills from {:?}: {}", skill_dir, err);
-    }
-    let skill_registry = Arc::new(skill_registry);
+    let skill_dir = config.skills_dir();
+    let loaded_skills = match nova_skill_loader::load_skills_from_dir_async(&skill_dir).await {
+        Ok(skills) => {
+            log::info!("Loaded {} skills from {:?}", skills.len(), skill_dir);
+            skills
+        }
+        Err(err) => {
+            log::warn!("Failed to load skills from {:?}: {}", skill_dir, err);
+            Vec::new()
+        }
+    };
+    let packages = super::skill_adapter::convert_loaded_skills(loaded_skills);
+    let skill_registry = match SkillRegistry::from_packages(packages) {
+        Ok(registry) => Arc::new(registry),
+        Err(err) => {
+            log::warn!("Failed to create skill registry: {}", err);
+            Arc::new(SkillRegistry::new())
+        }
+    };
```

### 错误策略

- skill 目录不存在：外层记录 debug/info 级别信息，注入空 registry。
- 单个 skill 解析失败：默认 fail-fast 更安全；如要 warn-skip，应在 loader 或 bootstrap 明确实现并测试。
- 重复 slug/id：注入 API 拒绝重复，错误由外层带上 source path 后记录。

## 迁移步骤

1. 新增 `app/skill_adapter.rs`，实现 `convert_loaded_skills`。
2. 修改 `app/mod.rs` 增加 `mod skill_adapter;`。
3. 修改 `app/bootstrap.rs`：直接调用 `nova_skill_loader::load_skills_from_dir_async`，经 adapter 转换后调用 `SkillRegistry::from_packages`。
4. 删除 `src/skill/registry/discovery.rs`。
5. 删除 `src/skill/registry/parser.rs` 中的 `From` impl 和 `load_single_skill*` 方法。如果 `parser.rs` 中只剩 `extend_loaded_skills` 和 `push_loaded_skill`，且这两个方法不再被调用，则整个文件删除。
6. 修改 `src/skill/registry.rs`：移除 `mod discovery; mod parser;`。
7. 验证 `src/skill/` 中没有 `nova_skill_loader` import：
   ```bash
   rg "nova_skill_loader" crates/nova-agent/src/skill
   ```
8. 更新测试：loader 测试留在 `nova-skill-loader`，registry 测试只构造 `SkillPackage`。
   - 当前 `registry.rs` 中的 `load_from_dir_async_loads_toml_and_markdown_skills` 测试需要迁到 `nova-skill-loader` 或改为使用 `from_packages`。

## 测试案例

- 正常路径：包含 `skill.toml` 和 `SKILL.md` 的目录由 `nova-skill-loader` 加载，外层 adapter 转换后注入 registry，catalog 与 active prompt 正确。
- 正常路径：`build_application` 使用外层 loader 后仍注册 builtin tools，并能通过 skill alias 匹配。
- 边界条件：skill 目录不存在时，注入空 registry，agent 正常启动。
- 边界条件：空 skill 列表不生成 skill prompt section。
- 异常场景：重复 slug/id 在注入阶段报错。
- 异常场景：skill 文件解析失败时，错误由外层处理，`nova-agent` 核心 skill 模块不记录文件路径类日志。

## 验收标准

- `crates/nova-agent/src/skill/` 中没有 `nova_skill_loader` import。
- `registry/discovery.rs` 和 `registry/parser.rs` 已删除。
- 运行时路径不调用 `SkillRegistry::load_from_dir*` 或 `load_single_skill*`。
- `nova-skill-loader` 自身测试覆盖 `skill.toml`、`SKILL.md`、不存在目录、解析失败。
- `build_application` 调用链：`nova_skill_loader::load_skills_from_dir_async` → `skill_adapter::convert_loaded_skills` → `SkillRegistry::from_packages`。
