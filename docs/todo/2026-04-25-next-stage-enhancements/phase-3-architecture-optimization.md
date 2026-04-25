# Phase 3: 架构优化 - 详细设计

> 日期：2026-04-25
> 范围：前端类型安全、异步统一、测试覆盖、CI/CD

---

## 背景

Phase 3 关注代码质量和可维护性，确保项目从"功能正确"演进到"工程优秀"。

---

## 任务清单

### 3.1 前端 TypeScript 类型安全

#### 3.1.1 协议类型定义同步

**问题：** `gateway-client.ts` 中大量使用 `as any` 断言

```typescript
// 当前
state.setAgents(agents as any);
state.setSessions(sessions as any);
```

**解决方案：**

创建统一的类型定义模块：

```typescript
// deskapp/src/proto/types.ts

import type { Session, Message, Agent } from './nova-protocol';

// 扩展前端所需的额外字段
export interface ExtendedSession extends Session {
  latestMessage?: string;
  updatedAt?: number;
}

export interface ExtendedAgent extends Agent {
  isCurrent?: boolean;
  icon?: string;
}

export interface ExtendedMessage extends Message {
  createdAt?: number;
  progress?: ProgressUpdate;
}

// 工具调用扩展
export interface ExtendedToolCall {
  id: string;
  toolName: string;
  input: Record<string, unknown>;
  status: 'idle' | 'running' | 'completed' | 'error';
  result?: string;
  error?: string;
}

// 进度事件扩展
export interface ProgressEvent {
  type: 'tool_start' | 'tool_complete' | 'token_stream' | 'thought' | 'error';
  toolName?: string;
  token?: string;
  thought?: string;
  timestamp: number;
}

// 定义完整的数据类型引用协议 DTO，消除 `as any`
```

**改写 setState 调用：**

```typescript
// 改写前
state.setAgents(agents as any);

// 改写后
state.setAgents(agents as ExtendedAgent[]);
```

**映射转换工具：**

```typescript
// deskapp/src/proto/mappers.ts

export function mapToExtendedSession(s: Session): ExtendedSession {
  return {
    ...s,
    updatedAt: new Date(s.updatedAt).getTime(),
  };
}

export function mapToExtendedAgents(agents: Agent[]): ExtendedAgent[] {
  return agents.map((a) => ({ ...a, isCurrent: false }));
}
```

---

#### 3.1.2 Error Boundary 与 类型守卫

```typescript
// deskapp/src/proto/typeGuards.ts

// 类型守卫函数
export function isProgressEvent(value: unknown): value is ProgressEvent {
  return (
    typeof value === 'object' &&
    value !== null &&
    'type' in value &&
    'timestamp' in value &&
    ['tool_start', 'tool_complete', 'token_stream', 'thought', 'error'].includes(
      (value as ProgressEvent).type
    )
  );
}

// Error Boundary 组件
export class GatewayConnectionErrorBoundary extends React.Component {
  // 用于处理 Gateway 连接错误的 React Error Boundary
}
```

---

### 3.2 异步 vs 阻塞 API 统一

#### 3.2.1 扫描阻塞点

**当前阻塞点：**

| 文件 | 函数 | 阻塞操作 |
|------|------|----------|
| deskapp/src-tauri/src/commands/gateway.rs | start_gateway_sidecar | std::process::Command |
| deskapp/src-tauri/src/commands/file.rs | file_read | std::fs::read |

**解决方案：**

```rust
// 使用 spawn_blocking 转换阻塞调用
#[tauri::command]
pub async fn file_read_large(file_path: String, max_buffer: u64) -> Result<FileBuffer, String> {
    let file_path_clone = file_path.clone();
    let data = tokio::task::spawn_blocking(move || {
        // 阻塞操作在线程池中执行
        std::fs::read(&file_path_clone)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
    .map_err(|e| e.to_string())?;

    // 返回缓冲或分块数据
    Ok(FileBuffer {
        size: data.len(),
        mime: detect_mime(&file_path_clone),
    })
}
```

---

#### 3.2.2 回调异步化

**Context 传递优化：**

```rust
// 添加 tokio::sync::watch 用于实时状态同步
pub struct ToolContext {
    // ... 现有字段 ...
    pub force_abort: tokio::sync::watch::Receiver<bool>,
}

// 支持异步取消的工具调用
trait AsyncCancelled {
    async fn with_abort_check(&mut self) -> Result<(), String>;
}
```

---

### 3.3 测试覆盖率提升

#### 3.3.1 工具单元测试模板

```rust
// crates/nova-core/src/tool/builtin/read.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn read_small_file_success() {
        let tool = ReadTool::new();
        let output = tool
            .execute(json!({"path": "README.md"}), None)
            .await;
        assert!(output.is_ok());
    }

    #[tokio::test]
    async fn read_nonexistent_file_returns_error() {
        let tool = ReadTool::new();
        let output = tool
            .execute(json!({"path": "/nonexistent/path"}), None)
            .await;
        assert!(output.is_err());
    }
}
```

---

#### 3.3.2 集成测试架构

```rust
// crates/nova-core/tests/integration
```

**集成测试使用 `tempfile` + 模拟 LLM 客户端：**

```rust
mod mock_client {
    use crate::provider::LlmClient;

    pub struct MockClient;

    impl LlmClient for MockClient {
        async fn generate(&self, prompt: &str) -> Result<String, String> {
            Ok(format!("Mock response for: {}", prompt));
        }
    }
}
```

---

#### 3.3.3 E2E 测试（Tauri + Puppeteer）

```
deskapp/e2e/
├── config/
│   └── playwright.config.ts
├── tests/
│   ├── chat.e2e.spec.ts      # 聊天功能
│   ├── sessions.e2e.spec.ts  # 会话管理
│   └── agents.e2e.spec.ts    # Agent 切换
```

**核心测试场景：**

| 场景 | 断言 |
|------|------|
| 发送消息 | 消息出现在 UI 中 |
| 流式响应 | token 实时追加 |
| 工具调用 | 工具卡片正确显示 |
| 错误恢复 | 错误后继续发送 |

```typescript
// deskapp/e2e/tests/chat.e2e.spec.ts

describe('Chat functionality', () => {
  test('sends message and receives streaming response', async ({ page }) => {
    // 1. 导航到页面
    await page.goto('http://localhost:1420');

    // 2. 输入消息
    await page.fill('#message-input', 'Hello');
    await page.click('#send-button');

    // 3. 断言消息出现在 UI
    await expect(page.locator('.chat-message')).toBeVisible();
  });
});
```

---

### 3.4 CI/CD 完善

#### 3.4.1 完整 Check Cycle

**修改 Makefile.toml：**

```toml
[tasks.check-full]
(command = "cargo clippy --workspace -- -D warnings")
(dependencies = ["fmt", "clippy", "test"])

[tasks.clippy]
command = "cargo clippy --workspace -- -D warnings"

[tasks.test]
command = "cargo test --workspace"
```

---

#### 3.4.2 多平台构建矩阵

**.github/workflows/release.yml 扩展：**

```yaml
jobs:
  build-matrix:
    strategy:
      matrix:
        include:
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            artifact: windows-x86_64
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact: linux-x86_64
          - os: ubuntu-latest
            target: aarch64-unknown-linux-gnu
            artifact: linux-arm64
          - os: macos-latest
            target: x86_64-apple-darwin
            artifact: macos-x86_64
          - os: macos-latest
            target: aarch64-apple-darwin
            artifact: macos-arm64
```

---

#### 3.4.3 rust-toolchain.toml 版本锁定

```toml
# rust-toolchain.toml

[toolchain]
channel = "1.89"  # 锁定具体版本
components = ["rustfmt", "clippy"]
targets = [
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
]
```

---

## 实施顺序建议

| 阶段 | 任务 | 依赖 |
|------|------|------|
| P3.1 | 前端类型同步 | 无 |
| P3.2 | 异步 API 统一 | P3.1 完成 |
| P3.3 | 测试覆盖提升 | P3.2 完成 |
| P3.4 | CI/CD 完善 | P3.3 完成 |

---

## 质量指标

| 指标 | 当前 | 目标 |
|------|------|------|
| Rust 警告数 | 0 | 0（clippy -D warnings） |
| TypeScript 类型注解覆盖率 | ~70% | >95% |
| 单元测试覆盖率 | ~40% | >70% |
| 集成测试数量 | 5 | 20+ |
| E2E 测试覆盖 | 0 | 5+ 场景 |
| CI 构建时间 | N/A | <10 分钟 |
