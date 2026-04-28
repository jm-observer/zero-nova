# Plan 1：执行链路与环境接入

## Plan 编号与标题
- Plan 1：执行链路与环境接入

## 前置依赖
- 无

## 本次目标

在不改动业务功能语义的前提下，补齐 `deskapp/e2e` 的运行基础设施，使开发者可以在 Windows 环境中按统一入口执行 E2E。

可验证标准：
- 项目存在明确的 E2E 启动脚本
- 本地安装后可调用 `Playwright` CLI
- `Playwright` 配置可从 `deskapp` 根目录直接解析并启动开发服务器
- 文档中明确浏览器安装和执行前提

## 涉及文件

- `deskapp/package.json`
- `deskapp/e2e/playwright.config.ts`
- `deskapp/README.md`
- `deskapp/README_zh.md`

如需补充环境说明，也可新增：

- `deskapp/e2e/README.md`

## 详细设计

### 1. 依赖补齐策略

为 `deskapp` 增加 `Playwright` 测试依赖，统一放在前端工程自身的 `devDependencies` 中，而不是放到 workspace 根目录。

原因：
- E2E 只服务于 `deskapp`
- 可避免污染 Rust workspace 的关注边界
- 与现有 `vite`、`vitest` 的管理方式一致

建议新增内容：
- `@playwright/test`

此处仅提出设计，不在文档阶段直接安装依赖。

### 2. 统一脚本入口

在 `deskapp/package.json` 中增加以下脚本职责：

- `test:e2e`：执行全部 E2E
- `test:e2e:headed`：本地可视模式运行，便于调试
- `test:e2e:ui`：Playwright UI 模式，便于录制和定位失败

设计原则：
- 所有脚本都从 `deskapp` 目录执行
- 不要求开发者手动先起 `vite`，统一交给 `webServer.command`
- 避免把浏览器安装逻辑塞进测试命令本身，降低首次执行歧义

### 3. Playwright 配置收敛

当前 `playwright.config.ts` 已具备基本结构，但需收敛为“Windows 本地可调试优先”的配置：

- 保持 `chromium` 为首个项目
- 保留 `trace: 'on-first-retry'`
- 保留 `screenshot: 'only-on-failure'`
- 明确 `testDir`、`webServer.command`、`port`、`reuseExistingServer`
- 如有需要，可将 `baseURL` 改为统一常量，避免散落在测试文件中

同时建议所有测试统一使用相对导航：
- 从 `page.goto('/')` 启动

而不是在每个测试中硬编码 `http://localhost:1420`。

### 4. 浏览器安装与 Windows 约束说明

由于 Windows 环境首次运行通常卡在浏览器二进制未安装，因此需要在文档中明确以下步骤：

1. 安装前端依赖
2. 安装 `Playwright` 浏览器
3. 运行 `test:e2e`

说明重点：
- Windows 是受支持平台
- 若 `pnpm` 已锁定为项目推荐包管理器，应优先使用 `pnpm`
- 首次运行慢于日常回归属于正常现象

### 5. 与 Tauri 的边界约定

本轮不直接把 E2E 目标提升为“驱动完整 Tauri 桌面进程”，而是先验证 Web 壳层。

原因：
- 当前 `playwright.config.ts` 已以 `vite` dev server 为中心
- 真实 Tauri 进程会引入更多系统权限、窗口管理和 IPC 初始化问题
- 对当前目标“先让测试可跑”来说，Web 壳层足以发现多数前端交互退化

后续若要验证 Tauri 特定能力，可追加独立计划，不混入本轮实施。

## 测试案例

### 正常路径
- 在 Windows 下执行 E2E 命令，`Playwright` 可正常发现测试文件
- 测试启动时可自动拉起 `pnpm dev`
- 测试结束后能生成标准报告与失败截图/trace

### 边界条件
- 本地已存在 `1420` 端口服务时，`reuseExistingServer` 行为符合预期
- 在 CI 环境下自动关闭多 worker，避免资源竞争

### 异常场景
- 未安装浏览器时，文档能指导开发者快速补齐
- 缺失 `@playwright/test` 时，错误信息能从脚本入口直接暴露，而不是隐式失败

