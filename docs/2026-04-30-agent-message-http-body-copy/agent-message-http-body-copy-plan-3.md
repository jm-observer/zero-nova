# Plan 3: 前端按钮交互与复制行为

## 前置依赖
- Plan 1: 元数据契约与消息结构扩展。
- Plan 2: Provider HTTP Body 采集与持久化链路。

## 本次目标
在每条 assistant 消息旁新增“复制请求 body / 复制响应 body”两个按钮，读取 metadata 中的 trace 数据并以 pretty JSON 写入剪贴板。

## 涉及文件
- `deskapp/src/ui/chat-view.ts`
- `deskapp/src/styles/main/chat.css`（或消息样式对应文件）
- `deskapp/src/i18n/zh.ts`
- `deskapp/src/i18n/en.ts`
- `deskapp/src/__tests__/chat-view-*.test.ts`（现有消息渲染测试文件）

## 详细设计
1. 按钮展示规则：
- 仅 assistant 消息渲染按钮组：`复制请求 body`、`复制响应 body`。
- 所有用户可见；若缺少对应 body，按钮置灰但保留位置，避免布局抖动。

2. 数据读取：
- 从 `message.metadata.providerHttpTrace` 读取 `requestBody` 和 `responseBody`。
- 读取前校验 `boundMessageId === message.id`，不一致则视为不可用并给出错误提示。

3. 复制行为：
- 使用 `JSON.stringify(value, null, 2)` 生成 pretty JSON。
- 调用剪贴板 API 复制。
- 成功：toast “已复制请求 body/已复制响应 body”。
- 失败：toast “复制失败”，并记录前端错误日志。

4. 交互细节：
- 防重复点击：点击后短暂禁用 300ms。
- 长文本不在 UI 中展开预览，避免聊天区噪音。

5. 无痕兼容：
- 历史消息无 trace 时不报错，按钮不可用态。
- 保持现有消息内容、工具卡片、流式渲染逻辑不变。

## 测试案例
1. 正常路径：有 request/response body 时，两个按钮均可复制出 2 空格缩进 JSON。
2. 边界路径：仅有 requestBody 时，“复制响应 body”置灰且提示不可用。
3. 错误路径：剪贴板写入失败，展示失败提示且不影响聊天区其他交互。
4. 一致性路径：`boundMessageId` 不匹配时禁止复制并提示数据异常。
5. 回放路径：刷新后重新进入会话，历史 assistant 消息仍可复制。
