# Plan 1: Loader Crate 抽取

## 前置依赖

无

## 本次目标

新增独立 workspace crate `nova-skill-loader`，提供同步/异步目录扫描、单技能加载、`skill.toml` 解析和 `SKILL.md` 兼容解析能力。

## 涉及文件

- `Cargo.toml`
- `crates/nova-skill-loader/Cargo.toml`
- `crates/nova-skill-loader/src/lib.rs`

## 详细设计

`nova-skill-loader` 定义 `LoadedSkillPackage`、`LoadedCompatSkill`、`LoadedToolPolicy` 和 `LoadedSkill`，作为与 agent 解耦的加载层模型。加载层只负责从文件系统读取并解析为中立模型，不依赖 agent runtime、prompt、tool policy 执行逻辑。

同步入口 `load_skills_from_dir` 保留给启动期和测试使用；异步入口 `load_skills_from_dir_async` 用显式栈遍历目录，避免 async 递归。单技能入口 `load_single_skill` 与 `load_single_skill_async` 保持优先读取 `skill.toml`、再回退 `SKILL.md` 的既有顺序。

## 测试案例

- 正常路径：目录中同时存在 TOML 与 Markdown skill 时均可加载。
- 边界条件：不存在或非目录路径返回空列表。
- 异常场景：无效 TOML 返回错误，交给调用方决定是否降级或中止。
