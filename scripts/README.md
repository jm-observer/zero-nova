# Zero-Nova 辅助工具脚本目录 (`/scripts`)

本目录包含用于支持 `skill-creator` 自动化流程以及辅助调试 `nova-cli` 的工具脚本。

## 核心脚本说明

### 1. `execute_with_nova.py`
**用途**: 作为 `nova-cli` 的 Python 驱动层，负责执行任务并捕获结构化事件流。
- **特性**:
    - 自动调用 `cargo run --bin nova_cli`。
    - 支持 `--output-format stream-json` 解析。
    - 处理子代理的沙箱隔离与技能注入。
    - 实时将 Agent 事件转换为 Python 对象。

### 2. `validate_trigger.py`
**用途**: 分析 `nova-cli` 的 JSON 事件流，判定技能触发状态。
- **判定逻辑**:
    - 检查是否存在 `ToolStart` 事件。
    - 验证工具名称或输入参数中是否包含目标技能标识。

### 3. `smoke_test_nova_cli.py`
**用途**: `nova-cli` 改造后的“冒烟测试”脚本。
- **验证点**: 确保 `nova-cli` 的 JSON 输出格式正确，且能捕捉到常见的 API 错误或任务完成信号。

## 诊断与调试脚本

- `diagnose_evals.py`: 检查 Eval 集的 JSON 格式是否合规。
- `test_json_structure.py`: 验证 Agent 输出的 JSON 是否符合 Schema 定义。

## 使用示例

### 运行单次评估
```bash
python scripts/execute_with_nova.py "帮我设计一个 Rust 架构" --skill ./tech-solution-architect --workspace ./.nova
```

### 验证触发率
```bash
# 先保存事件到文件
python scripts/execute_with_nova.py "查询新闻" > events.json
# 运行验证
python scripts/validate_trigger.py --events-file events.json --skill-name tech-solution-architect
```

---
*注：本目录下所有脚本均针对适配 nova-cli 进行了重构。执行前请确保已安装 Python 3.10+ 及相关的依赖库。*
