export const AGENT_CONSOLE_TEMPLATE = `
<div class="agent-console-inner">
  <header class="agent-console-header">
    <div class="agent-console-title-group">
        <span class="agent-console-title" data-i18n="console.title">Agent 工作台</span>
        <span class="agent-console-status-dot"></span>
    </div>
    <div class="agent-console-actions">
        <button class="agent-console-refresh" data-i18n-title="common.refresh" title="刷新">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <polyline points="23 4 23 10 17 10" />
                <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
            </svg>
        </button>
        <button class="agent-console-close" data-i18n-title="common.close" title="关闭">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
        </button>
    </div>
  </header>
  
  <nav class="agent-console-tabs">
    <button class="agent-console-tab active" data-tab="overview" data-i18n="console.tab_overview">概览</button>
    <button class="agent-console-tab" data-tab="runs" data-i18n="console.tab_runs">运行</button>
    <button class="agent-console-tab" data-tab="model" data-i18n="console.tab_model">模型</button>
    <button class="agent-console-tab" data-tab="tools" data-i18n="console.tab_tools">工具</button>
    <button class="agent-console-tab" data-tab="skills" data-i18n="console.tab_skills">技能</button>
    <button class="agent-console-tab" data-tab="prompt-memory" data-i18n="console.tab_prompt_memory">Prompt</button>
    <button class="agent-console-tab" data-tab="permissions" data-i18n="console.tab_permissions">权限</button>
    <button class="agent-console-tab" data-tab="diagnostics" data-i18n="console.tab_diagnostics">诊断</button>
  </nav>
  
  <section class="agent-console-body">
    <!-- 概览页 -->
    <div class="console-tab-content active" data-tab="overview" id="console-tab-overview">
        <div class="console-section">
            <div class="console-section-title" data-i18n="console.runtime_status">运行状态</div>
            <div id="console-runtime-card" class="console-status-card">
                <div class="skeleton-text"></div>
            </div>
        </div>
        <div class="console-summary-grid">
            <div class="summary-card clickable" data-goto-tab="model">
                <div class="summary-card-label" data-i18n="console.current_model">当前模型</div>
                <div class="summary-card-value" id="summary-model-name">—</div>
            </div>
            <div class="summary-card clickable" data-goto-tab="model">
                <div class="summary-card-label" data-i18n="console.tokens_total">累计 Token</div>
                <div class="summary-card-value" id="summary-tokens-total">0</div>
            </div>
            <div class="summary-card clickable" data-goto-tab="tools">
                <div class="summary-card-label" data-i18n="console.tools_count">可用工具</div>
                <div class="summary-card-value" id="summary-tools-count">0</div>
            </div>
            <div class="summary-card clickable" data-goto-tab="skills">
                <div class="summary-card-label" data-i18n="console.skills_count">已挂载技能</div>
                <div class="summary-card-value" id="summary-skills-count">0</div>
            </div>
        </div>
        <div class="console-section">
            <div class="console-section-title" data-i18n="console.recent_run">最近运行</div>
            <div id="console-recent-run-list" class="console-run-list">
                <div class="empty-hint" data-i18n="console.no_runs">暂无运行记录</div>
            </div>
        </div>
    </div>

    <div class="console-tab-content" data-tab="runs" id="console-tab-runs">
        <div class="console-section">
            <div class="console-section-title" data-i18n="console.current_run">当前运行</div>
            <div id="console-current-run-card" class="console-status-card">
                <div class="empty-hint" data-i18n="console.no_runs">暂无运行记录</div>
            </div>
        </div>
        <div class="console-section">
            <div class="console-section-title" data-i18n="console.run_history">运行历史</div>
            <div id="console-run-filters" class="console-filter-row"></div>
            <div class="console-split-panel">
                <div id="console-runs-list" class="console-run-list"></div>
                <div id="console-run-detail" class="console-run-detail">
                    <div class="empty-hint" data-i18n="console.select_run">请选择一条运行记录</div>
                </div>
            </div>
        </div>
    </div>
    
    <!-- 模型页 -->
    <div class="console-tab-content" data-tab="model" id="console-tab-model">
        <div class="console-section">
            <div class="console-section-title" data-i18n="console.model_override">模型覆盖 (会话级)</div>
            <div id="console-model-settings" class="console-model-settings">
                <div class="empty-hint" data-i18n="common.loading">加载中...</div>
            </div>
        </div>
        <div class="console-section">
            <div class="console-section-title" data-i18n="console.token_usage_detail">Token 消耗详情</div>
            <div id="console-token-usage" class="token-usage-panel">
                <div class="empty-hint" data-i18n="console.no_data">暂无消耗记录</div>
            </div>
        </div>
    </div>
    
    <!-- 工具页 -->
    <div class="console-tab-content" data-tab="tools" id="console-tab-tools">
        <div id="console-tools-list" class="console-tools-list">
            <div class="empty-hint" data-i18n="common.loading">加载中...</div>
        </div>
    </div>
    
    <!-- 技能页 -->
    <div class="console-tab-content" data-tab="skills" id="console-tab-skills">
        <div id="console-skills-list" class="console-skills-list">
            <div class="empty-hint" data-i18n="common.loading">加载中...</div>
        </div>
    </div>
    
    <!-- Prompt/Memory 页 -->
    <div class="console-tab-content" data-tab="prompt-memory" id="console-tab-prompt-memory">
        <div class="console-section">
            <div class="console-section-title" data-i18n="console.prompt_preview">Prompt 预览</div>
            <div id="console-prompt-preview" class="prompt-preview-container">
                <div class="empty-hint" data-i18n="common.loading">加载中...</div>
            </div>
        </div>
        <div class="console-section">
            <div class="console-section-title" data-i18n="console.memory_hits">记忆命中 (本轮)</div>
            <div id="console-memory-hits" class="memory-hits-list">
                <div class="empty-hint" data-i18n="console.no_data">暂无命中记录</div>
            </div>
        </div>
    </div>

    <div class="console-tab-content" data-tab="permissions" id="console-tab-permissions">
        <div class="console-section">
            <div class="console-section-title" data-i18n="console.pending_permissions">待确认请求</div>
            <div id="console-permission-pending" class="console-card-list">
                <div class="empty-hint" data-i18n="console.no_pending_permissions">当前没有待确认请求</div>
            </div>
        </div>
        <div class="console-section">
            <div class="console-section-title" data-i18n="console.audit_logs">审计日志</div>
            <div id="console-audit-list" class="console-card-list">
                <div class="empty-hint" data-i18n="console.no_audit_logs">暂无审计记录</div>
            </div>
        </div>
    </div>

    <div class="console-tab-content" data-tab="diagnostics" id="console-tab-diagnostics">
        <div class="console-section">
            <div class="console-section-title" data-i18n="console.workspace_restore">工作区恢复</div>
            <div id="console-restore-card" class="console-status-card">
                <div class="empty-hint" data-i18n="console.no_restore">暂无可恢复上下文</div>
            </div>
        </div>
        <div class="console-section">
            <div class="console-section-title" data-i18n="console.current_diagnostics">当前诊断</div>
            <div id="console-diagnostics-list" class="console-card-list">
                <div class="empty-hint" data-i18n="console.no_diagnostics">当前没有诊断问题</div>
            </div>
        </div>
    </div>
  </section>
  
  <footer class="agent-console-footer">
    <span class="update-time" id="console-update-time"></span>
    <span class="data-source" data-i18n="console.live_data">实时运行态数据</span>
  </footer>
</div>
`;
