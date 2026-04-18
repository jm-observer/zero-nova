# Web Search 结果处理流程

> 基于 `targets/claw-code/rust/crates/tools/src/lib.rs` 和 `rusty-claude-cli/src/main.rs` 的分析。

## 1. 搜索引擎与请求

- 默认使用 **DuckDuckGo** (`https://html.duckduckgo.com/html/`) 作为搜索后端 (`lib.rs:2866`)
- 可通过环境变量 `CLAWD_WEB_SEARCH_BASE_URL` 替换为自定义搜索引擎 (`lib.rs:2860`)
- HTTP client 设置 20 秒超时、最多 10 次重定向 (`lib.rs:2836-2838`)

## 2. HTML 解析与结果提取

入口函数 `execute_web_search()` (`lib.rs:2781`) 使用**纯手写的 HTML 解析器**（非 DOM 库），分两级提取：

1. **`extract_search_hits()`** (`lib.rs:2981`) — 查找 CSS class `result__a` 的 `<a>` 标签（DuckDuckGo 特有的结果标记），提取 `href` 和 anchor 文本作为 `SearchHit { title, url }`
2. **`extract_search_hits_from_generic_links()`** (`lib.rs:3018`) — 如果第一级提取为空，回退为提取所有 `<a href="http(s)://...">` 链接作为兜底

DuckDuckGo 的链接是重定向 URL（如 `/l/?uddg=实际URL`），通过 `decode_duckduckgo_redirect()` (`lib.rs:3070`) 解码 `uddg` 参数得到真实 URL。

## 3. 结果过滤与去重

```
allowed_domains → 白名单过滤 (lib.rs:2798)
blocked_domains → 黑名单过滤 (lib.rs:2801)
dedupe_hits()   → 按 URL 去重   (lib.rs:3125)
truncate(8)     → 最多保留 8 条  (lib.rs:2806)
```

域名匹配支持子域名（如 filter `example.com` 同时匹配 `sub.example.com`）(`lib.rs:3106-3108`)。

## 4. 输出格式

最终返回 `WebSearchOutput`，其 `results` 包含两个元素：

- **`Commentary(String)`** — 一段 Markdown 文本，格式为：
  ```
  Search results for "query". Include a Sources section in the final answer.
  - [Title1](url1)
  - [Title2](url2)
  ```
- **`SearchResult { tool_use_id, content: Vec<SearchHit> }`** — 结构化的搜索命中列表

## 5. REPL/CLI 端的渲染

在 `rusty-claude-cli/src/main.rs` 中：

- **发起时**：`format_tool_call_start("WebSearch", input)` 显示 query 字符串
- **结果回来后**：走通用的 `format_generic_tool_result()` 路径（Web Search 没有专门的格式化逻辑），最终通过 `TerminalRenderer::stream_markdown()` 将 Markdown 渲染为终端 ANSI 彩色输出
- 结果被序列化为 JSON 后转换为 `ContentBlock::ToolResult`，作为 `ToolResultContentBlock::Text` 送回 API 上下文

## 6. WebFetch（相关但独立）

`execute_web_fetch()` (`lib.rs:2747`) 用于获取单个 URL：
- URL 自动升级到 HTTPS
- HTML 内容通过手写的 `html_to_text()` (`lib.rs:2929`) 剥离标签转为纯文本
- 根据 prompt 关键词（"title" / "summary"）决定返回策略，文本预览最多 600-900 字符

## 7. 搜索结果中的 URL 是否会被自动访问？

**不会。** Web Search 和 Web Fetch 是两个完全独立的工具：

- `WebSearch` 只返回搜索结果列表（标题 + URL），**不会自动访问**任何结果 URL
- 搜索结果以 Markdown 链接形式返回给模型上下文后，由模型自行决定是否调用 `WebFetch` 来访问具体 URL
- 两者之间没有自动串联逻辑，模型可以选择只使用搜索结果的标题和 URL 信息来回答问题，也可以进一步 fetch 某个 URL 获取详细内容

## 总结

整个方案是一个**纯本地实现**——不依赖任何搜索 API key，直接抓取 DuckDuckGo HTML 页面，用手写解析器提取结果，经过域名过滤/去重/截断后以 Markdown 格式返回给模型上下文。CLI 端没有对搜索结果做特殊渲染，统一走 Markdown 渲染管线。
