# Plan 1: 改进模式入口、会话模型与工作区约定

## 前置依赖
- 无

## 本次目标
- 为 `skill-creator` 增加显式的“改进已有 skill”模式，而不是让用户自己拼装创建、测试、优化步骤。
- 定义改进会话的元数据模型、目录布局、状态流转与恢复约束。
- 统一“目标 skill 路径 / skill id / 工作区路径 / 输出产物”的命名和持久化格式，作为后续自动化链路的基础。

## 涉及文件
- `.nova/skills/skill-creator/SKILL.md`
- `.nova/skills/skill-creator/references/schemas.md`
- `.nova/skills/skill-creator/assets/eval_review.html`
- `.nova/skills/skill-creator/scripts/utils.py`
- 新增 `.nova/skills/skill-creator/scripts/session_schema.py`
- 新增 `.nova/skills/skill-creator/assets/improvement_session_template.json`

## 详细设计
- 在 `SKILL.md` 中新增独立章节“Improve Existing Skill”，明确该模式的触发条件：
  - 用户明确给出 skill 路径、skill 名称或“帮我改进这个 skill”。
  - 当前目标是迭代现有 skill，而不是新建 skill。
  - 优先进入“加载 -> 校验 -> 快照 -> 评测 -> 候选优化 -> 比较 -> 写回”流程。
- 定义 `improvement-session.json` 结构，最少包含：
  - `session_id`
  - `target_skill_path`
  - `target_skill_name`
  - `workspace_path`
  - `snapshot_path`
  - `status`（initialized / evaluating / optimizing / paused / completed / failed）
  - `baseline_result_path`
  - `best_iteration`
  - `best_score`
  - `iterations`
  - `created_at`、`updated_at`
- 在 `references/schemas.md` 中补充改进会话 schema，并区分两类 eval：
  - `trigger_evals`：验证“该不该触发”“触发是否稳定”。
  - `behavior_evals`：验证“触发后输出/行为是否更好”。
- 为 `utils.py` 增加通用会话辅助函数：
  - 解析目标 skill 基础信息。
  - 生成稳定的工作区名。
  - 初始化或恢复改进会话。
  - 统一读写 JSON，避免各脚本自行拼字段。
- 在 `eval_review.html` 中增加会话上下文展示：当前目标 skill、工作区、eval 类型、导出目标文件名，降低用户在人工审阅阶段的迷失成本。
- 约束状态流转：
  - `initialized` 只能进入 `evaluating` 或 `failed`。
  - `evaluating` 结束后进入 `optimizing` 或 `paused`。
  - `optimizing` 每轮结束更新 `best_iteration`，直到 `completed` 或 `failed`。
  - 任何异常退出都必须写回 `updated_at` 与最近阶段，便于恢复。

## 测试案例
- 正常路径：提供合法 skill 目录，成功创建工作区并写出 `improvement-session.json`。
- 恢复路径：已有会话文件存在时，脚本应恢复会话而不是覆盖历史。
- 边界条件：用户只给 skill id，不给路径时，能根据标准 skill 根目录解析到目标。
- 错误路径：目标目录缺少 `SKILL.md`、frontmatter 缺字段、路径不存在时，给出明确错误并停在 `failed`。
- 一致性：不同脚本读写同一个会话文件时，不出现字段名漂移或状态含义不一致。
