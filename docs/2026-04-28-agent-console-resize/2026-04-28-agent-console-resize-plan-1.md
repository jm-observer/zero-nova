# Plan 1: 控制台拖拽交互与样式适配

## 前置依赖
- 无

## 本次目标
- 为 `agent-console` 增加左侧拖拽手柄。
- 桌面端拖拽时实时调整控制台宽度。
- 宽度限制在合理范围内，避免挤压主工作区。
- 将宽度持久化到本地存储，并在初始化时恢复。

## 涉及文件
- `deskapp/src/ui/templates/agent-console-template.ts`
- `deskapp/src/ui/agent-console-view.ts`
- `deskapp/src/styles/main/agent-console.css`

## 详细设计
- 模板层：在 `agent-console` 内加入独立的 resize handle，放在最左侧边缘。
- 样式层：
  - 控制台宽度改为 CSS 变量 `--agent-console-width` 驱动，默认 `360px`。
  - `#workspace.console-open` 的 `margin-right` 也改为同一变量，确保两侧同步。
  - 拖拽手柄只在桌面端显示，移动端隐藏。
- 交互层：
  - 在 `AgentConsoleView` 中绑定 `mousedown / mousemove / mouseup`。
  - 拖拽时基于起始宽度和鼠标水平位移计算新宽度。
  - 限制最小宽度和最大宽度，避免过窄或过宽。
  - 拖拽结束时写入 `localStorage`。
  - 初始化时从 `localStorage` 恢复，缺省回退到默认宽度。
- 兼容策略：
  - 仅桌面端启用拖拽；窄屏继续沿用覆盖式面板。
  - 关闭/打开控制台不重置已保存宽度。

## 测试案例
- 正常路径：桌面端拖动控制台，宽度实时变化，聊天区同步收缩/恢复。
- 持久化：刷新页面后控制台宽度保持上次值。
- 边界条件：拖到最小/最大值时被正确夹紧。
- 兼容场景：移动端不显示拖拽手柄，控制台仍按原逻辑展示。
