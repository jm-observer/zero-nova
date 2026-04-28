# 2026-04-28 Deskapp E2E Enable 详细设计

## 时间
- 创建时间：2026-04-28
- 最后更新：2026-04-28

## 项目现状

当前 `deskapp/e2e` 已存在 `Playwright` 配置和 3 组端到端测试草案，覆盖聊天、会话管理和 Agent 切换场景，但整体仍停留在“结构已搭好、尚未接入可执行链路”的状态。

现状确认如下：

1. 目录与测试草案已存在
   - `deskapp/e2e/playwright.config.ts`
   - `deskapp/e2e/tests/chat.e2e.spec.ts`
   - `deskapp/e2e/tests/sessions.e2e.spec.ts`
   - `deskapp/e2e/tests/agents.e2e.spec.ts`

2. 运行链路未接通
   - `deskapp/package.json` 中未声明 `@playwright/test`
   - 未提供 `test:e2e` 等脚本入口
   - 本地 `node_modules` 中不存在 `playwright` 可执行文件

3. 测试脚本与当前 DOM 存在错位
   - 已存在：`#message-input`、`#new-session-btn`、`#agent-list`、`#session-list`
   - 实际发送按钮为 `#send-btn`，但测试中使用 `#send-button`
   - 实际确认弹窗根节点为 `#confirm-modal` / `#confirm-dialog-overlay`，但测试中使用 `#confirm-dialog`
   - 测试使用的 `#new-agent-btn`、`#agent-modal`、`.validation-error`、`.token-stream` 等选择器当前未在页面源码中落地

4. 当前前端已有单元测试基础
   - `deskapp/src/__tests__` 已使用 `Vitest`
   - 部分测试已对 `@tauri-apps/api/core` 的 `invoke` 进行 mock
   - 说明前端已有“可测试性”基础，但 E2E 所需的端到端稳定夹具仍未补齐

5. Windows 平台可作为首个目标平台
   - `Playwright` 自身支持 Windows
   - 当前 E2E 配置使用 `chromium` + `pnpm dev` + `http://localhost:1420`
   - 因此阻塞点不在平台兼容性，而在依赖、脚本、测试夹具和选择器契约未完成

## 整体目标

本次设计的最终目标是：

- 让 `deskapp/e2e` 在 Windows 开发环境下可稳定执行
- 将现有 E2E 从“示意性脚本”收敛为“可重复、可诊断、可维护”的测试套件
- 明确前端 DOM 测试契约，避免测试与 UI 演进长期漂移
- 为后续在 CI 中接入跨平台 E2E 保留扩展路径，但本轮优先保证 Windows 本地可跑

本次设计不直接扩展业务功能，重点解决以下问题：

- 如何补齐 `Playwright` 运行入口
- 如何让测试不依赖真实外部网关、真实模型响应和随机初始状态
- 如何修正现有测试选择器与页面实现不一致的问题
- 如何定义最小可用的 E2E 场景，先跑通，再逐步扩面

## Plan 拆分

### Plan 1：执行链路与环境接入
- 目标：补齐 `Playwright` 依赖、脚本入口、浏览器安装指引和本地执行约定
- 产出：E2E 可被开发者在 Windows 上一条命令启动
- 依赖：无
- 顺序：第一步

### Plan 2：测试契约与夹具稳定化
- 目标：为现有页面建立稳定选择器契约、统一等待策略，并引入可控测试夹具
- 产出：测试不再直接依赖易变样式类名、随机文案或真实后端状态
- 依赖：Plan 1
- 顺序：第二步

### Plan 3：场景重写与 CI 收口
- 目标：按当前实现重写聊天/会话/Agent 三组用例，并为后续 CI 接入提供执行约束
- 产出：最小稳定用例集、失败诊断产物和接入流程说明
- 依赖：Plan 1、Plan 2
- 顺序：第三步

## 风险与待定项

### 已知风险
- 当前页面存在初始化引导、登录态、网关连接、Tauri API 等多重启动条件，若无测试夹具，E2E 很容易被环境噪音干扰。
- 现有测试草案部分断言依赖中文文案，如 `New Chat`，存在国际化漂移风险。
- 若继续使用“视觉 class 名 + 隐式时序等待”的方式编写测试，后续 UI 微调会频繁打断测试。

### 待确认事项
- E2E 的首选运行目标是“纯 Web 壳层”还是“真正的 Tauri Desktop 进程”。本设计默认优先跑 Web 壳层，因为实现成本更低、反馈更快。
- 是否接受为前端增加少量 `data-testid` 或稳定 `id`。若不接受，需要额外设计页面对象层来降低选择器脆弱性。
- 是否允许新增前端开发依赖 `@playwright/test`。本设计会提出该依赖，但不在文档阶段直接落库。

### 执行顺序建议
- 先让 E2E “能启动”
- 再让 E2E “不随机失败”
- 最后再扩大覆盖范围

