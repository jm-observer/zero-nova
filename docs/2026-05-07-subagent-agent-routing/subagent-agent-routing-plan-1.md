# Plan 1: 配置模型与提示词约束

- **前置依赖**：无
- **状态**：已完成（2026-05-07）

---

## 本次目标

1. 明确首版复用 `[[gateway.agents]]` 的配置策略
2. 定义 `developer` Agent 的配置项与职责边界
3. 约束 `agent-developer.md` 应承载的系统提示词内容，避免与 `nova` 重叠失控

---

## 涉及文件

| 文件 | 操作 | 说明 |
|---|---|---|
| `.nova/config.toml` | 修改 | 新增 `developer` Agent 注册项 |
| `.nova/prompts/agent-developer.md` | 新增 | 开发型子 Agent 专用提示词 |
| `.nova/prompts/agent-nova.md` | 可选修改 | 如需补充“可被编排器选作默认回退 Agent”的简短说明 |

---

## 详细设计

### 1. `gateway.agents` 复用方式

首版不新建 `subagent_profiles`。直接在现有注册表中新增：

```toml
[[gateway.agents]]
id = "developer"
display_name = "Developer"
description = "开发任务子代理"
aliases = ["开发", "coder"]
prompt_file = "agent-developer.md"
```

设计要点：

1. `nova` 继续作为默认通用 Agent，不调整现有角色
2. `developer` 是首个面向编排器内部使用的专用 Agent
3. 顶级默认入口仍然建议保持 `nova`，避免用户入口语义变化过大

### 2. `developer` Agent 的职责边界

`developer` 只处理以下子任务：

1. 实现新功能
2. 修改现有代码
3. 修复缺陷
4. 补充或调整测试
5. 在限定文件范围内做局部重构

`developer` 不负责：

1. 全局任务分解
2. 多子任务结果汇总
3. 纯调研和纯评审任务
4. 超出分配文件范围的泛化性改动

### 3. `agent-developer.md` 内容要求

`agent-developer.md` 应比 `agent-nova.md` 更偏工程执行，至少包含以下规则：

1. 先读取相关文件，理解现有实现意图，再开始修改
2. 单次改动保持小而聚焦，不混入无关重构
3. 遵守项目技术栈和工程约束：
   - `tokio` 异步运行时
   - 禁止 `println!` 打应用日志
   - 使用 `anyhow::Result` 和 `?`
   - 禁止在异步上下文中引入阻塞操作
4. 默认只处理分配到的 `context_files`
5. 完成后执行最小必要验证，并返回修改摘要与验证结果
6. 信息不足时明确报告阻塞点，不自行脑补需求

### 4. 提示词拆分原则

为了避免 `developer` prompt 失控，建议遵循：

1. 通用行为保留在 `agent-nova.md` 或共享约定中
2. `agent-developer.md` 只追加开发任务特有约束
3. 不在 `developer` prompt 中重复描述编排协议本身

---

## 测试案例

### T1-01：配置加载

- **前提**：`.nova/config.toml` 新增 `developer`
- **预期**：Agent 注册表能够正确解析出 `nova` 和 `developer`

### T1-02：提示词区分

- **前提**：分别加载 `nova` 与 `developer`
- **预期**：`developer` 的系统提示词明显包含代码修改、验证、文件范围约束；`nova` 保持通用能力

### T1-03：默认入口不变

- **前提**：未显式指定 Agent
- **预期**：用户顶级会话仍以 `nova` 作为默认入口，不因新增 `developer` 改变行为
