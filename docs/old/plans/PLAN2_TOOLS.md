# Plan 2: 工具系统 + 内置工具

## 目标

Agent 能调用工具，完成 搜索 → 抓取 → 文件写入 等完整链路。

## 前置

Plan 1 完成。

## 范围

| # | 文件 | 内容 |
|---|------|------|
| 1 | `src/tool/mod.rs` | Tool trait、ToolOutput、ToolRegistry |
| 2 | `src/tool/builtin/mod.rs` | `register_builtin_tools()` 便捷函数 |
| 3 | `src/tool/builtin/bash.rs` | 系统命令执行工具 |
| 4 | `src/tool/builtin/file_ops.rs` | read_file、write_file 工具 |
| 5 | `src/tool/builtin/web_search.rs` | Web 搜索工具 |
| 6 | `src/tool/builtin/web_fetch.rs` | 网页抓取工具（HTML → 文本提取） |
| 7 | `src/prompt.rs` 补充 | `{tool_descriptions}` 自动生成逻辑 |
| 8 | `src/agent.rs` 补充 | 工具执行路径的端到端验证 |

## 详细设计

### 1. Tool trait + ToolOutput

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    /// 工具定义，序列化后发送给 LLM
    fn definition(&self) -> ToolDefinition;

    /// 执行工具
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput>;
}

pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}
```

### 2. ToolRegistry

```rust
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, tool: Box<dyn Tool>);
    pub fn register_many(&mut self, tools: Vec<Box<dyn Tool>>);
    pub fn unregister(&mut self, name: &str) -> bool;
    pub fn definitions(&self) -> Vec<ToolDefinition>;
    pub async fn execute(&self, name: &str, input: serde_json::Value) -> Result<ToolOutput>;
    pub fn merge(&mut self, other: ToolRegistry);
}
```

查找逻辑：`execute()` 遍历 tools 匹配 `definition().name == name`。如果工具集较大（>50），可改用 `HashMap<String, Box<dyn Tool>>`，但初期 Vec 足够。

### 3. 内置工具

#### 3.1 bash

```rust
pub struct BashTool;
```

- **name**: `bash`
- **description**: Execute a shell command and return its stdout/stderr
- **input_schema**:
  ```json
  {
    "type": "object",
    "properties": {
      "command": { "type": "string", "description": "The shell command to execute" },
      "timeout_ms": { "type": "integer", "description": "Timeout in milliseconds (default 30000)" }
    },
    "required": ["command"]
  }
  ```
- **执行逻辑**: `tokio::process::Command::new("sh").args(["-c", &command])`，捕获 stdout + stderr，超时杀进程
- **输出格式**: `"exit_code: {code}\nstdout:\n{stdout}\nstderr:\n{stderr}"`
- **安全约束**: 输出截断上限 100KB

#### 3.2 file_ops（read_file + write_file）

拆为两个独立 Tool 实现：

**ReadFileTool**:
- **name**: `read_file`
- **input_schema**:
  ```json
  {
    "type": "object",
    "properties": {
      "path": { "type": "string", "description": "Absolute path to the file" },
      "offset": { "type": "integer", "description": "Start line (1-based, optional)" },
      "limit": { "type": "integer", "description": "Number of lines to read (optional)" }
    },
    "required": ["path"]
  }
  ```
- **输出格式**: 带行号的文件内容，与 `cat -n` 类似
- **约束**: 单次最大读取 2000 行，超长行截断至 2000 字符

**WriteFileTool**:
- **name**: `write_file`
- **input_schema**:
  ```json
  {
    "type": "object",
    "properties": {
      "path": { "type": "string", "description": "Absolute path to the file" },
      "content": { "type": "string", "description": "Content to write" }
    },
    "required": ["path", "content"]
  }
  ```
- **输出**: `"Written {n} bytes to {path}"`

#### 3.3 web_search

```rust
pub struct WebSearchTool {
    api_key: String,      // 搜索 API key（如 Brave Search API）
    endpoint: String,
}
```

- **name**: `web_search`
- **input_schema**:
  ```json
  {
    "type": "object",
    "properties": {
      "query": { "type": "string", "description": "Search query" },
      "count": { "type": "integer", "description": "Number of results (default 5, max 20)" }
    },
    "required": ["query"]
  }
  ```
- **执行逻辑**: 调用搜索 API（Brave Search / SearXNG / 其他），返回结构化结果
- **输出格式**:
  ```text
  Search results for "query":

  1. [Title](url)
     Snippet text...

  2. [Title](url)
     Snippet text...
  ```
- **搜索后端可配置**: 通过构造函数传入 endpoint + api_key，不硬编码

#### 3.4 web_fetch

```rust
pub struct WebFetchTool {
    http: reqwest::Client,
}
```

- **name**: `web_fetch`
- **input_schema**:
  ```json
  {
    "type": "object",
    "properties": {
      "url": { "type": "string", "description": "URL to fetch" },
      "selector": { "type": "string", "description": "Optional CSS selector to extract specific content" }
    },
    "required": ["url"]
  }
  ```
- **执行逻辑**: HTTP GET → HTML → 文本提取（使用 `scraper` crate 或简单的标签剥离）
- **输出**: 提取后的纯文本内容，截断至 50KB
- **约束**: 遵循 robots.txt，30 秒超时，跟随重定向（最多 5 次）

### 4. register_builtin_tools()

```rust
/// 注册所有内置工具到 registry
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    #[cfg(feature = "tool-bash")]
    registry.register(Box::new(BashTool));

    #[cfg(feature = "tool-file-ops")]
    {
        registry.register(Box::new(ReadFileTool));
        registry.register(Box::new(WriteFileTool));
    }

    #[cfg(feature = "tool-web-search")]
    if let Ok(tool) = WebSearchTool::from_env() {
        registry.register(Box::new(tool));
    }

    #[cfg(feature = "tool-web-fetch")]
    registry.register(Box::new(WebFetchTool::new()));
}
```

### 5. prompt.rs 补充：工具说明自动生成

`SystemPromptBuilder::with_tools()` 遍历 `ToolRegistry::definitions()`，为每个工具生成：

```text
## {name}

{description}

Input schema:
```json
{input_schema}
```​
```

替换模板中的 `{tool_descriptions}` 占位符。

### 6. agent.rs 工具执行路径

run_turn 中的工具执行分支（Plan 1 中预留）正式启用：

1. 从 assistant 消息中提取所有 `ContentBlock::ToolUse`
2. 对每个 ToolUse：
   - 发送 `AgentEvent::ToolStart`
   - 调用 `self.tools.execute(name, input)`
   - 发送 `AgentEvent::ToolEnd`
   - 构造 `ContentBlock::ToolResult`
3. 将 tool results 组装为 User 消息追加到 messages
4. 回到 LLM stream 调用，继续循环

## 验证方式

1. 单元测试：每个内置工具的 execute 方法
2. 集成测试：
   - mock LlmClient 返回包含 ToolUse 的响应
   - 验证 ToolRegistry 正确分发到对应工具
   - 验证 ToolResult 正确回传给下一轮 LLM 调用
3. 动态注册测试：运行时 register/unregister 后 definitions() 正确反映变化

## 交付物

Agent 能在对话中自动调用工具（bash、文件读写、搜索、抓取）并利用结果继续推理。
