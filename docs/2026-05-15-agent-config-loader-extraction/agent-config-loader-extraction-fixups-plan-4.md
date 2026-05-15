# Plan 4: `ConfigStore` 抽象与 reload 回写

## 前置依赖

Plan 1。可与 Plan 2 / Plan 3 并行（但 Plan 3 引入 `AgentBindingResolver` 时若提前预留 ConfigStore，可少改一次签名）。

## 本次目标

修复 `ConfigBackedSessionPromptReloader` reload 后不回写 `Arc<RwLock<AppConfig>>` 的 bug，建立配置 single source of truth：

- 引入 `ConfigStore` 类型，封装"读取当前快照 / 触发 reload / 通知订阅者"。
- `AgentWorkspaceService`、`ConversationService`、`AgentApplicationImpl`、Agent tool 一律从 store 读取，而非各自持有 `RwLock<AppConfig>` clone。
- reload 完成后，全局可见的 config 是新值。
- 让 `PromptMaterialLoader` 句柄按需根据最新 config 重建（或缓存 + 失效）。

## 涉及文件

| 文件 | 变更类型 | 说明 |
| --- | --- | --- |
| `crates/nova-agent-loader/src/config_store.rs` | 新增 | `ConfigStore` 与 `ConfigSubscription` |
| `crates/nova-agent-loader/src/lib.rs` | 修改 | 暴露 ConfigStore |
| `crates/nova-agent-loader/src/bootstrap.rs` | 修改 | 用 ConfigStore 替代裸 `Arc<RwLock<AppConfig>>` |
| `crates/nova-agent/src/app/agent_workspace_service.rs` | 修改 | `config: Arc<RwLock<AppConfig>>` → `config_store: Arc<dyn ConfigSnapshot>` |
| `crates/nova-agent/src/app/conversation_service.rs` | 修改 | `app_config: AppConfig` → 通过 store 读取或保留只读 snapshot 字段 |
| `crates/nova-agent/src/app/application.rs` | 修改 | `update_config` 走 store |

## 详细设计

### ConfigStore 接口

```rust
// crates/nova-agent-loader/src/config_store.rs

pub trait ConfigSnapshot: Send + Sync {
    fn current(&self) -> Arc<AppConfig>;
}

pub struct ConfigStore {
    inner: ArcSwap<AppConfig>,
    config_path: PathBuf,
    config_dir: PathBuf,
    listeners: RwLock<Vec<Arc<dyn ConfigListener>>>,
}

#[async_trait]
pub trait ConfigListener: Send + Sync {
    async fn on_config_changed(&self, new_config: Arc<AppConfig>);
}

impl ConfigStore {
    pub fn new(initial: AppConfig) -> Arc<Self> { ... }

    pub async fn reload_from_disk(&self) -> Result<Arc<AppConfig>> {
        let next = AppConfig::load_from_file(&self.config_path, self.config_dir.clone())?;
        let arc = Arc::new(next);
        self.inner.store(arc.clone());
        let listeners = self.listeners.read().await.clone();
        for l in listeners {
            l.on_config_changed(arc.clone()).await;
        }
        Ok(arc)
    }

    pub async fn apply(&self, config: AppConfig) -> Arc<AppConfig> {
        let arc = Arc::new(config);
        self.inner.store(arc.clone());
        // notify same as reload
        arc
    }

    pub async fn add_listener(&self, listener: Arc<dyn ConfigListener>) { ... }
}

impl ConfigSnapshot for ConfigStore {
    fn current(&self) -> Arc<AppConfig> { self.inner.load_full() }
}
```

要点：

- 用 `ArcSwap<AppConfig>` 而非 `RwLock<AppConfig>`：读取无锁、克隆轻；写入原子；与现有 `config_snapshot_cache: Arc<ArcSwap<Value>>` 风格一致。
- listener 用于：`PromptMaterialLoader` 句柄重建、catalog 缓存失效、prompt diagnostics 重置等。

### 上层接入

`build_application`：

```rust
let store = ConfigStore::new(config.clone());
let snapshot: Arc<dyn ConfigSnapshot> = store.clone();

// 把原本 self.config_arc 的位置都替换为 store
let prompt_loader = PromptMaterialLoaderHandle::new(store.clone());
store.add_listener(prompt_loader.clone()).await;

let workspace_service = AgentWorkspaceService::new(
    agent_registry,
    session_service,
    snapshot.clone(),
    skill_registry.clone(),
    Some(Arc::new(StoreBackedReloader { store: store.clone() })),
);
```

`PromptMaterialLoaderHandle`：内部 `ArcSwap<PromptMaterialLoader>`，listener 收到新 config 后重建 loader，调用方 `handle.current().load_turn_material(...).await`。

### ConfigBackedSessionPromptReloader 修复

新实现：

```rust
struct StoreBackedReloader { store: Arc<ConfigStore> }

#[async_trait]
impl SessionPromptReloader for StoreBackedReloader {
    async fn reload_session_prompt(&self, ...) -> Result<ReloadedSessionPrompt> {
        let new_config = self.store.reload_from_disk().await?;
        // listener 已经把 PromptMaterialLoaderHandle 更新到新 config
        // 这里直接用 store.current() 构建本次 prompt
        ...
    }
}
```

副作用：

- workspace_service / agent_tool / conversation_service 读取 config 时拿到的就是最新值。
- `AgentApplicationImpl::config_snapshot` 直接返回 `store.current()` 的 JSON 序列化。

### AgentWorkspaceService 改造

```rust
pub struct AgentWorkspaceService {
    pub agent_registry: AgentRegistry,
    pub sessions: SessionService,
    pub config_snapshot: Arc<dyn ConfigSnapshot>,
    pub skill_registry: Arc<SkillRegistry>,
    prompt_reloader: Arc<dyn SessionPromptReloader>,
}

impl AgentWorkspaceService {
    fn config(&self) -> Arc<AppConfig> { self.config_snapshot.current() }
}
```

所有 `self.config.read().await.clone()` 替换为 `self.config()`。读为常数时间，无锁等待。

### ConversationService 改造

`app_config: AppConfig` 字段移除，改为 `config_snapshot: Arc<dyn ConfigSnapshot>`。

调用点（如 `resolve_run_models`、`prompt_compaction` 读取）改为：

```rust
let config = self.config_snapshot.current();
let compaction = &config.prompt_compaction;
```

注意：`agent.config.config_dir` 在 turn 内仍使用 `AgentRuntime::config.config_dir`（这是冻结的 runtime 配置，不需变更）。

### Agent tool 与 Plan 3 联动

Plan 3 设计的 `AgentBindingResolver` 实现可直接由 `Arc<dyn ConfigSnapshot>` 支撑：

```rust
pub struct SnapshotBindingResolver { snapshot: Arc<dyn ConfigSnapshot> }

impl AgentBindingResolver for SnapshotBindingResolver {
    fn resolve(&self, agent_id: &str) -> Result<ResolvedAgentBinding> {
        let cfg = self.snapshot.current();
        cfg.resolve_agent_binding_by_id(agent_id)
    }
    ...
}
```

### 配置更新 RPC

`AgentApplicationImpl::update_config(payload)`：

- 将 payload 反序列化为 `AppConfig`（保留现有 validate）。
- 调用 `store.apply(new_config)`，listener 自动级联。
- `config_snapshot_cache` 由 listener 维护（提交后写新 JSON）。

### Drop 顺序与生命周期

- `ConfigStore` 持有 `Arc<dyn ConfigListener>`；listener 持有 `Arc<dyn Something>` 通常不包含 store。避免循环引用。
- 若需要 listener 持有 store，使用 `Weak<ConfigStore>`。

## 测试案例

### 单测：ConfigStore

- `reload_from_disk` 在临时目录写 `config.toml`，验证 `current()` 返回的 AppConfig 字段被更新。
- listener 在 reload 完成前后被回调一次，参数等于新 config。
- `apply(config)` 通知 listener。

### 集成：workspace reload

`crates/nova-agent/tests/integration/`（或新增 `session_reload.rs`）：

1. 构造一个 fake `LlmClient`、`ConfigStore`、`AgentWorkspaceService`。
2. 修改 `config.toml` 文件内容（例如改 `gateway.max_iterations` 或新增 agent）。
3. 调用 `workspace_service.reload_session_system_prompt(session_id)`。
4. 验证：
   - `app.config_snapshot()` JSON 包含新值。
   - `workspace_service.inspect_agent` 用新 config 解析 binding。
   - prompt loader 持有的 `prompts_dir` 与新 config 一致（如该字段变化）。

### 行为不回归

- `cargo test --workspace`：含 prompt material / app facade 全部通过。
- `cargo clippy --workspace -- -D warnings`：通过。

### 异常路径

- reload 时 `AppConfig::load_from_file` 失败：`reload_from_disk` 返回 Err，store 内容保持旧值，listener 不触发。
- listener panic：用 `tokio::spawn` 隔离或 catch，store 自身不应丢失新值；建议 listener trait 保持 `Result<()>` 返回，store 记录失败日志后继续。

## 备注

- 本 Plan 完成后，可删除 `bootstrap.rs::ConfigBackedSessionPromptReloader` / `ConfigBackedTurnPromptMaterialLoader` / `ConfigBackedAgentPromptLoader` 三个临时 reloader/loader 适配类型，统一由 `ConfigStore` + `PromptMaterialLoaderHandle` 提供。
- 若后续 deskapp 也需订阅配置变更（如修改 model 后 UI 自动刷新），可在 ConfigStore 上再叠加 `tokio::sync::watch::Receiver` 暴露给前端，但不在本 Plan 范围。
