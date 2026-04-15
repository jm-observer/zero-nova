# Phase 5: Tauri Integration & Channel Switch

## Goal

将 nova-gateway 集成到 Tauri 应用生命周期中，支持在配置中切换 nova / node 两种 channel，改造 gateway commands。

## Prerequisites

- Phase 4 完成 (chat 端到端可用)
- 已 fork OpenFlux 仓库并能编译运行

## Tasks

### 5.1 扩展配置 — Channel 模式

在 OpenFlux 的 `openflux.yaml` 配置中增加 nova channel 支持:

```yaml
gateway:
  channel: "nova"            # "nova" | "node" (默认 "node" 保持向后兼容)
  host: "localhost"
  port: 18801

# 仅 channel: "nova" 时使用
nova:
  model: "claude-sonnet-4-20250514"
  max_tokens: 8192
  max_iterations: 10
  temperature: null           # null = 不设置，使用 API 默认值
  # API key 从 ANTHROPIC_API_KEY 环境变量读取
  # base_url 从 ANTHROPIC_BASE_URL 读取，默认 https://api.anthropic.com
```

对应 Rust 结构:

```rust
#[derive(Debug, Deserialize)]
pub struct GatewayConfig {
    pub channel: String,     // "nova" | "node"
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct NovaConfig {
    pub model: String,
    pub max_tokens: u32,
    pub max_iterations: usize,
    pub temperature: Option<f64>,
}
```

### 5.2 改造 commands/gateway.rs

```rust
use crate::nova_gateway::NovaGateway;

/// Gateway 状态: 支持两种 channel
enum GatewayHandle {
    Nova(tokio::task::JoinHandle<()>),     // Rust WS server task
    Node(std::process::Child),              // Node.js 子进程
}

struct GatewayState {
    handle: Mutex<Option<GatewayHandle>>,
}

#[tauri::command]
pub async fn start_gateway(
    config: tauri::State<'_, AppConfig>,
    gw_state: tauri::State<'_, GatewayState>,
) -> Result<(), String> {
    let mut handle = gw_state.handle.lock().await;
    if handle.is_some() {
        return Err("Gateway already running".into());
    }

    match config.gateway.channel.as_str() {
        "nova" => {
            // 初始化 zero-nova runtime
            let client = AnthropicClient::from_env()
                .map_err(|e| e.to_string())?;
            let mut tools = ToolRegistry::new();
            register_builtin_tools(&mut tools);

            let agent_config = AgentConfig {
                max_iterations: config.nova.max_iterations,
                model_config: ModelConfig {
                    model: config.nova.model.clone(),
                    max_tokens: config.nova.max_tokens,
                    temperature: config.nova.temperature,
                    top_p: None,
                },
            };

            let system_prompt = SystemPromptBuilder::default().build();
            let runtime = AgentRuntime::new(client, tools, system_prompt, agent_config);

            let state = Arc::new(nova_gateway::GatewayState {
                agent: Mutex::new(runtime),
                sessions: SessionStore::new(),
                config: config.nova.clone(),
            });

            let h = NovaGateway::start(
                &config.gateway.host,
                config.gateway.port,
                state,
            ).await.map_err(|e| e.to_string())?;

            *handle = Some(GatewayHandle::Nova(h));
            Ok(())
        }
        "node" | _ => {
            // 保留原有 Node.js sidecar 逻辑
            let child = start_node_sidecar(&config)?;
            *handle = Some(GatewayHandle::Node(child));
            Ok(())
        }
    }
}

#[tauri::command]
pub async fn stop_gateway(
    gw_state: tauri::State<'_, GatewayState>,
) -> Result<(), String> {
    let mut handle = gw_state.handle.lock().await;
    match handle.take() {
        Some(GatewayHandle::Nova(h)) => {
            h.abort();  // 停止 WS server task
            Ok(())
        }
        Some(GatewayHandle::Node(mut child)) => {
            child.kill().map_err(|e| e.to_string())?;
            child.wait().map_err(|e| e.to_string())?;
            Ok(())
        }
        None => Err("Gateway not running".into()),
    }
}

#[tauri::command]
pub async fn restart_gateway(
    config: tauri::State<'_, AppConfig>,
    gw_state: tauri::State<'_, GatewayState>,
) -> Result<(), String> {
    stop_gateway(gw_state.clone()).await.ok();
    start_gateway(config, gw_state).await
}
```

### 5.3 改造 Tauri main (lib.rs)

```rust
fn main() {
    tauri::Builder::default()
        .manage(AppConfig::load())
        .manage(GatewayState { handle: Mutex::new(None) })
        .invoke_handler(tauri::generate_handler![
            start_gateway,
            stop_gateway,
            restart_gateway,
            // ... 其他 commands
        ])
        .setup(|app| {
            // 应用启动时自动启动 gateway
            let config = app.state::<AppConfig>();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = start_gateway(config, gw_state).await {
                    log::error!("Failed to start gateway: {}", e);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

### 5.4 优雅关闭

```rust
// 在 Tauri setup 中注册窗口关闭事件
app.on_window_event(|event| {
    if let tauri::WindowEvent::CloseRequested { .. } = event.event() {
        // 停止 gateway
        tauri::async_runtime::block_on(async {
            stop_gateway(gw_state).await.ok();
        });
    }
});
```

### 5.5 Cargo.toml 依赖

```toml
# src-tauri/Cargo.toml 新增
[dependencies]
zero-nova = { path = "../../zero-nova" }
tokio-tungstenite = "0.24"
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
```

### 5.6 集成测试

1. 配置 `channel: "nova"` → 启动应用 → WS server 在 18801 端口可达
2. 配置 `channel: "node"` → 启动应用 → Node.js sidecar 正常启动
3. restart_gateway command → 旧 server 停止，新 server 启动
4. 关闭窗口 → gateway 正确停止，无残留进程

## Modified Files

```
src-tauri/src/
├── commands/gateway.rs    # MAJOR REWRITE
├── config.rs              # MODIFIED: 增加 NovaConfig
├── lib.rs                 # MODIFIED: 注册 state 和 setup
└── nova_gateway/mod.rs    # MODIFIED: pub start/stop 接口
```

## Definition of Done

- [ ] `channel: "nova"` 配置下应用正常启动
- [ ] WS server 自动启动，前端自动连接成功
- [ ] `channel: "node"` 配置下原有行为不受影响
- [ ] start/stop/restart commands 正常工作
- [ ] 应用关闭时 gateway 正确清理
