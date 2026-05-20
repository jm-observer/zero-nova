use crate::conversation::session::SessionSummary;
use crate::conversation::SessionService;
use crate::message::Message;
use anyhow::Result;
use std::sync::atomic::Ordering;

/// 父子 Session 树的递归视图。根 Session 的 `parent_tool_use_id` 为 None；
/// 子节点的 `parent_tool_use_id` 指向父 history 中那条 ToolUse 的 id。
#[derive(Debug, Clone)]
pub struct SessionTree {
    pub summary: SessionSummary,
    pub parent_tool_use_id: Option<String>,
    pub history: Vec<Message>,
    pub children: Vec<SessionTree>,
    /// 当 `max_depth` 触底导致本节点子树未完全展开时为 true。
    pub truncated: bool,
}

/// DFS 串行构建 SessionTree。
///
/// 已知串行实现（见设计稿「已收敛的待澄清点」#5）：实现简单、正确性优先。
/// zero 侧调用是错误标记触发、非热路径，可接受。
/// 极端场景（8 层 × 4 子 ≈ 65k Session）慢；未来下游高频调用可改 `futures::try_join_all`。
pub async fn build_session_tree(sessions: &SessionService, id: &str, max_depth: usize) -> Result<SessionTree> {
    let session = sessions
        .get_with_history(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("session {} not found", id))?;
    let history = session.get_history().await;
    let summary = SessionSummary {
        id: session.id.clone(),
        name: session.get_name().await,
        agent_id: session.get_active_agent().await,
        created_at: session.created_at,
        updated_at: session.updated_at.load(Ordering::SeqCst),
        message_count: history.len(),
    };
    let parent_tool_use_id = session.parent_tool_use_id.clone();

    let child_ids = session.get_child_ids().await;
    let (children, truncated) = if max_depth == 0 {
        (Vec::new(), !child_ids.is_empty())
    } else {
        let mut children = Vec::with_capacity(child_ids.len());
        for child_id in child_ids {
            let subtree = Box::pin(build_session_tree(sessions, &child_id, max_depth - 1)).await?;
            children.push(subtree);
        }
        (children, false)
    };

    Ok(SessionTree {
        summary,
        parent_tool_use_id,
        history,
        children,
        truncated,
    })
}
