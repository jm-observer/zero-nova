import { AppState } from '../core/state';
import { EventBus, Events } from '../core/event-bus';
import { AGENT_CONSOLE_TEMPLATE } from './templates/agent-console-template';
import { t } from '../i18n/index';
import { escapeHtml, formatTime } from '../utils/html';
import type {
    AgentRuntimeSnapshot,
    ModelBindingDetailView,
    SessionRuntimeSnapshot,
    MemoryHitView,
    PromptPreviewView,
    ResourceState,
    ToolDescriptorView,
    TokenUsageView,
} from '../core/types';

type ConsoleTab = 'overview' | 'model' | 'tools' | 'skills' | 'prompt-memory';
type ConnectionStatus = 'connecting' | 'connected' | 'disconnected' | 'reconnecting' | 'failed';

interface ConsoleTogglePayload {
    visible: boolean;
}

interface ConsoleTabPayload {
    tab: ConsoleTab;
}

interface ConsoleDataPayload {
    sessionId: string | null;
}

interface GatewayStatusPayload {
    status: ConnectionStatus;
}

/**
 * Agent 控制台视图类
 * 负责运行态可观测与临时控制界面的渲染和交互
 */
export class AgentConsoleView {
    private container: HTMLElement | null = null;
    private tabs: NodeListOf<HTMLButtonElement> | null = null;
    private refreshBtn: HTMLButtonElement | null = null;
    private closeBtn: HTMLButtonElement | null = null;
    private isDisposed = false;
    private connectionStatus: ConnectionStatus = 'connecting';
    private unsubs: Array<() => void> = [];
    private keydownHandler = (event: KeyboardEvent) => {
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'i') {
            event.preventDefault();
            this.state.setConsoleVisible(!this.state.consoleVisible);
        }
    };

    constructor(private state: AppState, private bus: EventBus) {}

    init() {
        const consoleEl = document.getElementById('agent-console');
        if (!consoleEl) return;

        this.container = consoleEl;
        this.isDisposed = false;
        this.container.innerHTML = AGENT_CONSOLE_TEMPLATE;
        this.tabs = this.container.querySelectorAll('.agent-console-tab');
        this.refreshBtn = this.container.querySelector('.agent-console-refresh');
        this.closeBtn = this.container.querySelector('.agent-console-close');

        this.bindEvents();
        this.listenBus();
        this.updateTabUI(this.state.consoleActiveTab);
        this.renderCurrentTab();
        this.updateFooter();
    }

    dispose() {
        this.isDisposed = true;
        this.unsubs.forEach(unsub => unsub());
        this.unsubs = [];
        window.removeEventListener('keydown', this.keydownHandler);
    }

    private bindEvents() {
        this.tabs?.forEach(tab => {
            tab.addEventListener('click', () => {
                const target = tab.getAttribute('data-tab') as ConsoleTab | null;
                if (target) {
                    this.state.setConsoleTab(target);
                }
            });
        });

        this.refreshBtn?.addEventListener('click', () => {
            void this.refreshCurrentData();
        });

        this.closeBtn?.addEventListener('click', () => {
            this.state.setConsoleVisible(false);
        });

        this.container?.addEventListener('click', event => {
            const target = event.target as HTMLElement;
            const summaryCard = target.closest('.summary-card.clickable');
            if (!summaryCard) return;

            const targetTab = summaryCard.getAttribute('data-goto-tab') as ConsoleTab | null;
            if (targetTab) {
                this.state.setConsoleTab(targetTab);
            }
        });

        window.addEventListener('keydown', this.keydownHandler);
    }

    private listenBus() {
        this.unsubs.push(
            this.bus.on<ConsoleTogglePayload>(Events.CONSOLE_TOGGLED, payload => {
                if (!payload) return;

                if (payload.visible) {
                    this.container?.classList.remove('hidden');
                    this.syncLayout(true);
                    void this.refreshCurrentData();
                } else {
                    this.container?.classList.add('hidden');
                    this.syncLayout(false);
                }
            }),
            this.bus.on<ConsoleTabPayload>(Events.CONSOLE_TAB_CHANGED, payload => {
                if (!payload) return;

                this.updateTabUI(payload.tab);
                void this.loadTabDataIfNeeded(payload.tab);
            }),
            this.bus.on<ConsoleDataPayload>(Events.CONSOLE_DATA_UPDATED, payload => {
                if (!payload) return;

                if (payload.sessionId === null || payload.sessionId === this.state.currentSessionId) {
                    this.renderCurrentTab();
                    this.updateFooter();
                }
            }),
            this.bus.on('tool:start', (event: { sessionId?: string; tool?: string; toolName?: string }) => {
                const toolName = event.toolName ?? event.tool;
                if (!toolName) return;

                this.state.updateToolStatus(toolName, { lastCallStatus: 'running', lastUsedAt: Date.now() });
            }),
            this.bus.on('tool:result', (event: { sessionId?: string; tool?: string; toolName?: string; isError?: boolean }) => {
                const toolName = event.toolName ?? event.tool;
                if (!toolName) return;

                this.state.updateToolStatus(toolName, {
                    lastCallStatus: event.isError ? 'error' : 'success',
                    lastUsedAt: Date.now(),
                });
            }),
            this.bus.on(Events.SESSION_SELECTED, () => {
                if (this.state.consoleVisible) {
                    void this.refreshCurrentData();
                }
            }),
            this.bus.on(Events.AGENT_SWITCHED, () => {
                this.state.updateResourceState('agentRuntime', this.state.createEmptyResource());
                if (this.state.consoleVisible) {
                    void this.refreshCurrentData();
                }
            }),
            this.bus.on<GatewayStatusPayload>(Events.GATEWAY_STATUS, payload => {
                if (!payload) return;

                this.connectionStatus = payload.status;
                this.updateFooter();
                if (payload.status === 'connected' && this.state.consoleVisible) {
                    void this.loadTabDataIfNeeded(this.state.consoleActiveTab, true);
                }
            })
        );
    }

    private syncLayout(consoleVisible: boolean) {
        const workspace = document.getElementById('workspace');
        if (workspace) {
            workspace.classList.toggle('console-open', consoleVisible && window.innerWidth >= 1024);
        }

        const artifactsPanel = document.querySelector('.artifacts-panel');
        if (artifactsPanel) {
            artifactsPanel.classList.toggle('collapsed', consoleVisible);
        }

        window.dispatchEvent(new Event('resize'));
    }

    private updateTabUI(activeTab: ConsoleTab) {
        this.tabs?.forEach(tab => {
            tab.classList.toggle('active', tab.getAttribute('data-tab') === activeTab);
        });

        const contents = this.container?.querySelectorAll('.console-tab-content');
        contents?.forEach(content => {
            content.classList.toggle('active', content.getAttribute('data-tab') === activeTab);
        });
    }

    private async refreshCurrentData() {
        if (this.isDisposed) return;

        const sessionId = this.state.currentSessionId;
        if (!sessionId || !this.state.currentAgentId) {
            this.renderCurrentTab();
            this.updateFooter();
            return;
        }

        await this.loadOverviewData(sessionId, true);
        if (this.state.consoleActiveTab !== 'overview') {
            await this.loadTabDataIfNeeded(this.state.consoleActiveTab, true);
        }
        this.updateUpdateTime();
    }

    private async loadTabDataIfNeeded(tab: ConsoleTab, force = false) {
        const sessionId = this.state.currentSessionId;
        if (!sessionId) return;

        switch (tab) {
            case 'overview':
                await this.loadOverviewData(sessionId, force);
                break;
            case 'model':
                await this.loadModelData(sessionId, force);
                break;
            case 'tools':
                await this.loadToolsData(sessionId, force);
                break;
            case 'skills':
                await this.loadSkillsData(sessionId, force);
                break;
            case 'prompt-memory':
                await this.loadPromptMemoryData(sessionId, force);
                break;
        }
    }

    private async loadOverviewData(sessionId: string, force = false) {
        if (!this.state.gatewayClient || this.isDisposed) return;

        const agentState = this.state.agentRuntimeState;
        const tokenState = this.state.getSessionResourceState(sessionId, 'tokenUsage');
        if (!force && agentState.loaded && tokenState?.loaded) {
            this.renderOverview();
            return;
        }

        this.state.updateResourceState('agentRuntime', this.state.setLoadingResource(agentState));
        const currentTokenState = (tokenState as ResourceState<TokenUsageView> | undefined) ?? this.state.createEmptyResource();
        this.state.updateSessionResourceState(sessionId, 'tokenUsage', this.state.setLoadingResource(currentTokenState));

        try {
            const [snapshot, usage] = await Promise.all([
                this.state.gatewayClient.getAgentInspect(),
                this.state.gatewayClient.getSessionTokenUsage(sessionId),
            ]);

            this.state.updateResourceState('agentRuntime', this.state.setLoadedResource(snapshot));
            this.state.updateSessionResourceState(sessionId, 'tokenUsage', this.state.setLoadedResource(usage));
        } catch (error) {
            const message = error instanceof Error ? error.message : t('common.load_failed');
            this.state.updateResourceState('agentRuntime', this.state.setErrorResource(message));
            this.state.updateSessionResourceState(sessionId, 'tokenUsage', this.state.setErrorResource(message));
        }
    }

    private async loadModelData(sessionId: string, force = false) {
        // 加载模型绑定数据（包括 orchestrationDetail / executionDetail）
        if (!this.state.gatewayClient || this.isDisposed) return;

        if (force) {
            this.state.updateResourceState('agentRuntime', this.state.setLoadingResource(this.state.agentRuntimeState));
        }

        const currentRuntime = this.state.getSessionResourceState(sessionId, 'runtime') ?? this.state.createEmptyResource();
        if (force) {
            this.state.updateSessionResourceState(sessionId, 'runtime', this.state.setLoadingResource(currentRuntime));
        }

        try {
            const [snapshot, runtime] = await Promise.all([
                this.state.gatewayClient.getAgentInspect(),
                this.state.gatewayClient.getSessionRuntime(sessionId),
            ]);

            this.state.updateResourceState('agentRuntime', this.state.setLoadedResource(snapshot));
            this.state.updateSessionResourceState(sessionId, 'runtime', this.state.setLoadedResource(runtime));
            this.state.updateSessionResourceState(sessionId, 'tokenUsage', this.state.setLoadedResource(runtime.totalUsage));
        } catch (error) {
            const message = error instanceof Error ? error.message : t('common.load_failed');
            this.state.updateResourceState('agentRuntime', this.state.setErrorResource(message));
            this.state.updateSessionResourceState(sessionId, 'runtime', this.state.setErrorResource(message));
            this.state.updateSessionResourceState(sessionId, 'tokenUsage', this.state.setErrorResource(message));
        }

        this.renderModel();
    }

    private async loadToolsData(sessionId: string, force = false) {
        if (!this.state.gatewayClient || this.isDisposed) return;

        const current = this.state.getSessionResourceState(sessionId, 'tools') as ResourceState<ToolDescriptorView[]> | undefined;
        if (!force && current?.loaded) {
            this.renderTools();
            return;
        }

        this.state.updateSessionResourceState(
            sessionId,
            'tools',
            this.state.setLoadingResource(current ?? this.state.createEmptyResource())
        );

        try {
            const tools = await this.state.gatewayClient.getSessionTools(sessionId);
            const mergedTools = tools.map(tool => ({ ...tool, ...this.state.getToolStatus(tool.name) }));
            this.state.updateSessionResourceState(sessionId, 'tools', this.state.setLoadedResource(mergedTools));
        } catch (error) {
            const message = error instanceof Error ? error.message : t('common.load_failed');
            this.state.updateSessionResourceState(sessionId, 'tools', this.state.setErrorResource(message));
        }
    }

    private async loadSkillsData(sessionId: string, force = false) {
        if (!this.state.gatewayClient || this.isDisposed) return;

        await this.loadOverviewData(sessionId, force);

        if (!force && this.state.getAllSkillBindings().size > 0) {
            this.renderSkills();
            return;
        }

        try {
            const bindings = await this.state.gatewayClient.getSessionSkillBindings(sessionId);
            this.state.skillBindingStates.clear();
            bindings.forEach(binding => {
                this.state.setSkillBinding(binding.id, binding);
            });
        } catch {
            this.state.skillBindingStates.clear();
        }

        this.renderSkills();
    }

    private async loadPromptMemoryData(sessionId: string, force = false) {
        if (!this.state.gatewayClient || this.isDisposed) return;

        const currentPrompt = this.state.getSessionResourceState(sessionId, 'prompt') as ResourceState<PromptPreviewView> | undefined;
        const currentMemory = this.state.getSessionResourceState(sessionId, 'memory') as ResourceState<MemoryHitView[]> | undefined;
        if (!force && currentPrompt?.loaded && currentMemory?.loaded) {
            this.renderPromptMemory();
            return;
        }

        this.state.updateSessionResourceState(
            sessionId,
            'prompt',
            this.state.setLoadingResource(currentPrompt ?? this.state.createEmptyResource())
        );
        this.state.updateSessionResourceState(
            sessionId,
            'memory',
            this.state.setLoadingResource(currentMemory ?? this.state.createEmptyResource())
        );

        try {
            const [promptPreview, memoryHits] = await Promise.all([
                this.state.gatewayClient.getSessionPromptPreview(sessionId),
                this.state.gatewayClient.getSessionMemoryHits(sessionId),
            ]);

            this.state.updateSessionResourceState(sessionId, 'prompt', this.state.setLoadedResource(promptPreview));
            this.state.updateSessionResourceState(sessionId, 'memory', this.state.setLoadedResource(memoryHits));
        } catch (error) {
            const message = error instanceof Error ? error.message : t('common.load_failed');
            this.state.updateSessionResourceState(sessionId, 'prompt', this.state.setErrorResource(message));
            this.state.updateSessionResourceState(sessionId, 'memory', this.state.setErrorResource(message));
        }
    }

    private renderCurrentTab() {
        switch (this.state.consoleActiveTab) {
            case 'overview':
                this.renderOverview();
                break;
            case 'model':
                this.renderModel();
                break;
            case 'tools':
                this.renderTools();
                break;
            case 'skills':
                this.renderSkills();
                break;
            case 'prompt-memory':
                this.renderPromptMemory();
                break;
        }
    }

    private renderOverview() {
        if (this.isDisposed) return;

        const agentState = this.state.agentRuntimeState;
        const agentData = agentState.data;
        const sessionId = this.state.currentSessionId;
        const usageState = sessionId
            ? (this.state.getSessionResourceState(sessionId, 'tokenUsage') as ResourceState<TokenUsageView> | undefined)
            : undefined;
        const usageData = usageState?.data;

        const statusCard = document.getElementById('console-runtime-card');
        if (statusCard) {
            statusCard.innerHTML = this.renderStatusCard(agentState);
        }

        this.setTextContent('summary-model-name', agentData?.model?.model ?? '—');
        this.setTextContent('summary-tokens-total', usageData ? String(usageData.inputTokens + usageData.outputTokens) : '0');
        this.setTextContent('summary-tools-count', String(agentData?.availableTools.length ?? 0));
        this.setTextContent('summary-skills-count', String(agentData?.skills?.length ?? agentData?.activeSkills.length ?? 0));
    }

    private renderStatusCard(state: ResourceState<AgentRuntimeSnapshot>): string {
        if (state.loading && !state.data) {
            return '<div class="skeleton-text"></div>';
        }

        if (state.error) {
            return `<div class="empty-hint">${escapeHtml(state.error)}</div>`;
        }

        const status = state.data?.status ?? 'idle';
        const statusKey = status === 'running' ? 'console.status_running' : 'console.status_idle';

        return `
            <div class="runtime-status-display">
                <span class="status-badge ${status}">${escapeHtml(t(statusKey))}</span>
                <span class="agent-id-small">${escapeHtml(this.state.currentAgentId ?? '')}</span>
            </div>
        `;
    }

    private renderModel() {
        if (this.isDisposed) return;

        const agentData = this.state.agentRuntimeState.data;
        const sessionId = this.state.currentSessionId;
        const usageState = sessionId
            ? (this.state.getSessionResourceState(sessionId, 'tokenUsage') as ResourceState<TokenUsageView> | undefined)
            : undefined;
        const usageData = usageState?.data;

        const modelSettings = document.getElementById('console-model-settings');
        if (modelSettings) {
            if (this.state.agentRuntimeState.error) {
                modelSettings.innerHTML = `<div class="empty-hint">${escapeHtml(this.state.agentRuntimeState.error)}</div>`;
            } else {
                // 使用 SessionRuntimeSnapshot 中的双模型绑定数据（Plan 2 扩展）
                const orchRuntime = sessionId
                    ? (this.state.getSessionResourceState(sessionId, 'runtime') as ResourceState<SessionRuntimeSnapshot> | undefined)
                    : undefined;
                const orchDetail = orchRuntime?.data;
                const orchestrationDetail = orchDetail?.orchestrationDetail;
                const executionDetail = orchDetail?.executionDetail;

                // 使用 agentRuntimeSnapshot 中的 model 作为备选
                const orchFromAgent = agentData?.model ? {
                    provider: agentData.model.provider,
                    model: agentData.model.model,
                    source: agentData.model.source,
                } : undefined;
                const execFromAgent = orchFromAgent ? { ...orchFromAgent } : undefined;

                const orchBinding = orchestrationDetail || orchFromAgent;
                const execBinding = executionDetail || execFromAgent;

                const get_source_label = (source: string): string => {
                    switch (source) {
                        case 'session_override': return t('console.source_session');
                        case 'agent': return t('console.source_agent');
                        default: return t('console.source_global');
                    }
                };

                modelSettings.innerHTML = `
                    <div class="model-config-panel model-binding-panel">
                        <div class="model-item model-binding-item">
                            <span class="model-label">${escapeHtml(t('console.binding_orchestration'))}</span>
                            <span class="model-value">${escapeHtml(orchBinding?.model ?? '—')}</span>
                            <span class="model-source-badge">${escapeHtml(get_source_label(orchBinding?.source ?? 'global'))}</span>
                        </div>
                        <div class="model-item model-binding-item">
                            <span class="model-label">${escapeHtml(t('console.binding_execution'))}</span>
                            <span class="model-value">${escapeHtml(execBinding?.model ?? '—')}</span>
                            <span class="model-source-badge">${escapeHtml(get_source_label(execBinding?.source ?? 'global'))}</span>
                        </div>
                        <div class="model-item model-binding-actions">
                            <button class="model-switch-btn" data-scope="session">${escapeHtml(t('console.switch_model'))}</button>
                            ${this.hasSessionOverride(orchestrationDetail, executionDetail) ? `<button class="model-reset-btn" data-scope="session">${escapeHtml(t('console.restore_inherit'))}</button>` : ''}
                        </div>
                    </div>
                `;

                // 绑定模型切换按钮
                const switchBtn = modelSettings.querySelector('.model-switch-btn');
                if (switchBtn) {
                    switchBtn.addEventListener('click', () => this.handleModelSwitch(sessionId, 'session'));
                }

                // 绑定恢复继承按钮
                const resetBtn = modelSettings.querySelector('.model-reset-btn');
                if (resetBtn) {
                    resetBtn.addEventListener('click', () => this.handleModelReset(sessionId));
                }
            }
        }

        const tokenUsage = document.getElementById('console-token-usage');
        if (!tokenUsage) return;

        if (usageState?.loading && !usageData) {
            tokenUsage.innerHTML = `<div class="empty-hint">${escapeHtml(t('common.loading'))}</div>`;
            return;
        }

        if (usageState?.error) {
            tokenUsage.innerHTML = `<div class="empty-hint">${escapeHtml(usageState.error)}</div>`;
            return;
        }

        if (!usageData) {
            tokenUsage.innerHTML = `<div class="empty-hint">${escapeHtml(t('console.no_data'))}</div>`;
            return;
        }

        const rows = [
            this.renderTokenRow('Input', usageData.inputTokens),
            this.renderTokenRow('Output', usageData.outputTokens),
        ];

        if (usageData.cacheCreationInputTokens) {
            rows.push(this.renderTokenRow('Cache (Create)', usageData.cacheCreationInputTokens, 'cache-row'));
        }
        if (usageData.cacheReadInputTokens) {
            rows.push(this.renderTokenRow('Cache (Read)', usageData.cacheReadInputTokens, 'cache-row'));
        }
        if (usageData.totalCost !== undefined) {
            rows.push(this.renderTokenRow('Est. Cost', usageData.totalCost.toFixed(4), 'cost-row'));
        }

        tokenUsage.innerHTML = `<div class="token-usage-detail">${rows.join('')}</div>`;
    }

    /**
     * 处理模型切换
     */
    private async handleModelSwitch(sessionId: string | null, _scope: 'global' | 'agent' | 'session') {
        if (!sessionId) return;
        if (!this.state.gatewayClient) return;

        try {
            const currentRuntime = await this.state.gatewayClient.getSessionRuntime(sessionId);
            const orchestration = await this.promptForBinding(
                t('console.binding_orchestration'),
                currentRuntime.orchestrationDetail
            );
            if (!orchestration) {
                return;
            }

            const useSameForExecution = window.confirm(`${t('console.binding_execution')} 使用与 orchestration 相同的 provider/model？`);
            const execution = useSameForExecution
                ? orchestration
                : await this.promptForBinding(t('console.binding_execution'), currentRuntime.executionDetail);
            if (!execution) {
                return;
            }

            const snapshot = await this.state.gatewayClient.setSessionModelOverride(sessionId, {
                orchestration,
                execution,
            });
            this.state.updateSessionResourceState(
                sessionId,
                'runtime',
                this.state.setLoadedResource(snapshot as SessionRuntimeSnapshot)
            );
            this.state.updateSessionResourceState(
                sessionId,
                'tokenUsage',
                this.state.setLoadedResource(snapshot.totalUsage)
            );
            this.bus.emit(Events.NOTIFICATION, { type: 'success', message: t('console.model_switched') });
            this.renderModel();
        } catch (error) {
            const message = error instanceof Error ? error.message : t('console.model_switch_failed');
            this.bus.emit(Events.NOTIFICATION, { type: 'error', message });
        }
    }

    /**
     * 处理模型恢复继承
     */
    private async handleModelReset(sessionId: string | null) {
        if (!sessionId) return;
        if (!this.state.gatewayClient) return;

        try {
            const snapshot = await this.state.gatewayClient.resetSessionModelOverride(sessionId);
            this.state.updateSessionResourceState(
                sessionId,
                'runtime',
                this.state.setLoadedResource(snapshot as SessionRuntimeSnapshot)
            );
            this.state.updateSessionResourceState(
                sessionId,
                'tokenUsage',
                this.state.setLoadedResource(snapshot.totalUsage)
            );
            this.bus.emit(Events.NOTIFICATION, { type: 'success', message: t('console.model_reset') });
            this.renderModel();
        } catch (error) {
            const message = error instanceof Error ? error.message : t('console.model_reset_failed');
            this.bus.emit(Events.NOTIFICATION, { type: 'error', message });
        }
    }

    private renderTokenRow(label: string, value: number | string, className = ''): string {
        return `
            <div class="token-row ${className}">
                <span class="token-label">${escapeHtml(label)}</span>
                <span class="token-value">${escapeHtml(String(value))}</span>
            </div>
        `;
    }

    private renderTools() {
        if (this.isDisposed) return;

        const sessionId = this.state.currentSessionId;
        const toolsState = sessionId
            ? (this.state.getSessionResourceState(sessionId, 'tools') as ResourceState<ToolDescriptorView[]> | undefined)
            : undefined;
        const toolsList = document.getElementById('console-tools-list');
        if (!toolsList) return;

        if (toolsState?.loading && !toolsState.data) {
            toolsList.innerHTML = `<div class="empty-hint">${escapeHtml(t('common.loading'))}</div>`;
            return;
        }

        if (toolsState?.error) {
            toolsList.innerHTML = `<div class="empty-hint">${escapeHtml(toolsState.error)}</div>`;
            return;
        }

        const tools = (toolsState?.data ?? []).map(tool => ({ ...tool, ...this.state.getToolStatus(tool.name) }));
        if (tools.length === 0) {
            const availableTools = this.state.agentRuntimeState.data?.availableTools ?? [];
            if (availableTools.length === 0) {
                toolsList.innerHTML = `<div class="empty-hint">${escapeHtml(t('console.no_data'))}</div>`;
                return;
            }

            const summary = `${availableTools.length} ${t('common.results')}`;
            toolsList.innerHTML = `
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
            return;
        }

        // 统计各类工具数量
        const unlockedCount = tools.filter(t => t.source === 'skill_unlocked').length;
        const runnningCount = tools.filter(t => t.lastCallStatus === 'running').length;

        toolsList.innerHTML = `
            <div class="tool-summary">
                <span>${escapeHtml(String(tools.length))} ${t('console.tools_count')}</span>
                ${runnningCount > 0 ? `<span class="tool-running-badge">${escapeHtml(String(runnningCount))} ${t('tools.running')}</span>` : ''}
                ${unlockedCount > 0 ? `<span class="tool-unlocked-badge">${escapeHtml(String(unlockedCount))} ${t('tools.unlocked')}</span>` : ''}
            </div>
            ${tools
                .map(
                    tool => {
                        const statusClass = tool.lastCallStatus === 'success' ? 'tool-success' : tool.lastCallStatus === 'error' ? 'tool-error' : '';
                        const unlockedBadge = tool.source === 'skill_unlocked' ? ' <span class="tool-new-badge">NEW</span>' : '';
                        return `
                            <div class="tool-item ${statusClass}">
                                <span class="tool-name">${escapeHtml(tool.name)}${unlockedBadge}</span>
                                <span class="tool-source-badge ${escapeHtml(tool.source)}">${escapeHtml(tool.source)}</span>
                                <span class="tool-desc">${escapeHtml(tool.description)}</span>
                            </div>
                        `;
                    }
                )
                .join('')}
        `;
    }

    private renderSkills() {
        if (this.isDisposed) return;

        const skillsList = document.getElementById('console-skills-list');
        if (!skillsList) return;

        // 优先使用 SkillBindingView 缓存中的数据（Plan 3 运行时绑定）
        const skillBindings = this.state.getAllSkillBindings();
        const agentSkills = this.state.agentRuntimeState.data?.skills ?? [];
        const activeSkills = this.state.agentRuntimeState.data?.activeSkills ?? [];

        // 合并 SkillBindingView 和 agentSkills
        const items: Array<{ id: string; label: string; enabled: boolean; source: string; sticky?: boolean; contentPreview?: string }> = [];
        for (const [id, binding] of skillBindings) {
            items.push({ id, label: binding.title, enabled: binding.enabled, source: binding.source, sticky: binding.sticky, contentPreview: binding.contentPreview });
        }
        // 补充不在 SkillBindingView 中的 agentSkills
        agentSkills.forEach(skill => {
            if (!skillBindings.has(skill.id)) {
                items.push({ id: skill.id, label: skill.title || skill.id, enabled: skill.enabled, source: 'agent' });
            }
        });
        // 补充 pure activeSkills（string 类型）
        activeSkills.forEach(skillId => {
            if (!skillBindings.has(skillId) && !agentSkills.find(s => s.id === skillId)) {
                items.push({ id: skillId, label: skillId, enabled: true, source: 'runtime' });
            }
        });

        if (items.length === 0) {
            skillsList.innerHTML = `<div class="empty-hint">${escapeHtml(t('console.no_data'))}</div>`;
            return;
        }

        // 统计不同类型
        const runtimeCount = items.filter(i => i.source === 'runtime').length;
        const summaryHtml = runtimeCount > 0 ? ` <span class="skill-runtime-badge">${escapeHtml(String(runtimeCount))} ${t('skills.runtime')}</span>` : '';

        skillsList.innerHTML = `
            <div class="skill-summary">${escapeHtml(String(items.length))} ${t('console.skills_count')}${summaryHtml}</div>
            ${items
                .map(
                    skill => `
                        <div class="skill-item ${skill.enabled ? 'enabled' : 'disabled'} ${skill.sticky ? 'skill-sticky' : ''}">
                            <span class="skill-status-dot"></span>
                            <span class="skill-name">${escapeHtml(skill.label)}</span>
                            <span class="skill-source-badge ${skill.source === 'runtime' ? 'skill-runtime-badge' : ''}">${escapeHtml(skill.source)}</span>
                            ${skill.sticky ? '<span class="skill-sticky-badge">📌</span>' : ''}
                        </div>
                    `
                )
                .join('')}
        `;

        // 绑定技能详情点击事件
        const skillItems = skillsList.querySelectorAll('.skill-item');
        skillItems.forEach((item) => {
            const el = item as HTMLElement;
            el.style.cursor = 'pointer';
            el.addEventListener('click', () => {
                const skillId = (el.dataset.skillId || el.querySelector('.skill-name')?.textContent)?.trim();
                if (!skillId) return;
                const binding = skillBindings.get(skillId);
                if (binding && binding.contentPreview) {
                    this.bus.emit(Events.NOTIFICATION, { type: 'info', message: `${binding.title}:\n${binding.contentPreview}` });
                }
            });
            // 保存 skillId 供点击事件使用
            el.dataset.skillId = items[Array.from(skillItems).indexOf(el)]?.id || '';
        });
    }

    private renderPromptMemory() {
        if (this.isDisposed) return;

        const sessionId = this.state.currentSessionId;
        const promptState = sessionId
            ? (this.state.getSessionResourceState(sessionId, 'prompt') as ResourceState<PromptPreviewView> | undefined)
            : undefined;
        const memoryState = sessionId
            ? (this.state.getSessionResourceState(sessionId, 'memory') as ResourceState<MemoryHitView[]> | undefined)
            : undefined;

        const promptPreview = document.getElementById('console-prompt-preview');
        if (promptPreview) {
            if (promptState?.loading && !promptState.data) {
                promptPreview.innerHTML = `<div class="empty-hint">${escapeHtml(t('common.loading'))}</div>`;
            } else if (promptState?.error) {
                promptPreview.innerHTML = `<div class="empty-hint">${escapeHtml(promptState.error)}</div>`;
            } else if (promptState?.data) {
                promptPreview.innerHTML = this.renderPromptSegments(promptState.data);
            } else {
                promptPreview.innerHTML = `<div class="empty-hint">${escapeHtml(t('console.no_data'))}</div>`;
            }
        }

        const memoryHits = document.getElementById('console-memory-hits');
        if (!memoryHits) return;

        if (memoryState?.loading && !memoryState.data) {
            memoryHits.innerHTML = `<div class="empty-hint">${escapeHtml(t('common.loading'))}</div>`;
            return;
        }

        if (memoryState?.error) {
            memoryHits.innerHTML = `<div class="empty-hint">${escapeHtml(memoryState.error)}</div>`;
            return;
        }

        const hits = memoryState?.data ?? [];
        if (hits.length === 0) {
            memoryHits.innerHTML = `<div class="empty-hint">${escapeHtml(t('console.no_data'))}</div>`;
            return;
        }

        // 统计各类命中率
        const semanticHits = hits.filter(h => h.sourceType === 'semantic').length;
        const keywordHits = hits.filter(h => h.sourceType === 'keyword').length;
        const distillationHits = hits.filter(h => h.sourceType === 'distillation').length;
        const hitSummary = [semanticHits > 0 ? `${semanticHits} ${t('memory.semantic')}` : '', keywordHits > 0 ? `${keywordHits} ${t('memory.keyword')}` : '', distillationHits > 0 ? `${distillationHits} ${t('memory.distillation')}` : ''].filter(Boolean).join(', ');
        const summaryHtml = hitSummary ? `<div class="memory-hit-summary">${escapeHtml(hitSummary)}</div>` : '';

        memoryHits.innerHTML = `
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

    private renderPromptSegments(promptView: PromptPreviewView): string {
        const sections: string[] = [];

        sections.push(this.renderPromptSection(t('console.prompt_preview'), promptView.systemPrompt));

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
                                        <div>${this.renderMultilineText(fragment.content)}</div>
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
                            .map(fragment => `<div class="memory-fragment">${this.renderMultilineText(fragment.content)}</div>`)
                            .join('')}
                    </div>
                `
            );
        }

        const toolDescriptions = promptView.toolDescriptions ?? promptView.toolFragments;
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
                                        <div>${this.renderMultilineText(fragment.description)}</div>
                                    </div>
                                `
                            )
                            .join('')}
                    </div>
                `
            );
        }

        if (promptView.contextSummary) {
            sections.push(this.renderPromptSection('Context', promptView.contextSummary));
        }

        if (promptView.capabilityPolicy) {
            sections.push(this.renderPromptSection('Capability Policy', promptView.capabilityPolicy));
        }

        return sections.join('');
    }

    private renderPromptSection(label: string, content: string): string {
        const value = content ? this.renderMultilineText(this.truncateText(content, 500)) : `<span class="empty-hint">${escapeHtml(t('console.no_data'))}</span>`;

        return `
            <div class="prompt-segment">
                <div class="segment-label">${escapeHtml(label)}</div>
                <div class="segment-content">${value}</div>
            </div>
        `;
    }

    private renderMultilineText(content: string): string {
        return escapeHtml(content).replace(/\r?\n/g, '<br>');
    }

    private truncateText(text: string, maxLen: number): string {
        if (text.length <= maxLen) return text;
        return `${text.slice(0, maxLen)}...`;
    }

    private hasSessionOverride(
        orchestrationDetail?: ModelBindingDetailView | { source?: string },
        executionDetail?: ModelBindingDetailView | { source?: string }
    ): boolean {
        return orchestrationDetail?.source === 'session_override' || executionDetail?.source === 'session_override';
    }

    private async promptForBinding(
        label: string,
        current?: { provider: string; model: string }
    ): Promise<{ provider: string; model: string } | null> {
        const provider = window.prompt(`${label} provider`, current?.provider ?? '');
        if (!provider) {
            return null;
        }

        const model = window.prompt(`${label} model`, current?.model ?? '');
        if (!model) {
            return null;
        }

        return { provider: provider.trim(), model: model.trim() };
    }

    private updateUpdateTime() {
        const el = document.getElementById('console-update-time');
        if (el) {
            el.textContent = `${t('console.last_updated')}: ${formatTime(Date.now())}`;
        }
    }

    private updateFooter() {
        const dataSource = this.container?.querySelector('.data-source');
        if (!dataSource) return;

        const key =
            this.connectionStatus === 'connected' ? 'console.live_data' : 'console.data_stale';
        dataSource.textContent = t(key);
        dataSource.classList.toggle('stale', this.connectionStatus !== 'connected');
    }

    private setTextContent(id: string, value: string) {
        const element = document.getElementById(id);
        if (element) {
            element.textContent = value;
        }
    }
}
