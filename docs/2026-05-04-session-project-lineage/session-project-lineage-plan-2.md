# Plan 2: 同 Agent 最近 Session 继承与恢复

## 前置依赖
Plan 1: Session Project 数据模型收敛

## 本次目标
把“切换 Agent”和“创建新 session”的行为统一到同一套 session 路由规则上，确保 project 只在同 Agent 的 session 链中传播，并以最近活跃时间作为唯一判定依据。

## 涉及文件
- `D:\git\zero-nova\crates\nova-agent\src\conversation\service.rs`
- `D:\git\zero-nova\crates\nova-agent\src\conversation\repository.rs`
- `D:\git\zero-nova\crates\nova-agent\src\app\conversation_service.rs`
- `D:\git\zero-nova\crates\nova-agent\src\app\application.rs`
- `D:\git\zero-nova\crates\nova-agent\src\app\types.rs`
- 相关集成测试文件

## 详细设计

### 1. 最近 session 的判定规则
“最近 session” 定义为：
- `sessions.agent_id = target_agent_id`
- 按 `updated_at DESC` 排序的第一条

这里直接复用持久化层已有的 `updated_at`，不新增字段。

### 2. Repository 层新增查询能力
建议新增专用查询，例如：

```rust
pub async fn find_latest_session_by_agent(&self, agent_id: &str) -> Result<Option<SessionRow>>
```

查询条件和排序：
- `WHERE agent_id = ?`
- `ORDER BY updated_at DESC`
- `LIMIT 1`

不要在内存层遍历全部 session 再筛选，避免启动后缓存与数据库顺序不一致。

### 3. 创建 session 的继承逻辑
当前 `SessionService::create(...)` 固定使用默认目录。改造后应变为：

```rust
pub async fn create_for_agent(
    &self,
    name: Option<String>,
    agent_id: String,
    system_prompt: String,
    inherited_project_dir: Option<PathBuf>,
) -> Result<Arc<Session>>
```

调用方在创建前先查询：
- 若目标 Agent 有最近 session，则传入其 `project_dir.clone()`
- 若无，则传入 `None`

继承范围只限 `project_dir`，不复制历史消息、技能绑定、模型覆盖。

### 4. 切换 Agent 的恢复逻辑
当前 `switch_agent(session_id, agent_id)` 只改写已有 session 的 `active_agent`。新语义应改为“恢复或创建目标 Agent 的 session”。

推荐改法：
- 应用层新增一个“按 Agent 打开 session”的用例，而不是复用旧 `switch_agent`
- 行为如下：
- 查最近 session
- 有则返回该 session
- 无则创建 `project_dir = None` 的新 session 并返回

如果为了兼容现有 API 必须保留 `switch_agent` 名称，则其内部实现也必须切到上述语义，而不能再停留在“当前 session 改 active_agent”。

### 5. `active_agent` 与 `sessions.agent_id` 的关系
当前 session 持久化表里已有 `agent_id` 列，同时 `ControlState` 里也有 `active_agent`。

在“一个 session 归属一个 Agent”的新语义下：
- `sessions.agent_id` 是 session 归属
- `control.active_agent` 应与之保持一致

因此不建议再支持“同一个 session 内部切换 active_agent 到另一个 Agent”。否则“同 Agent 继承边界”会再次被破坏。

### 6. 最近活跃时间刷新要求
以下动作完成后必须刷新 `updated_at`：
- 用户发送消息
- 工具成功改写 `project_dir`
- session 被创建
- session 被恢复并成为当前活跃 session（若产品层把“进入 session”视为活跃行为）

至少要保证“修改 project 后，该 session 会成为该 Agent 最近 session”，否则继承规则会选错来源。

## 测试案例

- 测试 1：同一 Agent 有多个 session 时，按 `updated_at` 选择最近 session
- 测试 2：为 Agent A 新建 session 时，只继承 Agent A 最近 session 的 `project_dir`
- 测试 3：Agent B 新建 session 不继承 Agent A 的 `project_dir`
- 测试 4：目标 Agent 无历史 session 时，成功创建 `project_dir = None` 的新 session
- 测试 5：切换到已有历史 session 的 Agent 时，返回该 Agent 最近 session，而不是修改当前 session 的 `active_agent`
- 测试 6：修改 `project_dir` 后，新的 `updated_at` 能影响后续“最近 session”判定
