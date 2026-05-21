// OrchestrateTaskTool 在激活子 Agent 之前调用本 hook 让外部宿主改写传递给
// 子 Agent 的 prompt。典型用途：根据 skill_slug 前置注入运行时上下文
// （如 zero 给 alarm skill 注入 [Now] / [Delivery]）。
//
// 设计取舍参见 zero 仓 `docs/2026-05-22-nova-dynamic-injection/` Plan 2。
// 关键取舍：hook 拿 `ToolContext.session_id` 反查宿主自己的上下文表
// （nova 不持有 channel/from_user 等业务概念）；trait 返回 Err 时 nova
// 走 fallback（使用 original_prompt），不阻塞主链路。

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

/// `OrchestrateTaskTool` 激活子 Agent 前调用的 prompt 改写钩子。
#[async_trait]
pub trait OrchestrateTaskPromptHook: Send + Sync {
    /// 改写子 Agent 的 first user prompt。
    /// - `skill_slug`：被委派的 skill 标识（shorthand 模式来自 `OrchestrateTask`
    ///   的 `skill` 字段；plan 模式来自每个 `AgentRequest.skill`）。可能为空串
    /// - `original_prompt`：主 Agent 调 OrchestrateTask 时传的 prompt
    /// - `session_id`：当前调 OrchestrateTask 的 **主 Agent nova session_id**
    ///   （来自 `ToolContext.session_id`，hook 可据此反查宿主侧上下文）
    ///
    /// 返回值替换 `original_prompt` 喂给子 Agent。
    async fn transform_prompt(&self, skill_slug: &str, original_prompt: &str, session_id: &str) -> Result<String>;
}

/// 可重入注册的 hook slot。多个 `OrchestrateTaskTool::clone` 共享同一 slot
/// （Arc 内部），外部通过 `register` 注入；构造时为 None；hook 单一全局。
#[derive(Default, Clone)]
pub struct OrchestrateTaskHookSlot {
    inner: Arc<RwLock<Option<Arc<dyn OrchestrateTaskPromptHook>>>>,
}

impl OrchestrateTaskHookSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注入 hook（覆盖旧值）。
    pub async fn set(&self, hook: Arc<dyn OrchestrateTaskPromptHook>) {
        *self.inner.write().await = Some(hook);
    }

    /// 取当前 hook 克隆，调用方异步使用不持锁。None 表示未注入。
    pub async fn get(&self) -> Option<Arc<dyn OrchestrateTaskPromptHook>> {
        self.inner.read().await.clone()
    }
}

impl std::fmt::Debug for OrchestrateTaskHookSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OrchestrateTaskHookSlot {{ ... }}")
    }
}
