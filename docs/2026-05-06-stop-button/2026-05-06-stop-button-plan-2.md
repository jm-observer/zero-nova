# Plan 2：UI 与样式

**前置依赖**: Plan 1（状态管理与 `is-stop` CSS 类切换）

---

## 本次目标

1. 将停止图标（■）内嵌到按钮 HTML，用 CSS 控制发送/停止图标的显隐切换
2. 补全/完善 `chat.css` 中与停止状态相关的样式（含过渡动画）
3. 保证切换流畅自然，不引入额外 JavaScript DOM 操作

---

## 涉及文件

| 文件 | 操作 |
|------|------|
| `deskapp/index.html` | 在 `#send-btn` 内追加停止图标 SVG |
| `deskapp/src/styles/main/chat.css` | 补全图标显隐规则、完善过渡动画 |

> `dist/index.html` 是构建产物，由构建流程自动同步，**不需要手动修改**。

---

## 详细设计

### 1. index.html：双图标结构

将按钮内部从单一发送图标改为两个图标并存，通过 CSS 控制显隐：

**修改前**：
```html
<button id="send-btn" class="send-btn">
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
    <path d="M22 2L11 13M22 2l-7 20-4-9-9-4 20-7z" />
  </svg>
</button>
```

**修改后**：
```html
<button id="send-btn" class="send-btn" aria-label="发送">
  <!-- 发送图标（纸飞机）：默认显示 -->
  <svg class="icon-send" width="18" height="18" viewBox="0 0 24 24"
       fill="none" stroke="currentColor" stroke-width="2">
    <path d="M22 2L11 13M22 2l-7 20-4-9-9-4 20-7z" />
  </svg>
  <!-- 停止图标（实心圆角方块）：streaming 时显示 -->
  <svg class="icon-stop" width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
    <rect x="4" y="4" width="16" height="16" rx="3" />
  </svg>
</button>
```

**图标设计说明**：

| 属性 | 发送图标 | 停止图标 |
|------|---------|---------|
| 尺寸 | 18×18 | 16×16（视觉上与纸飞机等大） |
| 样式 | `stroke`（线条风格） | `fill`（实心，更具分量感） |
| 语义 | 纸飞机（发送） | 圆角方块（通用停止符号） |
| rx 值 | - | 3（轻微圆角，与按钮圆形风格协调） |

### 2. chat.css：图标显隐规则

在现有 `.send-btn.is-stop` 块之后追加：

```css
/* 图标显隐切换 */
.send-btn .icon-stop {
  display: none;
}
.send-btn.is-stop .icon-send {
  display: none;
}
.send-btn.is-stop .icon-stop {
  display: block;
}
```

### 3. chat.css：完善现有停止状态样式

现有代码（保留，仅补充）：
```css
.send-btn.is-stop {
  background: #e04040;   /* 已有 */
}
.send-btn.is-stop:hover {
  background: #c03030;   /* 已有 */
}
```

新增停止中（`disabled` + `is-stop`）视觉：
```css
/* 停止中：禁用态叠加停止背景 */
.send-btn.is-stop:disabled {
  opacity: 0.6;
  cursor: not-allowed;
  background: #e04040;  /* 保持红色，opacity 表达禁用 */
}
```

### 4. 过渡动画

现有 `transition: transform 0.15s, background 0.15s, opacity 0.15s;` 已覆盖背景色切换。

补充图标淡入淡出：

```css
.send-btn .icon-send,
.send-btn .icon-stop {
  transition: opacity 0.15s;
}
.send-btn.is-stop .icon-send {
  display: none;  /* display 切换不能动画，用 opacity 过渡更平滑 */
}
```

> **注意**：`display: none` 不支持 CSS 过渡，若需要淡入淡出效果，改用 `opacity + pointer-events` 方案：
>
> ```css
> .send-btn .icon-send,
> .send-btn .icon-stop {
>   position: absolute;
>   transition: opacity 0.12s ease;
> }
> .send-btn {
>   position: relative;  /* 为 absolute 子元素提供定位上下文 */
> }
> .send-btn .icon-stop {
>   opacity: 0;
>   pointer-events: none;
> }
> .send-btn.is-stop .icon-send {
>   opacity: 0;
>   pointer-events: none;
> }
> .send-btn.is-stop .icon-stop {
>   opacity: 1;
>   pointer-events: auto;
> }
> ```
>
> 选择哪种方案（`display` 直接切换 vs `opacity` 淡入淡出）由实现时根据视觉效果决定。`display` 方案更简单，`opacity` 方案更精细。

### 5. 视觉状态汇总

| 状态 | 按钮背景 | 显示图标 | 可交互 | `aria-label` |
|------|---------|---------|-------|-------------|
| IDLE | `--color-primary`（蓝紫） | 纸飞机 | 是 | 发送 |
| STREAMING | `#e04040`（红） | 实心方块 | 是 | 停止生成 |
| STOPPING | `#e04040`（红），`opacity: 0.6` | 实心方块 | 否 | 停止中... |

---

## 测试案例

| 编号 | 场景 | 预期结果 |
|------|------|---------|
| T1 | 初始页面加载 | 显示蓝紫发送按钮 + 纸飞机图标 |
| T2 | 发送消息后 | 按钮变红 + 显示方块图标，纸飞机消失 |
| T3 | LLM 回复完成 | 按钮恢复蓝紫 + 纸飞机图标 |
| T4 | 点击停止按钮 | 按钮红色但半透明（opacity 0.6），不可点击 |
| T5 | 停止完成后 | 按钮恢复蓝紫 + 纸飞机图标 |
| T6 | 深色模式（若支持） | 颜色变量自适应，红色保持清晰对比度 |
| T7 | 按钮 hover（streaming 状态） | 红色加深（`#c03030`） |
| T8 | 无障碍：键盘焦点 + Tab | `aria-label` 正确读出当前状态文字 |
