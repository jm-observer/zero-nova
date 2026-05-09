# Plan 4: 请求参数与回归验证

## 前置依赖

Plan 2: 规则块分层精简。

Plan 3: 历史 Tool 输出压缩。

---

## 本次目标

治理 OpenAI-compatible 请求参数冗余，并通过请求快照和集成测试验证 prompt compaction 的整体效果。

可验证目标：

1. `max_completion_tokens` 和 `max_tokens` 不再无条件双写。
2. 不同 provider 可配置是否发送 legacy `max_tokens`。
3. 请求快照能反映 system prompt、tools、history 的精简结果。
4. 压缩前后保持工具调用功能可用。
5. 修复流程全部通过。

---

## 涉及文件

| 文件 | 变更类型 | 说明 |
|---|---|---|
| `crates/nova-agent/src/provider/openai_compat/conv.rs` | 修改 | provider-aware max token 字段映射 |
| `crates/nova-agent/src/config.rs` | 修改 | 增加兼容字段配置 |
| `crates/nova-agent/tests/integration/mock_client.rs` | 修改 | 捕获并断言请求字段 |
| `crates/nova-agent/tests/integration/*` | 新增/修改 | 添加端到端请求快照测试 |
| `docs/2026-05-09-prompt-compaction/prompt-compaction.md` | 修改 | 实施完成后更新 Plan 状态 |

---

## 详细设计

### 1. max token 字段策略

当前 `build_request` 同时设置：

1. `max_completion_tokens: Some(config.max_tokens)`
2. `request.max_tokens = Some(config.max_tokens)`

代码注释说明这是为了兼容部分 OpenAI-compatible 服务端。问题是不同 provider 对双写容忍度不同。

建议新增配置：

```toml
[model]
max_tokens_field = "completion"
```

可选值：

| 值 | 行为 | 适用场景 |
|---|---|---|
| `completion` | 只发送 `max_completion_tokens` | 新 OpenAI-compatible API |
| `legacy` | 只发送 `max_tokens` | 老兼容服务 |
| `both` | 两者都发送 | 保持现有行为，用于过渡 |

为了降低破坏性，首版默认可以是 `both`，但配置和测试必须支持 `completion` 与 `legacy`。

### 2. 请求快照测试

新增测试构造包含以下内容的 turn：

1. developer agent prompt。
2. project instructions。
3. 一个未激活 skill。
4. 一个超长 tool result。
5. 至少一个工具 schema。

断言：

1. 未激活 skill 只出现 catalog，不出现完整正文。
2. tool result 出现 `[Tool output compacted]`。
3. 原始超长片段不完整回灌。
4. `max_tokens_field=completion` 时请求不含 legacy `max_tokens`。
5. `max_tokens_field=legacy` 时请求不含 `max_completion_tokens`。

### 3. 回归测试维度

需要覆盖三类链路：

1. `SystemPromptBuilder::from_config`：确认 section 选择和顺序。
2. `AgentRuntime`：确认 tool result 压缩发生在 history 写入前或发送前。
3. `openai_compat::build_request`：确认最终 JSON 请求字段符合配置。

### 4. 观测指标

建议测试中计算压缩比，不要求固定绝对数字：

```text
before_total_chars > after_total_chars
large_tool_result_after <= configured_max_chars + metadata_overhead
```

由于 prompt 内容会迭代，固定快照全文容易脆弱。建议快照只断言关键字段和关键 marker。

### 5. 向后兼容

默认策略建议：

1. `prompt_compaction.enabled = true`：如果担心风险，可先默认 false 并在 developer agent 打开。
2. `tool_result_compaction.enabled = true`：默认开启，阈值保守。
3. `max_tokens_field = both`：默认保持旧行为，后续迁移到 `completion`。

如果项目更重视立即修复请求冗余，也可以将默认改为 `completion`，但需要确认当前服务端兼容性。

---

## 测试案例

| 类型 | 用例 | 期望 |
|---|---|---|
| 参数映射 | `max_tokens_field=completion` | 只发送 `max_completion_tokens` |
| 参数映射 | `max_tokens_field=legacy` | 只发送 `max_tokens` |
| 参数映射 | `max_tokens_field=both` | 保持当前双写行为 |
| 端到端 | 超长 tool output + compact enabled | 最终请求中 tool 内容被压缩 |
| 端到端 | compact disabled | 最终请求保留原始 tool 内容 |
| 回归 | 未激活 skill | 不注入完整 skill 正文 |
| 回归 | prompt preview | 与实际请求 section 一致 |

---

## 验收标准

1. 请求参数策略可配置，并有测试覆盖三种模式。
2. 端到端测试证明请求上下文体量下降。
3. 工具调用协议仍保持合法。
4. 总览文档中 Plan 状态可按实施结果更新。
5. `cargo clippy --workspace -- -D warnings`、`cargo fmt --all --check`、`cargo test --workspace` 全部通过。
