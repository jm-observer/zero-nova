// 外部宿主（如 zero）通过本 trait 给 nova 注入动态 system prompt。
//
// 设计参见 zero 仓 `docs/2026-05-22-nova-dynamic-injection/` Plan 1。
// 关键取舍：pull 模式（nova 在 create_session 等时机回调 provider）；
// trait 缺失或返回 Err 时 nova 走 fallback（使用 AgentDescriptor.system_prompt_template）。

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 外部宿主提供 agent system prompt 的接口。
///
/// nova 在 `AgentApplicationImpl::create_session` 等需要 prompt 字符串的时机
/// 回调本 trait；实现方负责一次性返回完整 prompt 内容（含静态 base + 任何
/// 动态段如知识目录）。返回 `Err` 时 nova 走 fallback：使用 `AgentDescriptor.
/// system_prompt_template` 静态字段（即旧路径），调用主链路不阻塞。
#[async_trait]
pub trait AgentPromptProvider: Send + Sync {
    /// 返回 `agent_id` 的当前完整 system prompt。
    async fn current_system_prompt(&self, agent_id: &str) -> Result<String>;
}

/// 按 agent_id 注册外部 prompt provider 的运行时表。
///
/// 内部用 `Arc<RwLock<HashMap<...>>>`：调用方持有 `&PromptProviderRegistry`
/// 即可注册（无需 `&mut self`），便于挂在 immutable 持有的 service 上。
#[derive(Default, Clone)]
pub struct PromptProviderRegistry {
    inner: Arc<RwLock<HashMap<String, Arc<dyn AgentPromptProvider>>>>,
}

impl PromptProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册（或覆盖）某 agent 的 provider。重复 register 静默覆盖。
    pub async fn register(&self, agent_id: &str, provider: Arc<dyn AgentPromptProvider>) {
        self.inner.write().await.insert(agent_id.to_string(), provider);
    }

    /// 取某 agent 的 provider 克隆（None 表示该 agent 未注册 provider）。
    pub async fn get(&self, agent_id: &str) -> Option<Arc<dyn AgentPromptProvider>> {
        self.inner.read().await.get(agent_id).cloned()
    }
}

impl std::fmt::Debug for PromptProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PromptProviderRegistry {{ ... }}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StaticProvider(String);

    #[async_trait]
    impl AgentPromptProvider for StaticProvider {
        async fn current_system_prompt(&self, _agent_id: &str) -> Result<String> {
            Ok(self.0.clone())
        }
    }

    struct FailingProvider;

    #[async_trait]
    impl AgentPromptProvider for FailingProvider {
        async fn current_system_prompt(&self, _agent_id: &str) -> Result<String> {
            anyhow::bail!("intentional failure for test")
        }
    }

    #[tokio::test]
    async fn register_and_get_returns_provider() {
        let reg = PromptProviderRegistry::new();
        let p: Arc<dyn AgentPromptProvider> = Arc::new(StaticProvider("hello".into()));
        reg.register("zero", p).await;
        let got = reg.get("zero").await.expect("should have provider");
        let prompt = got.current_system_prompt("zero").await.unwrap();
        assert_eq!(prompt, "hello");
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let reg = PromptProviderRegistry::new();
        assert!(reg.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn register_twice_overrides() {
        let reg = PromptProviderRegistry::new();
        reg.register("zero", Arc::new(StaticProvider("first".into()))).await;
        reg.register("zero", Arc::new(StaticProvider("second".into()))).await;
        let prompt = reg
            .get("zero")
            .await
            .unwrap()
            .current_system_prompt("zero")
            .await
            .unwrap();
        assert_eq!(prompt, "second");
    }

    #[tokio::test]
    async fn provider_err_propagates() {
        let reg = PromptProviderRegistry::new();
        reg.register("zero", Arc::new(FailingProvider)).await;
        let err = reg
            .get("zero")
            .await
            .unwrap()
            .current_system_prompt("zero")
            .await
            .expect_err("should err");
        let msg = format!("{err:#}");
        assert!(msg.contains("intentional failure"), "msg = {msg}");
    }
}
