# 联网搜索工具重构设计方案 (Search Tool Refactoring)

## 1. 背景与目标

### 1.1 现状分析

当前 `web_search.rs` 已实现双后端支持（Google CSE + Brave Search），通过环境变量自动切换。但存在以下问题：

| 问题 | 描述 |
| :--- | :--- |
| **后端选择有限** | Brave Search 免费额度有限且国内可用性差，缺少免费兜底方案 |
| **架构耦合** | Google 和 Brave 的请求/解析逻辑直接硬编码在 `execute()` 中，`match` 分支会随后端增多膨胀 |
| **结果格式不统一** | 各后端的 JSON 响应字段不同（`items` vs `web.results`），解析逻辑分散 |
| **缺乏 LLM 优化** | 搜索结果仅返回 title/url/snippet，未针对 LLM 上下文做优化（如正文提取） |

### 1.2 目标

将 `WebSearchTool` 重构为策略模式架构，支持多后端热插拔，优先保障 Google CSE 的完整功能。

### 1.3 支持的后端对比

| 后端 | 费用 / 免费额度 | 优点 | 缺点 |
| :--- | :--- | :--- | :--- |
| **Google CSE** | 100次/天 (免费) | 结果最权威，可配置垂直搜索 (GitHub/HF) | 配置繁琐，需 API Key 和 CX ID |
| **Tavily** | 1000次/月 (免费) | 专为 LLM 优化，结果自带清洗过的正文 | 有月度额度限制 |
| **DuckDuckGo** | 完全免费 | 无需 API Key，隐私性好 | 稳定性略低于付费 API，速度稍慢 |

> **决策：淘汰 Brave Search**，用 Tavily（LLM 优化）和 DuckDuckGo（免费兜底）替代。

---

## 2. 核心架构设计

采用 **策略模式 (Strategy Pattern)**，将每个搜索后端实现为独立的 `SearchBackend` trait 对象。

### 2.1 Trait 定义

```rust
/// 统一搜索结果
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// 搜索后端 trait
#[async_trait]
pub trait SearchBackend: Send + Sync {
    /// 返回后端名称，用于日志和 description 动态生成
    fn name(&self) -> &str;

    /// 执行搜索，返回统一格式的结果列表
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>>;
}
```

### 2.2 WebSearchTool 结构重构

```rust
/// 重构后的 WebSearchTool，持有一个 trait object
pub struct WebSearchTool {
    backend: Box<dyn SearchBackend>,
    client: Client, // 共享的 reqwest::Client
}

impl WebSearchTool {
    /// 从环境变量自动感知并创建，优先级：Google > Tavily > DuckDuckGo
    pub fn from_env() -> Self {
        let client = Client::new();
        let backend: Box<dyn SearchBackend> = if /* Google env vars exist */ {
            Box::new(GoogleBackend::new(api_key, cx, client.clone()))
        } else if /* Tavily env var exists */ {
            Box::new(TavilyBackend::new(api_key, client.clone()))
        } else {
            Box::new(DuckDuckGoBackend::new(client.clone()))
        };
        Self { backend, client }
    }
}
```

**关键变化**：`from_env()` 返回 `Self` 而非 `Result<Self>`，因为 DuckDuckGo 作为兜底永远可用，注册逻辑不再需要 `match`。

### 2.3 自动感知优先级

| 优先级 | 环境变量 | 后端 |
| :--- | :--- | :--- |
| 1 (最高) | `GOOGLE_SEARCH_API_KEY` + `GOOGLE_SEARCH_CX` | Google CSE |
| 2 | `TAVILY_API_KEY` | Tavily |
| 3 (兜底) | *(无需配置)* | DuckDuckGo |

### 2.4 文件组织

重构后按模块拆分文件：

```
src/tool/builtin/
├── web_search/
│   ├── mod.rs          # WebSearchTool 主体 + from_env + Tool trait impl
│   ├── types.rs        # SearchResult, SearchBackend trait
│   ├── google.rs       # GoogleBackend 实现
│   ├── tavily.rs       # TavilyBackend 实现
│   └── duckduckgo.rs   # DuckDuckGoBackend 实现
├── mod.rs
├── bash.rs
├── file_ops.rs
└── web_fetch.rs
```

> 将 `web_search.rs` 单文件升级为 `web_search/` 模块目录，每个后端一个文件，职责清晰。

---

## 3. 垂直搜索优化策略

针对本项目，Google CSE Backend 将配置为专注于以下高质量技术站点：

* `github.com/*`
* `huggingface.co/*`
* `docs.rs/*`

**结论记录**：通过在 Google Programmable Search Engine 控制台配置特定的 CX ID，LLM 的搜索结果将集中在代码实现、AI 模型和官方文档，大幅减少垃圾信息的干扰。

---

## 4. 各后端 API 细节

### 4.1 Google CSE

- **Endpoint**: `https://www.googleapis.com/customsearch/v1`
- **认证**: Query param `key=<API_KEY>`
- **参数**: `q`, `cx`, `num` (1-10), `start` (分页偏移)
- **响应路径**: `items[].{title, link, snippet}`
- **环境变量**:
  - `GOOGLE_SEARCH_API_KEY` (必须)
  - `GOOGLE_SEARCH_CX` (必须)
  - `GOOGLE_SEARCH_ENDPOINT` (可选，覆盖默认 endpoint)

### 4.2 Tavily

- **Endpoint**: `https://api.tavily.com/search`
- **认证**: JSON body 中的 `api_key` 字段
- **方法**: POST
- **请求体**:
  ```json
  {
    "api_key": "<TAVILY_API_KEY>",
    "query": "search terms",
    "max_results": 5,
    "search_depth": "basic"
  }
  ```
- **响应路径**: `results[].{title, url, content}`
- **特色**: `content` 字段包含清洗过的正文摘要，对 LLM 更友好
- **环境变量**:
  - `TAVILY_API_KEY` (必须)

### 4.3 DuckDuckGo

- **方案**: 使用 DuckDuckGo HTML 搜索页面解析（无官方 API）
- **Endpoint**: `https://html.duckduckgo.com/html/`
- **方法**: POST, `application/x-www-form-urlencoded`
- **参数**: `q=<query>`
- **解析**: 使用 `scraper` crate（项目已引入）解析 HTML 提取结果
  - 标题: `.result__a` 选择器
  - URL: `.result__a[href]` 属性
  - 摘要: `.result__snippet` 选择器
- **环境变量**: 无需

---

## 5. 实施计划 (Implementation Plans)

### Plan 1: 架构重构 — Trait 抽象与 Google CSE 后端

> **优先级: 最高** | 涉及文件: `web_search.rs` → `web_search/` 模块

#### 目标
将现有单文件重构为模块化策略架构，保留并完善 Google CSE 作为首个后端实现。

#### 详细步骤

1. **创建模块目录结构**
   - 新建 `src/tool/builtin/web_search/` 目录
   - 创建 `types.rs`：定义 `SearchResult` struct 和 `SearchBackend` trait
   - 创建 `mod.rs`：`WebSearchTool` 主体，持有 `Box<dyn SearchBackend>`

2. **实现 `google.rs`**
   - 将现有 `SearchProvider::Google` 分支逻辑迁移到 `GoogleBackend` struct
   - 实现 `SearchBackend` trait
   - 保留现有环境变量约定：`GOOGLE_SEARCH_API_KEY`, `GOOGLE_SEARCH_CX`, `GOOGLE_SEARCH_ENDPOINT`

3. **重构 `WebSearchTool`**
   - `from_env()` 暂时只检测 Google 环境变量，未配置时返回 `Err`（DuckDuckGo 在 Plan 3 实现后改为兜底）
   - `Tool::execute()` 委托给 `self.backend.search()`，统一格式化 `Vec<SearchResult>` 为文本输出
   - `Tool::definition()` 中 description 根据 `self.backend.name()` 动态生成

4. **更新 `builtin/mod.rs`**
   - 将 `mod web_search;` 保持不变（Rust 模块系统自动识别目录）
   - 注册逻辑暂不变

5. **删除旧代码**
   - 移除 `SearchProvider` enum
   - 移除 Brave Search 相关逻辑

#### 验收标准
- [x] `cargo build` 通过
- [x] 设置 Google 环境变量后，`web_search` 工具行为与重构前一致
- [x] 未设置任何环境变量时，工具不注册（与现有行为一致）

---

### Plan 2: Tavily 后端实现

> **优先级: 中** | 涉及文件: `web_search/tavily.rs`, `web_search/mod.rs`

#### 目标
新增 Tavily 搜索后端，作为 Google CSE 之后的备选方案。

#### 详细步骤

1. **创建 `tavily.rs`**
   - 实现 `TavilyBackend` struct，持有 `api_key: String` 和 `client: Client`
   - 实现 `SearchBackend` trait
   - POST 请求体构建：`api_key`, `query`, `max_results`, `search_depth: "basic"`
   - 响应解析：`results[].{title, url, content}` → `Vec<SearchResult>`
     - 注意：Tavily 的 `content` 字段映射到 `SearchResult::snippet`

2. **更新 `from_env()` 感知链**
   ```rust
   // Google > Tavily > Err
   if google_configured { GoogleBackend }
   else if tavily_configured { TavilyBackend }
   else { return Err(...) }
   ```

3. **错误处理**
   - Tavily API 返回 `{ "error": "..." }` 时，提取错误信息返回 `ToolOutput { is_error: true }`

#### 验收标准
- [x] 设置 `TAVILY_API_KEY` 后工具正常工作
- [x] Google 优先级高于 Tavily（同时设置两者时使用 Google）

---

### Plan 3: DuckDuckGo 兜底后端

> **优先级: 低** | 涉及文件: `web_search/duckduckgo.rs`, `web_search/mod.rs`

#### 目标
实现无需 API Key 的 DuckDuckGo 后端作为兜底方案，确保搜索工具在无任何配置时仍可用。

#### 详细步骤

1. **创建 `duckduckgo.rs`**
   - 实现 `DuckDuckGoBackend` struct，仅持有 `client: Client`
   - POST `https://html.duckduckgo.com/html/` with `q=<query>`
   - 使用 `scraper` crate 解析 HTML 响应：
     ```rust
     let title_selector = Selector::parse(".result__a").unwrap();
     let snippet_selector = Selector::parse(".result__snippet").unwrap();
     ```
   - 从 `href` 属性提取真实 URL（DuckDuckGo 会包装为重定向链接，需解码 `uddg=` 参数）

2. **更新 `from_env()` — 完成感知链闭环**
   ```rust
   // Google > Tavily > DuckDuckGo (永远成功)
   if google_configured { GoogleBackend }
   else if tavily_configured { TavilyBackend }
   else { DuckDuckGoBackend }
   ```

3. **关键变更：`from_env()` 签名改为返回 `Self`**
   - 因为 DuckDuckGo 不需要配置，`from_env()` 永远成功
   - 同步更新 `builtin/mod.rs` 中的注册逻辑，移除 `match` 错误处理：
     ```rust
     // Before: match web_search::WebSearchTool::from_env() { Ok(t) => ..., Err(e) => ... }
     // After:  registry.register(Box::new(web_search::WebSearchTool::from_env()));
     ```

4. **健壮性处理**
   - HTML 解析失败时返回空结果而非 error
   - 设置合理的请求超时（10s）
   - 添加 User-Agent header 避免被拒绝

#### 验收标准
- [x] 不设置任何环境变量时，`web_search` 工具自动使用 DuckDuckGo 并返回结果
- [x] `builtin/mod.rs` 中注册逻辑简化，无 `match` 分支

---

### Plan 4: 系统集成与优化

> **优先级: 低** | 涉及文件: 多处

#### 目标
完成集成打磨，动态调整工具 description，端到端验证。

#### 详细步骤

1. **动态 description**
   - `Tool::definition()` 中根据当前后端名称生成描述：
     ```
     Google:     "Search the web via Google. Focused on GitHub, HuggingFace, docs.rs."
     Tavily:     "Search the web via Tavily. Results are optimized for LLM consumption."
     DuckDuckGo: "Search the web via DuckDuckGo. No API key required."
     ```

2. **日志增强**
   - 启动时打印 `INFO` 日志：`"Web search backend: {backend_name}"`
   - 搜索时打印 `DEBUG` 日志：查询词、结果数量、耗时

3. **端到端测试**
   - 测试矩阵：

   | 环境变量配置 | 期望后端 |
   | :--- | :--- |
   | `GOOGLE_SEARCH_API_KEY` + `GOOGLE_SEARCH_CX` | Google |
   | `TAVILY_API_KEY` only | Tavily |
   | 无任何 Key | DuckDuckGo |
   | Google + Tavily 同时设置 | Google (优先) |

4. **文档更新**
   - 在项目 README 或配置说明中记录环境变量用法

#### 验收标准
- [x] 所有后端在 CLI 中实际可用
- [x] 日志清晰反映当前使用的后端
- [x] 测试矩阵全部通过
