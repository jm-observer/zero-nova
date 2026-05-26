# Plan 5: zero-nova 发新 nova tag + zero 升级

## 前置依赖

Plan 4（修复循环全绿）

## 任务目标

- 把 Plan 1–4 的 commits 合并提交，bump nova 版本号到下一个补丁版本（如 `v0.3.18`）。
- 创建并推送 tag。
- 在 `D:\git\zero` 那边把 `nova-agent` 依赖指向新 tag，跑 zero 自身的修复循环全绿。

## 执行范围

**zero-nova 侧**：

- `Cargo.toml`（workspace root）：bump `version` 字段（如有 nova workspace 统一版本）。如果 nova 的 git tag 与 crate version 解耦，仅打 tag 即可。
- `CHANGELOG`（若存在）：增一条 `### 2026-05-26 v0.3.18 — title generator: LLM-backed injection`。
- git commit + push + tag。

**zero 侧**：

- 检查 `D:\git\zero\Cargo.toml` 中 nova-agent 的 git tag 引用方式（user memory 提示是 git tag 依赖）。
- `cargo update -p nova-agent`（或更精确地按 nova workspace 名）使锁文件指向新 tag。
- 跑 zero 修复循环：`cargo clippy --workspace -- -D warnings && cargo fmt --check --all && cargo test --workspace`。
- 若 zero 这边因为 `SessionService` clone 行为变化或 title 默认 fallback 行为差异导致测试失败，定位并修复。

## Agent 执行步骤

1. 在 zero-nova：
   ```bash
   cd D:/git/zero-nova
   git add -A
   git commit -m "feat(title): LLM-backed title generator + dependency-inversion injection"
   git push origin main
   # 确认下一个版本号
   git tag v0.3.18
   git push origin v0.3.18
   ```
2. 在 zero：
   ```bash
   cd D:/git/zero
   # 编辑 Cargo.toml 中所有 nova-* git 依赖的 tag 字段为 "v0.3.18"
   cargo update -p nova-agent -p nova-protocol -p nova-gateway-core -p nova-agent-config -p nova-agent-loader -p nova-skill-loader
   cargo clippy --workspace -- -D warnings
   cargo fmt --check --all
   cargo test --workspace
   ```
3. 若编译/测试失败：
   - 编译失败：通常是 SessionService 构造方式被外部直接调到（zero 侧不应该用到 SessionService::new，由 nova-agent-loader 间接装配）；定位 caller，按新 API 调整。
   - 测试失败：检查 zero 这边是否 mock 了 title 调度链路，若有，按 Plan 1 的 mock 策略调整。
4. zero 修复循环全绿后 commit 升级。

## 行为规则

| 场景 | 期望 |
|------|------|
| zero 依赖升级到 v0.3.18 | nova title 生成默认走 LLM；fallback 在 LLM 失败时启用 |
| zero 内现有 title 相关测试（如有） | 应该全绿；若有断言用户消息拼接的，需调整 |
| zero `cargo update -p nova-agent` 后 lock 文件指向 v0.3.18 commit | git diff Cargo.lock 仅显示 nova-* 版本字段变化 |

## 禁止事项

- 禁止使用 `--no-verify` 跳过 hooks。
- 禁止在 zero 修复循环未跑通前 commit 升级。
- 禁止在 zero-nova 这边出现未提交的 docs 改动就发 tag（设计文档要在打 tag 前就 commit）。

## 测试要求

- zero-nova：`cargo clippy + fmt + test` 三件套全绿（Plan 4 已验证）
- zero：`cargo clippy --workspace -- -D warnings && cargo fmt --check --all && cargo test --workspace` 三件套全绿

## 完成条件

- [ ] zero-nova v0.3.18 tag 已 push
- [ ] zero `Cargo.toml` 引用 v0.3.18
- [ ] zero 三件套全绿
- [ ] zero commit 已提交（不含 push，按用户授权）
