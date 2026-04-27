import { t } from '../i18n/index';
import { escapeHtml, formatFileSize, formatTime } from '../utils/html';
import type {
    AuditLogView,
    DiagnosticIssueView,
    ModelBindingDetailView,
    PermissionRequestView,
    PromptPreviewView,
    ResourceState,
    RunDetailView,
    RunStepView,
    RunSummaryView,
    SessionArtifactView,
} from '../core/types';

export type RunFilter = 'all' | 'running' | 'waiting' | 'failed' | 'artifacts';

type ResourceHintGetter = <T>(state: ResourceState<T>) => string;

export function renderTokenRow(label: string, value: number | string, className = ''): string {
    return `
            <div class="token-row ${className}">
                <span class="token-label">${escapeHtml(label)}</span>
                <span class="token-value">${escapeHtml(String(value))}</span>
            </div>
        `;
}

export function renderRunFilters(activeRunFilter: RunFilter): string {
    const filters: Array<{ id: RunFilter; label: string }> = [
        { id: 'all', label: t('console.filter_all') },
        { id: 'running', label: t('console.filter_running') },
        { id: 'waiting', label: t('console.filter_waiting') },
        { id: 'failed', label: t('console.filter_failed') },
        { id: 'artifacts', label: t('console.filter_artifacts') },
    ];

    return filters
        .map(filter => `<button class="console-filter-chip ${filter.id === activeRunFilter ? 'active' : ''}" data-filter="${filter.id}">${escapeHtml(filter.label)}</button>`)
        .join('');
}

export function filterRunsByFilter(runs: RunSummaryView[], activeRunFilter: RunFilter): RunSummaryView[] {
    return runs.filter(run => {
        switch (activeRunFilter) {
            case 'running':
                return run.status === 'running';
            case 'waiting':
                return run.status === 'waiting_user';
            case 'failed':
                return run.status === 'failed' || run.status === 'stopped';
            case 'artifacts':
                return (run.artifactCount ?? 0) > 0;
            default:
                return true;
        }
    });
}

export function formatDuration(durationMs?: number, startedAt?: number, finishedAt?: number): string {
    const value = durationMs ?? (startedAt ? (finishedAt ?? Date.now()) - startedAt : 0);
    if (!value || value < 0) return '0s';
    if (value < 1000) return `${value}ms`;
    const seconds = Math.round(value / 1000);
    if (seconds < 60) return `${seconds}s`;
    const minutes = Math.floor(seconds / 60);
    const remainSeconds = seconds % 60;
    return remainSeconds > 0 ? `${minutes}m ${remainSeconds}s` : `${minutes}m`;
}

export function renderCurrentRunCard(run: RunSummaryView): string {
    const actions: string[] = [];
    if (run.status === 'running') {
        actions.push(`<button class="console-action-btn danger" data-run-action="stop">${escapeHtml(t('console.action_stop'))}</button>`);
    }
    if (run.status === 'waiting_user') {
        actions.push(`<button class="console-action-btn" data-run-action="resume_waiting">${escapeHtml(t('console.action_resume'))}</button>`);
    }

    return `
            <div class="console-detail-card">
                <div class="run-item-header">
                    <div>
                        <div class="run-title">${escapeHtml(run.title ?? run.id)}</div>
                        <div class="run-subtitle">${escapeHtml(formatTime(run.startedAt))}</div>
                    </div>
                    <span class="run-status ${escapeHtml(run.status)}">${escapeHtml(run.status)}</span>
                </div>
                <div class="console-detail-grid">
                    <div><strong>${escapeHtml(t('console.label_model'))}</strong><div class="run-meta">${escapeHtml(run.modelSummary ?? '—')}</div></div>
                    <div><strong>${escapeHtml(t('console.label_duration'))}</strong><div class="run-meta">${escapeHtml(formatDuration(run.durationMs, run.startedAt, run.finishedAt))}</div></div>
                    <div><strong>${escapeHtml(t('console.label_tools'))}</strong><div class="run-meta">${escapeHtml(String(run.toolCount ?? 0))}</div></div>
                    <div><strong>${escapeHtml(t('console.label_artifacts'))}</strong><div class="run-meta">${escapeHtml(String(run.artifactCount ?? 0))}</div></div>
                </div>
                ${run.waitingReason ? `<div class="run-meta"><strong>${escapeHtml(t('console.label_waiting_reason'))}</strong> ${escapeHtml(run.waitingReason)}</div>` : ''}
                ${actions.length > 0 ? `<div class="detail-actions">${actions.join('')}</div>` : ''}
            </div>
        `;
}

export function renderRunListItem(run: RunSummaryView, active: boolean): string {
    const tokenTotal = (run.tokenUsage?.inputTokens ?? 0) + (run.tokenUsage?.outputTokens ?? 0);
    return `
            <div class="run-item ${active ? 'active' : ''}" data-run-id="${escapeHtml(run.id)}">
                <div class="run-item-header">
                    <span class="run-title">${escapeHtml(run.title ?? run.id)}</span>
                    <span class="run-status ${escapeHtml(run.status)}">${escapeHtml(run.status)}</span>
                </div>
                <div class="run-meta-row">
                    <span class="run-meta">${escapeHtml(formatTime(run.startedAt))}</span>
                    <span class="run-duration">${escapeHtml(formatDuration(run.durationMs, run.startedAt, run.finishedAt))}</span>
                </div>
                <div class="run-meta-row">
                    <span class="run-meta">${escapeHtml(run.modelSummary ?? '—')}</span>
                    <span class="run-meta">${escapeHtml(`${t('console.label_tokens')}: ${tokenTotal}`)}</span>
                </div>
                ${run.errorSummary ? `<div class="run-meta">${escapeHtml(run.errorSummary)}</div>` : ''}
            </div>
        `;
}

export function renderStepItem(step: RunStepView): string {
    return `
            <div class="step-item ${escapeHtml(step.status)}">
                <strong>${escapeHtml(step.title)}</strong>
                <div class="card-item-meta">${escapeHtml(step.type)} · ${escapeHtml(step.status)}</div>
                ${step.description ? `<div class="card-item-meta">${escapeHtml(step.description)}</div>` : ''}
            </div>
        `;
}

export function renderArtifactItem(artifact: SessionArtifactView): string {
    const name = artifact.filename ?? artifact.title ?? artifact.id;
    return `
            <div class="card-item">
                <div class="card-item-header">
                    <span class="card-item-title">${escapeHtml(name)}</span>
                    <span class="artifact-type-badge ${escapeHtml(artifact.type)}">${escapeHtml(artifact.type)}</span>
                </div>
                <div class="card-item-meta">${escapeHtml(artifact.path ?? '')}</div>
                <div class="card-item-footer">
                    <span class="card-item-meta">${escapeHtml(formatFileSize(artifact.size))}</span>
                    <div class="card-item-actions">
                        <button class="console-inline-btn" data-artifact-action="preview" data-artifact-id="${escapeHtml(artifact.id)}">${escapeHtml(t('console.action_open'))}</button>
                        ${artifact.path ? `<button class="console-inline-btn" data-artifact-action="copy-path" data-artifact-id="${escapeHtml(artifact.id)}">${escapeHtml(t('console.action_copy_path'))}</button>` : ''}
                        ${artifact.path ? `<button class="console-inline-btn" data-artifact-action="reveal" data-artifact-id="${escapeHtml(artifact.id)}">${escapeHtml(t('console.action_reveal'))}</button>` : ''}
                        ${artifact.content ? `<button class="console-inline-btn" data-artifact-action="copy-content" data-artifact-id="${escapeHtml(artifact.id)}">${escapeHtml(t('console.action_copy_content'))}</button>` : ''}
                    </div>
                </div>
            </div>
        `;
}

export function renderRunDetailCard(run: RunSummaryView, detail: RunDetailView | undefined, fallbackArtifacts: SessionArtifactView[]): string {
    const artifacts = detail?.artifacts ?? fallbackArtifacts.filter(item => item.runId === run.id);
    const steps = detail?.steps ?? [];
    const permissions = detail?.permissions ?? [];
    const diagnostics = detail?.diagnostics ?? [];
    const auditLogs = detail?.auditLogs ?? [];

    return `
            <div class="console-detail-card">
                <div class="run-item-header">
                    <div>
                        <div class="run-title">${escapeHtml(run.title ?? run.id)}</div>
                        <div class="run-subtitle">${escapeHtml(run.id)}</div>
                    </div>
                    <span class="run-status ${escapeHtml(run.status)}">${escapeHtml(run.status)}</span>
                </div>
                <div class="console-detail-grid">
                    <div><strong>${escapeHtml(t('console.label_model'))}</strong><div class="run-meta">${escapeHtml(run.modelSummary ?? '—')}</div></div>
                    <div><strong>${escapeHtml(t('console.label_duration'))}</strong><div class="run-meta">${escapeHtml(formatDuration(run.durationMs, run.startedAt, run.finishedAt))}</div></div>
                </div>
            </div>
            <div class="console-detail-card">
                <div class="console-section-title">${escapeHtml(t('console.section_steps'))}</div>
                <div class="step-list">
                    ${steps.length > 0 ? steps.map(step => renderStepItem(step)).join('') : `<div class="empty-hint">${escapeHtml(t('console.no_data'))}</div>`}
                </div>
            </div>
            <div class="console-detail-card">
                <div class="console-section-title">${escapeHtml(t('console.section_artifacts'))}</div>
                <div class="artifact-list">
                    ${artifacts.length > 0 ? artifacts.map(artifact => renderArtifactItem(artifact)).join('') : `<div class="empty-hint">${escapeHtml(t('console.no_data'))}</div>`}
                </div>
            </div>
            <div class="console-detail-card">
                <div class="console-section-title">${escapeHtml(t('console.section_relations'))}</div>
                <div class="card-item-meta">${escapeHtml(`permissions ${permissions.length} · diagnostics ${diagnostics.length} · audit ${auditLogs.length}`)}</div>
                ${permissions.map(item => `<div class="card-item"><div class="card-item-title">${escapeHtml(item.title)}</div><div class="card-item-actions"><button class="console-inline-btn" data-nav-permission="${escapeHtml(item.id)}">${escapeHtml(t('console.action_go_permission'))}</button></div></div>`).join('')}
                ${diagnostics.map(item => `<div class="card-item diagnostic-card ${escapeHtml(item.severity)}"><div class="card-item-header"><span class="card-item-title">${escapeHtml(item.title)}</span><span class="diagnostic-badge ${escapeHtml(item.severity)}">${escapeHtml(item.severity)}</span></div><div class="card-item-meta">${escapeHtml(item.message)}</div>${item.relatedRunId ? `<div class="card-item-actions"><button class="console-inline-btn" data-nav-run="${escapeHtml(item.relatedRunId)}">${escapeHtml(t('console.action_go_run'))}</button></div>` : ''}</div>`).join('')}
                ${auditLogs.map(item => `<div class="card-item"><div class="card-item-header"><span class="card-item-title">${escapeHtml(item.summary)}</span><span class="card-item-meta">${escapeHtml(formatTime(item.createdAt))}</span></div></div>`).join('')}
                ${permissions.length === 0 && diagnostics.length === 0 && auditLogs.length === 0 ? `<div class="empty-hint">${escapeHtml(t('console.no_data'))}</div>` : ''}
            </div>
        `;
}

export function renderPermissionList(
    state: ResourceState<PermissionRequestView[]> | undefined,
    getResourceHint: ResourceHintGetter
): string {
    if (state?.loading && !state.data) {
        return `<div class="empty-hint">${escapeHtml(t('common.loading'))}</div>`;
    }
    if (state?.error) {
        return `<div class="empty-hint">${escapeHtml(getResourceHint(state))}</div>`;
    }

    const pending = (state?.data ?? []).filter(item => item.status === 'pending');
    if (pending.length === 0) {
        return `<div class="empty-hint">${escapeHtml(t('console.no_pending_permissions'))}</div>`;
    }

    return pending.map(item => `
            <div class="card-item permission-card ${escapeHtml(item.status)}">
                <div class="card-item-header">
                    <span class="card-item-title">${escapeHtml(item.title)}</span>
                    <span class="risk-badge ${escapeHtml(item.riskLevel)}">${escapeHtml(item.riskLevel)}</span>
                </div>
                <div class="card-item-meta">${escapeHtml(item.reason ?? item.target ?? '')}</div>
                <div class="card-item-actions">
                    <button class="console-inline-btn" data-permission-decision="approve" data-permission-id="${escapeHtml(item.id)}">${escapeHtml(t('console.permission_approved'))}</button>
                    <button class="console-inline-btn danger" data-permission-decision="deny" data-permission-id="${escapeHtml(item.id)}">${escapeHtml(t('console.permission_denied'))}</button>
                    ${item.runId ? `<button class="console-inline-btn" data-permission-run="${escapeHtml(item.runId)}">${escapeHtml(t('console.action_go_run'))}</button>` : ''}
                </div>
            </div>
        `).join('');
}

export function renderAuditList(
    state: ResourceState<AuditLogView[]> | undefined,
    getResourceHint: ResourceHintGetter
): string {
    if (state?.loading && !state.data) {
        return `<div class="empty-hint">${escapeHtml(t('common.loading'))}</div>`;
    }
    if (state?.error) {
        return `<div class="empty-hint">${escapeHtml(getResourceHint(state))}</div>`;
    }
    const logs = state?.data ?? [];
    if (logs.length === 0) {
        return `<div class="empty-hint">${escapeHtml(t('console.no_audit_logs'))}</div>`;
    }
    return logs.map(item => `
            <div class="card-item">
                <div class="card-item-header">
                    <span class="card-item-title">${escapeHtml(item.summary)}</span>
                    <span class="card-item-meta">${escapeHtml(formatTime(item.createdAt))}</span>
                </div>
                <div class="card-item-meta">${escapeHtml(`${item.actionType} · ${item.result}`)}</div>
            </div>
        `).join('');
}

export function renderDiagnosticList(
    state: ResourceState<DiagnosticIssueView[]> | undefined,
    getResourceHint: ResourceHintGetter
): string {
    if (state?.loading && !state.data) {
        return `<div class="empty-hint">${escapeHtml(t('common.loading'))}</div>`;
    }
    if (state?.error) {
        return `<div class="empty-hint">${escapeHtml(getResourceHint(state))}</div>`;
    }
    const issues = state?.data ?? [];
    if (issues.length === 0) {
        return `<div class="empty-hint">${escapeHtml(t('console.no_diagnostics'))}</div>`;
    }
    return issues.map(issue => `
            <div class="card-item diagnostic-card ${escapeHtml(issue.severity)}">
                <div class="card-item-header">
                    <span class="card-item-title">${escapeHtml(issue.title)}</span>
                    <span class="diagnostic-badge ${escapeHtml(issue.severity)}">${escapeHtml(issue.severity)}</span>
                </div>
                <div class="card-item-meta">${escapeHtml(issue.message)}</div>
                <div class="card-item-actions">
                    ${issue.relatedRunId ? `<button class="console-inline-btn" data-diagnostic-action="run" data-diagnostic-id="${escapeHtml(issue.id)}">${escapeHtml(t('console.action_go_run'))}</button>` : ''}
                    ${issue.relatedPermissionRequestId ? `<button class="console-inline-btn" data-diagnostic-action="permission" data-diagnostic-id="${escapeHtml(issue.id)}">${escapeHtml(t('console.action_go_permission'))}</button>` : ''}
                    ${issue.category === 'mcp' || issue.category === 'memory' ? `<button class="console-inline-btn" data-diagnostic-action="settings" data-diagnostic-id="${escapeHtml(issue.id)}">${escapeHtml(t('console.action_go_settings'))}</button>` : ''}
                    ${issue.retryable ? `<button class="console-inline-btn" data-diagnostic-action="retry" data-diagnostic-id="${escapeHtml(issue.id)}">${escapeHtml(t('console.action_retry'))}</button>` : ''}
                </div>
            </div>
        `).join('');
}

export function renderPromptSegments(promptView: PromptPreviewView): string {
    const sections: string[] = [];

    sections.push(renderPromptSection(t('console.prompt_preview'), promptView.systemPrompt));

    if (promptView.skillFragments.length > 0) {
        sections.push(
            `
                    <div class="prompt-segment">
                        <div class="segment-label">Skills (${promptView.skillFragments.length})</div>
                        ${promptView.skillFragments
                            .map(
                                fragment => `
                                    <div class="skill-fragment">
                                        <strong>${escapeHtml(fragment.title)}</strong>
                                        <div>${renderMultilineText(fragment.content)}</div>
                                    </div>
                                `
                            )
                            .join('')}
                    </div>
                `
        );
    }

    if (promptView.memoryFragments.length > 0) {
        sections.push(
            `
                    <div class="prompt-segment">
                        <div class="segment-label">Memory</div>
                        ${promptView.memoryFragments
                            .map(fragment => `<div class="memory-fragment">${renderMultilineText(fragment.content)}</div>`)
                            .join('')}
                    </div>
                `
        );
    }

    const toolDescriptions = promptView.toolDescriptions ?? promptView.toolFragments ?? [];
    if (toolDescriptions.length > 0) {
        sections.push(
            `
                    <div class="prompt-segment">
                        <div class="segment-label">Tools (${toolDescriptions.length})</div>
                        ${toolDescriptions
                            .map(
                                fragment => `
                                    <div class="tool-fragment">
                                        <strong>${escapeHtml(fragment.name)}</strong>
                                        <div>${renderMultilineText(fragment.description)}</div>
                                    </div>
                                `
                            )
                            .join('')}
                    </div>
                `
        );
    }

    if (promptView.contextSummary) {
        sections.push(renderPromptSection('Context', promptView.contextSummary));
    }

    if (promptView.capabilityPolicy) {
        sections.push(renderPromptSection('Capability Policy', promptView.capabilityPolicy));
    }

    return sections.join('');
}

function renderPromptSection(label: string, content: string): string {
    const value = content ? renderMultilineText(truncateText(content, 500)) : `<span class="empty-hint">${escapeHtml(t('console.no_data'))}</span>`;

    return `
            <div class="prompt-segment">
                <div class="segment-label">${escapeHtml(label)}</div>
                <div class="segment-content">${value}</div>
            </div>
        `;
}

function renderMultilineText(content: string): string {
    return escapeHtml(content).replace(/\r?\n/g, '<br>');
}

function truncateText(text: string, maxLen: number): string {
    if (text.length <= maxLen) return text;
    return `${text.slice(0, maxLen)}...`;
}

export function hasSessionOverride(
    orchestrationDetail?: ModelBindingDetailView | { source?: string },
    executionDetail?: ModelBindingDetailView | { source?: string }
): boolean {
    return orchestrationDetail?.source === 'session_override' || executionDetail?.source === 'session_override';
}
