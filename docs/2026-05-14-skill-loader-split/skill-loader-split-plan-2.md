# Plan 2: Agent Registry 适配

## 前置依赖

Plan 1

## 本次目标

将 `nova-agent` 中 `SkillRegistry` 的目录扫描与解析实现替换为对 `nova-skill-loader` 的调用，并保留 `load_from_dir`、`load_from_dir_async`、`load_single_skill` 与 `load_single_skill_async` 等既有接口。

## 涉及文件

- `crates/nova-agent/Cargo.toml`
- `crates/nova-agent/src/skill/registry/discovery.rs`
- `crates/nova-agent/src/skill/registry/parser.rs`

## 详细设计

`discovery.rs` 只负责调用 loader 的目录加载入口，并将结果批量写入 registry。`parser.rs` 作为适配层保留原 public API，将 `LoadedSkill` 转换为 agent 内部 `Skill` 和 `SkillPackage`。这样 agent 仍保留完整的 skill registry、策略裁剪、查询和 prompt 生成能力。

日志边界保持在 agent 侧，loader 不输出应用日志，避免公共加载 crate 引入运行期日志策略。

## 测试案例

- 正常路径：原有 registry 异步加载测试继续通过。
- 边界条件：空目录和不存在目录不改变 registry 状态。
- 异常场景：解析失败时错误可通过 `anyhow::Result` 传播。
