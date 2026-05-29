// SkillSystemPromptHook：AgentTool 在用 skill `instructions`（SKILL.md 正文）
// 作为子 Agent 完整 system prompt **之前**调用本 hook，让外部宿主有机会
// 把运行时上下文（参数、配置）拼到 system prompt 前。
//
// 与 [`OrchestrateTaskPromptHook`] 的区别：
//   - OrchestrateTaskPromptHook 改写的是子 Agent 的 **首条 user 消息**
//   - SkillSystemPromptHook  改写的是子 Agent 的 **system prompt**
//
// 典型用法：zero 把 skill 的 preload.toml `[[parameters]]` 解析为
// `## 运行时参数\n- key: value\n...` 块，拼到 SKILL.md 前作为完整 system。
//
// 失败语义：trait 返回 Err 时 nova 走 fallback（沿用未改写的 SKILL.md），
// 不阻塞主链路（与现有 OrchestrateTaskPromptHook 一致）。

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

/// `AgentTool` 在把 skill instructions 作为完整 system prompt 之前调用的钩子。
#[async_trait]
pub trait SkillSystemPromptHook: Send + Sync {
    /// 改写 skill 子 Agent 的 system prompt。
    ///
    /// - `skill_slug`：被委派的 skill 标识。
    /// - `base_system_prompt`：原始 SKILL.md 正文（`SkillPackage::instructions`）。
    /// - `session_id`：当前调用 OrchestrateTask 的 **主 Agent nova session_id**，
    ///   hook 可据此反查宿主侧的 session 状态。
    ///
    /// 返回值替换原 instructions 喂给子 Agent。
    async fn transform_system_prompt(
        &self,
        skill_slug: &str,
        base_system_prompt: &str,
        session_id: &str,
    ) -> Result<String>;
}

/// 可重入注册的 hook slot。多个 `AgentTool::clone` 共享同一 slot
/// （Arc 内部），外部通过 `set` 注入；构造时为 None；hook 单一全局。
#[derive(Default, Clone)]
pub struct SkillSystemPromptHookSlot {
    inner: Arc<RwLock<Option<Arc<dyn SkillSystemPromptHook>>>>,
}

impl SkillSystemPromptHookSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注入 hook（覆盖旧值）。
    pub async fn set(&self, hook: Arc<dyn SkillSystemPromptHook>) {
        *self.inner.write().await = Some(hook);
    }

    /// 取当前 hook 克隆，调用方异步使用不持锁。None 表示未注入。
    pub async fn get(&self) -> Option<Arc<dyn SkillSystemPromptHook>> {
        self.inner.read().await.clone()
    }
}

impl std::fmt::Debug for SkillSystemPromptHookSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SkillSystemPromptHookSlot {{ ... }}")
    }
}
