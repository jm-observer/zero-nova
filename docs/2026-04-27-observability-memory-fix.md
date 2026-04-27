# 可观测性与记忆子系统找回修复

**日期**: 2026-04-27

**依据**: 2026-04-26 设计的 DeskApp Agent 工作台能力文档
- `docs/old/2026-04-26-deskapp-agent-observability-and-control.md`（总览）
- `docs/old/2026-04-26-deskapp-agent-observability-and-control-backend.md`（后端设计）
- `docs/old/2026-04-26-deskapp-agent-observability-and-control-plan-1.md`（Plan 1）
- `docs/old/2026-04-26-deskapp-agent-observability-and-control-plan-2.md`（Plan 2）
- `docs/old/2026-04-26-deskapp-agent-observability-and-control-plan-3.md`（Plan 3）
- `docs/old/2026-04-26-deskapp-agent-observability-and-control-plan-4.md`（Plan 4）
- `docs/old/2026-04-26-deskapp-agent-observability-and-control-plan-5.md`（Plan 5）
- `docs/old/2026-04-26-deskapp-agent-observability-and-control-backend-plan-1.md`（后端 Plan 1）
- `docs/old/2026-04-26-deskapp-agent-observability-and-control-backend-plan-2.md`（后端 Plan 2）
- `docs/old/2026-04-26-deskapp-agent-observability-and-control-backlog.md`（遗留问题）
- `docs/old/2026-04-26-deskapp-agent-observability-and-control-implementation-review.md`（实施评审）

---

## 1. 现状概述

### 1.1 问题根因

在 `265d08eb`（设计文档创建当天的 HEAD，commit 时间 2026-04-26 02:38:03）之后，项目经历了 16+ 个 commit 的重构。其中关键变化包括：

- crate 拆分（`nova-core` → `nova-core` + `nova-conversation` + `nova-app`）
- crate 统一（`nova-app`/`nova-server` 被合并进协议层）
- Gateway handler 重构
- `observability.rs` 在主分支（`561d356`）中被创建但 **未被正确合并到当前 crate 结构中**
- `envelope.rs` 消息枚举在重构中被重写，丢失了设计文档定义的大部分新消息类型

### 1.2 版本时间线

```
265d08eb ────────────── HEAD (当前)
  │                     │
  │                     └─ 16 commits 延迟
  │                     └─ 但 HEAD tree 不包含 observability.rs
  │
  └── 后续 commits ──── 561d356 (main 最新)
                         │
                         └─ observability.rs (406 行) 创建于此处
                         └─ envelope.rs 扩展为 140+ 消息类型
                         └─ Gateway handler 聚合完成
```

**关键发现**: `observability.rs` 存在于 `561d356` 的 git 树中，但当前 HEAD (`265d08eb`) 的 tree 不包含该文件。它存放在 `.worktrees/plan3-crate-consolidation/crates/nova-protocol/src/observability.rs` 下。

### 1.3 影响范围

| 层级 | 受影响模块 | 严重程度 |
|------|-----------|---------|
| 协议层 | envelope.rs, observability.rs | 高 |
| 后端服务 | AgentWorkspaceService, RunTracker | 高 |
| 持久化 | SQLite 表格, migrations | 中 |
| 前端状态 | ResourceState 缓存, 会话隔离 | 中 |
| 事件桥接 | bridge.rs APP event → gateway event | 高 |

---

## 2. 修复 Plan

### 2.1 总览

```
┌─────────────────────────────────────────────────────────────┐
│                    总览文档完成                 │
│                                                             │
│  ┌─ Plan 1 ────────────────┐    ┌─ Plan 3 ────────────────┐ │
│  │ 协议层：observability     │    │ 持久化层：新增 SQLite    │ │
│  │ + envelope.rs 扩展        │    │   表格 + migrations      │ │
│  └──────────────────────────┘    └─────────────────────────┘ │
│           ↓                            ↓                      │
│           └──→ Plan 2 ←──────────────────┘                  │
│           后端服务：AgentWorkspaceService + RunTracker        │
│           ↓                                                   │
│           └──→ Plan 4 ←──────────────────┘                  │
│           事件桥接 + Gateway handler                           │
│           ↓                                                   │
│           └──→ Plan 5 ←──────────────────┘                  │
│           前端改进 + 测试                                       │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. 具体 Plan 详情

> **注意**: 由于总览文件和 Plan 子文件已在 `docs/old/` 中详细定义，本修复文档直接引用已有设计，重点梳理 **哪些代码已存在但功能不完整、哪些已丢失、需要如何恢复**。

### 3.1 Plan 1: 协议层修复

**前置依赖**: 无

**涉及文件**:
- `crates/nova-protocol/src/observability.rs`（创建或恢复）
- `crates/nova-protocol/src/envelope.rs`（扩展 MessageEnvelope 枚举）
- `crates/nova-protocol/src/lib.rs`（添加 observability 模块导出）

#### 3.1.1 observability.rs

从 `561d356` 恢复完整的 observability.rs 定义（参考后端 Plan 1 的 Plan 2）。

**核心模块划分**:

| 分组 | 内容 | 引用文档 |
|------|------|---------|
| Plan 1: Runtime Snapshots | AgentInspect, SessionRuntime, PromptPreview, ToolAvailability, SkillBinding, MemoryHit, TokenUsage | 后端 Plan 1 §3 |
| Plan 2: Execution Records | RunRecord, RunStepRecord, ArtifactRecord, PermissionRequestRecord, AuditLogRecord, DiagnosticIssueRecord | 后端 Plan 2 §1 |
| Plan 2: Control & Restore | RunControlRequest, WorkspaceRestoreResponse | 后端 Plan 2, Plan 5 |

#### 3.1.2 当前 envelope.rs 缺失的消息类型

当前 envelope.rs 仅有 20 种消息类型，设计文档定义了 **140+ 种**。需要新增的关键消息：

```rust
// 核心可观测性消息（缺失）
SessionRuntime*      - 会话运行态快照
SessionMemoryHits    - 记忆命中
SessionModelOverride - 模型覆盖
SessionTokenUsage    - Token 统计
```

#### 3.1.3 Log

---

## 4. 实施顺序总览

```
1. Plan 1: 协议层修复 (observability.rs + envelope.rs)
   ↓
2. Plan 2: 持久化层修复 (SQLite 表格 + migrations)
   ↓
3. Plan 3: 后端服务修复 (AgentWorkspaceService + RunTracker)
   ↓
4. Plan 4: 事件桥接 (bridge.rs + router.rs + handlers)
   ↓
5. Plan 5: 前端改进 + 测试
```

---

## 5. 验收标准

| 编号 | 标准 | 对应 Plan |
|------|------|---------|
| 1 | `observability.rs` 模块加载，所有类型可导入 | Plan 1 |
| 2 | envelope.rs 包含全部 140+ 消息类型 | Plan 1 |
| 3 | `ChatCompletePayload` 正确返回 `Usage` | Plan 3 |
| 4 | 新增 7 个 SQLite 表格存在且可查询 | Plan 2 |
| 5 | Gateway handler 处理新增 8 个接口 | Plan 4 |
| 6 | 8 种新事件通过 bridge 正确桥接 | Plan 4 |
| 7 | frontend Skill 缓存按 sessionId 隔离 | Plan 5 |
| 8 | Prompt/Memory 加载解耦 | Plan 5 |
| 9 | Token 校正按会话维度 | Plan 5 |
