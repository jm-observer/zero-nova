//! 静态模板文本、常量和正则表达式。
//!
//! 此模块包含所有纯文本模板、常量和正则表达式定义，不含业务逻辑。
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
//  常量定义
// ---------------------------------------------------------------------------

/// 项目上下文文件名（按优先级排列）
pub const PROJECT_CONTEXT_FILES: &[&str] = &["PROJECT.md", "NOVA.md"];

/// 项目上下文最大字符数（约 4000 token）
pub const MAX_PROJECT_CONTEXT_CHARS: usize = 16000;

/// 开发项目提示词 section 标题
pub const DEVELOPER_PROMPT_SECTION_HEADING: &str = "Developer Project Instructions";

/// 内置行为约束文本。
///
/// 与 bootstrap.rs 中原始硬编码字符串语义一致，Phase 1 不做内容变更。
pub const BEHAVIOR_GUARDS: &str = r#"
## CRITICAL: Action Consistency
- You are a physical entity with real-world capabilities.
- If you state that you are going to perform an action (e.g., "running a command", "writing a file", "searching the web"), you MUST generate the corresponding tool_use block in the SAME response.
- NEVER claim you are doing something "in the background" or "internally" without an actual tool call.
- Textual confirmation of an action is only valid AFTER the tool has been invoked.
"#;

// ---------------------------------------------------------------------------
//  模板变量正则匹配
// ---------------------------------------------------------------------------

/// 模板变量正则匹配
pub static TEMPLATE_VAR_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\{\{([a-zA-Z_][a-zA-Z0-9_]*)\}\}").unwrap());

/// 预定义模板变量名称。
pub mod template_vars {
    /// 当前 workflow 阶段
    pub const WORKFLOW_STAGE: &str = "workflow_stage";
    /// 当前挂起交互
    pub const PENDING_INTERACTION: &str = "pending_interaction";
    /// 当前话题
    pub const TOPIC: &str = "topic";
    /// 约束条件
    pub const CONSTRAINTS: &str = "constraints";
    /// 候选方案列表
    pub const CANDIDATES: &str = "candidates";
    /// 已选方案
    pub const SELECTED_CANDIDATE: &str = "selected_candidate";
    /// 当前活跃 agent
    pub const ACTIVE_AGENT: &str = "active_agent";
}

// ---------------------------------------------------------------------------
//  模板渲染（纯函数，无依赖）
// ---------------------------------------------------------------------------

/// 简单的 `{{key}}` 模板变量替换。
pub struct TemplateContext;

impl TemplateContext {
    /// 替换模板中的 `{{key}}` 占位符。
    ///
    /// - 已匹配的变量替换为对应值
    /// - 未匹配的占位符替换为空字符串（清理模式）
    pub fn render(template: &str, vars: &HashMap<String, String>) -> String {
        TEMPLATE_VAR_RE
            .replace_all(template, |caps: &regex::Captures| {
                let key = &caps[1];
                vars.get(key).cloned().unwrap_or_default()
            })
            .to_string()
    }

    /// 替换模板中的 `{{key}}` 占位符（保留模式）。
    ///
    /// 已匹配的变量替换为对应值，未匹配的保持原样。
    pub fn render_partial(template: &str, vars: &HashMap<String, String>) -> String {
        TEMPLATE_VAR_RE
            .replace_all(template, |caps: &regex::Captures| {
                let key = &caps[1];
                match vars.get(key) {
                    Some(value) => value.clone(),
                    None => caps[0].to_string(),
                }
            })
            .to_string()
    }

    /// 提取模板中所有占位符的名称。
    pub fn extract_vars(template: &str) -> Vec<String> {
        TEMPLATE_VAR_RE
            .captures_iter(template)
            .map(|cap| cap[1].to_string())
            .collect()
    }
}
