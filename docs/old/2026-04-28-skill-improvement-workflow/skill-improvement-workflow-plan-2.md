# Plan 2: 目标 skill 加载、校验与基线评测链路

## 前置依赖
- Plan 1

## 本次目标
- 让改进流程可以稳定地“真实加载目标 skill”并运行基线评测。
- 统一原 skill、无 skill、候选 skill 三类运行方式，确保比较结果有可解释基线。
- 在不改动原 skill 的前提下，完成快照、候选副本和迭代评测目录的准备。

## 涉及文件
- `.nova/skills/skill-creator/scripts/quick_validate.py`
- `.nova/skills/skill-creator/scripts/run_eval.py`
- `.nova/skills/skill-creator/scripts/run_loop.py`
- 新增 `.nova/skills/skill-creator/scripts/prepare_improvement.py`
- 新增 `.nova/skills/skill-creator/scripts/run_iteration.py`
- `crates/nova-agent/src/skill.rs`
- `crates/nova-cli/src/main.rs`

## 详细设计
- 新增 `prepare_improvement.py`，职责保持单一：
  - 校验目标 skill。
  - 创建工作区。
  - 将目标 skill 目录完整复制到 `target-skill-snapshot/`。
  - 生成首轮会话文件与 eval 文件占位。
- 将 `quick_validate.py` 从“只校验 `SKILL.md` frontmatter”扩展为两层：
  - 轻量层：继续做格式和字段校验。
  - 组合层：校验目标目录是否具备迭代所需最小结构，例如 `evals/`、引用资源、辅助脚本路径等。
- 为 `run_eval.py` 增加统一输入模型，使其可接受以下模式：
  - `baseline_none`：完全不加载 skill。
  - `baseline_original`：加载 `target-skill-snapshot/`。
  - `candidate`：加载 `iterations/iteration-xxx/candidate-skill/`。
- 真实加载方式优先复用现有 `nova-cli --include-skill <path>`，不在 Python 层模拟 skill 注入，从而保证评测行为和实际运行一致。
- 如果需要 CLI 层补强，只增加薄封装，不改变已有主流程：
  - 可选新增 `nova-cli skill validate <path>`。
  - 可选新增 `nova-cli skill run-eval --include-skill <path> ...`。
  - 这些命令底层仍复用 `nova-agent` 现有 skill registry 与事件系统。
- 新增 `run_iteration.py`，负责一轮内的目录编排：
  - 创建 `iteration-xxx/`。
  - 准备 `candidate-skill/`。
  - 执行 `baseline_original` 与 `candidate` 的评测。
  - 汇总结果到 `eval-results.json` 和 `score-summary.json`。
- 分数模型建议拆成两部分：
  - 触发分：基于 `should_trigger`、触发率阈值、误触发率。
  - 行为分：基于 assertions 通过率、人工比较结果、失败严重度。
- 任何一轮评测失败都不得吞错：
  - 明确区分“加载失败”“运行超时”“输出不符合预期”“评测脚本异常”。
  - 错误写入会话文件和本轮 `notes.md`，供后续优化器使用。

## 测试案例
- 正常路径：合法 skill 可以被完整快照，且 `baseline_original` 与 `candidate` 都能实际加载运行。
- 对照路径：同一 eval 集下，无 skill 与原 skill 得分应能同时产出，便于后续比较。
- 边界条件：目标 skill 附带额外资源文件时，目录级复制后仍能成功运行。
- 错误路径：候选 skill 被写坏后，系统应只标记该轮失败，不污染原始快照与历史最佳版本。
- 稳定性：同一基线重复执行多次时，结果文件结构稳定且字段完整。
