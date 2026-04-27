import { t } from '../i18n/index';
import { escapeHtml } from '../utils/html';
import type { AgentRuntimeSnapshot, MemoryHitView, SkillBindingView, ToolDescriptorView } from '../core/types';

type ToolStatus = Pick<ToolDescriptorView, 'lastCallStatus'>;

export type ToolListItem = ToolDescriptorView & ToolStatus;

export interface SkillPanelItem {
    id: string;
    label: string;
    enabled: boolean;
    source: string;
    sticky?: boolean;
    contentPreview?: string;
}

export function renderToolsPanelHtml(tools: ToolListItem[], availableTools: string[]): string {
    if (tools.length === 0) {
        if (availableTools.length === 0) {
            return `<div class="empty-hint">${escapeHtml(t('console.no_data'))}</div>`;
        }

        const summary = `${availableTools.length} ${t('common.results')}`;
        return `
                <div class="tool-summary">${escapeHtml(summary)}</div>
                ${availableTools
                    .map(
                        toolName => `
                            <div class="tool-item">
                                <span class="tool-name">${escapeHtml(toolName)}</span>
                                <span class="tool-source-badge">${escapeHtml(t('common.none'))}</span>
                            </div>
                        `
                    )
                    .join('')}
            `;
    }

    const unlockedCount = tools.filter(item => item.source === 'skill_unlocked').length;
    const runningCount = tools.filter(item => item.lastCallStatus === 'running').length;

    return `
            <div class="tool-summary">
                <span>${escapeHtml(String(tools.length))} ${t('console.tools_count')}</span>
                ${runningCount > 0 ? `<span class="tool-running-badge">${escapeHtml(String(runningCount))} ${t('tools.running')}</span>` : ''}
                ${unlockedCount > 0 ? `<span class="tool-unlocked-badge">${escapeHtml(String(unlockedCount))} ${t('tools.unlocked')}</span>` : ''}
            </div>
            ${tools
                .map(tool => {
                    const statusClass = tool.lastCallStatus === 'success' ? 'tool-success' : tool.lastCallStatus === 'error' ? 'tool-error' : '';
                    const unlockedBadge = tool.source === 'skill_unlocked' ? ' <span class="tool-new-badge">NEW</span>' : '';
                    return `
                            <div class="tool-item ${statusClass}">
                                <span class="tool-name">${escapeHtml(tool.name)}${unlockedBadge}</span>
                                <span class="tool-source-badge ${escapeHtml(tool.source)}">${escapeHtml(tool.source)}</span>
                                <span class="tool-desc">${escapeHtml(tool.description)}</span>
                            </div>
                        `;
                })
                .join('')}
        `;
}

export function mergeSkillPanelItems(
    skillBindings: SkillBindingView[],
    agentSkills: AgentRuntimeSnapshot['skills'] = [],
    activeSkills: string[] = []
): SkillPanelItem[] {
    const bindingMap = new Map(skillBindings.map(item => [item.id, item] as const));
    const items: SkillPanelItem[] = skillBindings.map(binding => ({
        id: binding.id,
        label: binding.title,
        enabled: binding.enabled,
        source: binding.source,
        sticky: binding.sticky,
        contentPreview: binding.contentPreview,
    }));

    agentSkills.forEach(skill => {
        if (!bindingMap.has(skill.id)) {
            items.push({ id: skill.id, label: skill.title || skill.id, enabled: skill.enabled, source: 'agent' });
        }
    });

    activeSkills.forEach(skillId => {
        if (!bindingMap.has(skillId) && !agentSkills.find(item => item.id === skillId)) {
            items.push({ id: skillId, label: skillId, enabled: true, source: 'runtime' });
        }
    });

    return items;
}

export function renderSkillsPanelHtml(items: SkillPanelItem[]): string {
    const runtimeCount = items.filter(item => item.source === 'runtime').length;
    const summaryHtml = runtimeCount > 0 ? ` <span class="skill-runtime-badge">${escapeHtml(String(runtimeCount))} ${t('skills.runtime')}</span>` : '';

    return `
            <div class="skill-summary">${escapeHtml(String(items.length))} ${t('console.skills_count')}${summaryHtml}</div>
            ${items
                .map(
                    skill => `
                        <div class="skill-item ${skill.enabled ? 'enabled' : 'disabled'} ${skill.sticky ? 'skill-sticky' : ''}" data-skill-id="${escapeHtml(skill.id)}">
                            <span class="skill-status-dot"></span>
                            <span class="skill-name">${escapeHtml(skill.label)}</span>
                            <span class="skill-source-badge ${skill.source === 'runtime' ? 'skill-runtime-badge' : ''}">${escapeHtml(skill.source)}</span>
                            ${skill.sticky ? '<span class="skill-sticky-badge">📌</span>' : ''}
                        </div>
                    `
                )
                .join('')}
        `;
}

export function renderMemoryHitsHtml(hits: MemoryHitView[], approximate: boolean): string {
    const approximateWarning = approximate
        ? `<div class="memory-approximate-warning">${escapeHtml(t('console.memory_approximate'))}</div>`
        : '';

    const semanticHits = hits.filter(hit => hit.sourceType === 'semantic').length;
    const keywordHits = hits.filter(hit => hit.sourceType === 'keyword').length;
    const distillationHits = hits.filter(hit => hit.sourceType === 'distillation').length;
    const hitSummary = [
        semanticHits > 0 ? `${semanticHits} ${t('memory.semantic')}` : '',
        keywordHits > 0 ? `${keywordHits} ${t('memory.keyword')}` : '',
        distillationHits > 0 ? `${distillationHits} ${t('memory.distillation')}` : '',
    ]
        .filter(Boolean)
        .join(', ');
    const summaryHtml = hitSummary ? `<div class="memory-hit-summary">${escapeHtml(hitSummary)}</div>` : '';

    return `
            ${approximateWarning}
            ${summaryHtml}
            ${hits
                .map(
                    hit => `
                        <div class="memory-hit-item">
                            <span class="memory-hit-score">${escapeHtml((hit.score * 100).toFixed(1))}%</span>
                            <span class="memory-hit-content">${escapeHtml(hit.content)}</span>
                            <span class="memory-hit-source">${escapeHtml(hit.source)}${hit.sourceType ? `<span title="${hit.sourceType}"> · ${escapeHtml(hit.sourceType)}</span>` : ''}</span>
                        </div>
                    `
                )
                .join('')}
        `;
}
