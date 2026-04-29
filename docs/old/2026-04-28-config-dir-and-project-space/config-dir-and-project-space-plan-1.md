# Plan 1: 配置模型重命名与兼容策略

## 前置依赖
- 无

## 本次目标
- 把现有单一根目录语义由 `workspace` 迁移为 `config_dir`。
- 对外行为保持不变：技能、配置文件、数据、prompts 的默认路径解析逻辑不改变。
- 提供兼容层，保证旧字段/旧调用在过渡期继续可用。
- `EnvironmentSnapshot` 中的 `working_directory` 重命名为 `config_dir`，与配置模型统一语义。

## 涉及文件
- `crates/nova-agent/src/config.rs`
- `crates/nova-agent/src/agent.rs`
- `crates/nova-agent/src/prompt.rs`（`EnvironmentSnapshot` 字段重命名）
- `crates/nova-agent/src/app/bootstrap.rs`
- `crates/nova-agent/src/lib.rs`
- `crates/nova-agent/tests/bootstrap_paths.rs`

## 详细设计
- 数据结构调整：
  - `AppConfig.workspace: PathBuf` 重命名为 `AppConfig.config_dir: PathBuf`。
  - `AgentConfig.workspace: PathBuf` 保留字段名兼容内部调用，或同步重命名为 `config_dir`（建议同步重命名，避免语义混乱）。
- 构造流程：
  - `AppConfig::from_origin(origin, workspace)` 参数名调整为 `config_dir`，仅改命名不改逻辑。
  - 所有默认路径 helper 保持旧行为：
    - `skills_dir()` 默认 `<config_dir>/skills`
    - `data_dir_path()` 默认 `<config_dir>/data`
    - `prompts_dir()` 默认 `<config_dir>/prompts`
    - `config_path()` 默认 `<config_dir>/config.toml`
- `EnvironmentSnapshot` 调整：
  - `working_directory: String` 重命名为 `config_dir: String`。
  - 该字段的赋值来源从 `std::env::current_dir()` 改为从 `AppConfig.config_dir` 传入。
  - `collect()` 方法签名调整：接受 `config_dir: &Path` 参数。
  - `to_prompt_text()` 输出中 `Working directory:` 改为 `Config directory:`。
  - 此字段在提示词中暴露，供 LLM 在创建、加载 skill 等操作时定位 config 目录。
- 兼容策略：
  - 配置反序列化层保持原有 `tool.skills_dir` 等字段不变。
  - 若外部 crate 存在公开 API 依赖 `workspace` 命名，提供过渡方法或 type alias（仅在确有必要时，尽量限制在 crate 内）。
- 代码约束：
  - 不引入新依赖。
  - 保持函数职责单一，避免在 Plan 1 混入运行时行为变更。

## 测试案例
- 正常路径：默认配置下各 helper 路径与旧版本一致（仅根字段命名变化）。
- 覆盖路径：相对路径 override 与绝对路径 override 行为不变。
- 兼容路径：旧测试用例在最小修改后全部通过，确保无行为回归。
- 错误路径：非法配置值（如重复 agent id）校验行为不受影响。
- 提示词路径：`EnvironmentSnapshot.to_prompt_text()` 输出包含 `Config directory:` 而非 `Working directory:`。
