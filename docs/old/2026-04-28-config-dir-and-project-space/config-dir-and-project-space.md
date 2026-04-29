# config-dir-and-project-space 设计总览

## 时间
- 创建时间：2026-04-28
- 最后更新：2026-04-28（Plan 4 完成）

## 项目现状
- 当前 `nova-agent` 以单一 `workspace` 作为根目录语义。
- 配置、技能、运行数据、提示词路径均由该根目录派生（例如 `skills_dir()`、`data_dir_path()`、`prompts_dir()`）。
- 用户在对话过程中可表达"切换到某个项目目录并用 `@` 指定文件/目录"的需求，但现有模型缺少"会话级项目空间"概念。
- 系统提示词 Environment 部分的 `Working directory` 实际指向进程 cwd，与 config 根目录语义混淆。

## 整体目标
- 将当前 `workspace` 语义重命名为 `config_dir`（或等价命名），保持现有行为不变。
- 引入会话级 `project_dir: PathBuf`（**非可选，默认值为进程 cwd**），支持在运行中动态切换。
- **将 `config_dir` 和 `project_dir` 注入系统提示词的 Environment 部分**，使 LLM 能感知配置目录（skill 创建/加载）和当前项目目录上下文。
- 设计并落地统一的 `@` 路径解析策略，支持相对路径、绝对路径。
- 在不破坏既有配置与调用方的前提下完成兼容迁移。

## 目录模型概览

| 概念 | 语义 | 生命周期 | 提示词中的名称 |
|------|------|---------|---------------|
| `config_dir` | Nova 配置根目录，承载 config.toml、skills、prompts、data | 应用级，不可变 | `Config directory:` |
| `project_dir` | 用户当前工作的项目目录 | 会话级，可动态切换，默认 cwd | `Project directory:` |

## Plan 拆分
| Plan | 简要描述 | 依赖 | 执行顺序 | 状态 |
|---|---|---|---|---|
| Plan 1 | 配置模型重命名与兼容策略（workspace -> config_dir） | 无 | 1 | 已完成 |
| Plan 2 | 会话态 project_dir 设计与运行中切换机制 | Plan 1 | 2 | 已完成 |
| Plan 3 | `@` 路径解析器与调用链接入 | Plan 1, Plan 2 | 3 | 已完成 |
| Plan 4 | 测试、文档迁移与发布策略 | Plan 1, Plan 2, Plan 3 | 4 | 已完成 |

## 运行示例
- 运行中切换 `project_dir`：会话默认使用进程 cwd，可通过会话接口切换到任意目录，也可重置回 cwd。
- `@` 用法示例：
  - `@src/main.rs`：相对 `project_dir` 解析
  - `@D:/repo/app/Cargo.toml`（Windows）或 `@/home/user/app/Cargo.toml`（Unix）：按绝对路径解析

## 范围外事项（后续独立设计）
- **提示词中的目录认知与 skill 上下文增强**：skills、prompts 目录路径是否需要在提示词中暴露给 LLM，涉及 skill-creator 工具链、动态 skill 发现、提示词模板等，复杂度自成体系，建议新开设计文档。
- 当前 `agent-nova.md` 中硬编码了 `.nova/skills` 路径引用，后续应考虑模板化或动态注入。

## 风险与待定项
- 风险：旧代码可能把 `workspace` 当"业务项目根目录"使用，重命名后需逐处确认语义不漂移。
- 风险：路径解析跨平台差异（Windows 盘符、UNC、符号链接）可能导致边界行为不一致。
- 风险：会话并发场景下切换 `project_dir` 的一致性与可见性（读写锁粒度）需明确。
- 风险：`project_dir` 切换后，提示词中的 Git 信息（branch/status/commits）应反映 project_dir 目录而非 config_dir，否则 LLM 获取的上下文会错位。
- 风险：桌面端场景中进程 cwd 可能是应用安装目录而非用户项目目录，需在 Tauri shell 启动时主动设置合理默认值（如用户 home 或上次打开的目录）。
