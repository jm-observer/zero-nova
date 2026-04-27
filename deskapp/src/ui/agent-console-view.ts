import { invoke } from '@tauri-apps/api/core';
import { AppState } from '../core/state';
import { EventBus, Events } from '../core/event-bus';
import { GatewayRequestError } from '../gateway-client';
import { AGENT_CONSOLE_TEMPLATE } from './templates/agent-console-template';
import { t } from '../i18n/index';
import { escapeHtml, formatTime } from '../utils/html';
import {
    filterRunsByFilter,
    hasSessionOverride,
    renderAuditList,
    renderCurrentRunCard,
    renderDiagnosticList,
    renderPermissionList,
    renderPromptSegments,
    renderRunDetailCard,
    renderRunFilters,
    renderRunListItem,
    renderTokenRow,
    type RunFilter,
} from './agent-console-renderers';
import type {
    AgentRuntimeSnapshot,
    AuditLogView,
    ConsoleTab,
    DiagnosticIssueView,
    PermissionRequestView,
    RunDetailView,
    RunSummaryView,
    SkillBindingView,
    SessionArtifactView,
    SessionRuntimeSnapshot,
    MemoryHitView,
    PromptPreviewView,
    ResourceState,
    SettingsNavigatePayload,
    ToolDescriptorView,
    TokenUsageView,
    WorkspaceRestoreView,
} from '../core/types';
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
interface NavigateTarget {
    tab?: ConsoleTab;
    itemId?: string;
    settingsSection?: 'models' | 'memory' | 'mcp' | 'skills';
    settingsSearch?: string;
}

export class AgentConsoleView {
    private static readonly WORKSPACE_RESTORE_KEY = 'openflux-console-restore';
    private container: HTMLElement | null = null;
    private tabs: NodeListOf<HTMLButtonElement> | null = null;
    private refreshBtn: HTMLButtonElement | null = null;
    private closeBtn: HTMLButtonElement | null = null;
    private isDisposed = false;
    private connectionStatus: ConnectionStatus = 'connecting';
    private artifactsPanelWasOpen = false;
    private activeRunFilter: RunFilter = 'all';
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
        this.restoreLocalUiState();
        this.updateTabUI(this.state.consoleActiveTab);
        this.renderCurrentTab();
        this.updateFooter();
        if (this.state.consoleVisible) {
            void this.refreshCurrentData();
        }
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
                    this.persistLocalUiState();
                    void this.refreshCurrentData();
                } else {
                    this.container?.classList.add('hidden');
                    this.syncLayout(false);
                    this.persistLocalUiState();
                }
            }),
            this.bus.on<ConsoleTabPayload>(Events.CONSOLE_TAB_CHANGED, payload => {
                if (!payload) return;

                this.updateTabUI(payload.tab);
                this.persistLocalUiState();
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

                const sessionId = event.sessionId ?? this.state.currentSessionId;
                if (!sessionId) return;

                this.state.updateToolStatus(sessionId, toolName, { lastCallStatus: 'running', lastUsedAt: Date.now() });
            }),
            this.bus.on('tool:result', (event: { sessionId?: string; tool?: string; toolName?: string; isError?: boolean }) => {
                const toolName = event.toolName ?? event.tool;
                if (!toolName) return;
                const sessionId = event.sessionId ?? this.state.currentSessionId;
                if (!sessionId) return;

                this.state.updateToolStatus(sessionId, toolName, {
                    lastCallStatus: event.isError ? 'error' : 'success',
                    lastUsedAt: Date.now(),
                });
            }),
            this.bus.on(Events.SESSION_SELECTED, () => {
                this.persistLocalUiState();
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
            if (consoleVisible) {
                // 记录 artifacts-panel 打开前的状态，以便关闭 Console 时恢复
                this.artifactsPanelWasOpen = !artifactsPanel.classList.contains('collapsed');
                artifactsPanel.classList.add('collapsed');
            } else {
                // 恢复 artifacts-panel 到 Console 打开之前的状态
                if (this.artifactsPanelWasOpen) {
                    artifactsPanel.classList.remove('collapsed');
                }
                this.artifactsPanelWasOpen = false;
            }
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

        await this.loadWorkspaceRestoreSnapshot();
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
            case 'runs':
                await this.loadRunsData(sessionId, force);
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
            case 'permissions':
                await this.loadPermissionData(sessionId, force);
                break;
            case 'diagnostics':
                await this.loadDiagnosticData(sessionId, force);
                break;
        }
    }

    private async loadOverviewData(sessionId: string, force = false) {
        if (!this.state.gatewayClient || this.isDisposed) return;

        const agentState = this.state.agentRuntimeState;
        const tokenState = this.state.getSessionResourceState(sessionId, 'tokenUsage');
        const runState = this.state.getSessionResourceState(sessionId, 'runs');
        if (!force && agentState.loaded && tokenState?.loaded && runState?.loaded) {
            this.renderOverview();
            return;
        }

        this.state.updateResourceState('agentRuntime', this.state.setLoadingResource(agentState));
        const currentTokenState = (tokenState as ResourceState<TokenUsageView> | undefined) ?? this.state.createEmptyResource();
        this.state.updateSessionResourceState(sessionId, 'tokenUsage', this.state.setLoadingResource(currentTokenState));
        const currentRunState = (runState as ResourceState<RunSummaryView[]> | undefined) ?? this.state.createEmptyResource();
        this.state.updateSessionResourceState(sessionId, 'runs', this.state.setLoadingResource(currentRunState));

        try {
            const [snapshot, usage, runResult] = await Promise.all([
                this.state.gatewayClient.getAgentInspect(),
                this.state.gatewayClient.getSessionTokenUsage(sessionId),
                this.state.gatewayClient.getSessionRuns(sessionId),
            ]);

            this.state.updateResourceState('agentRuntime', this.state.setLoadedResource(snapshot));
            this.state.updateSessionResourceState(sessionId, 'tokenUsage', this.state.setLoadedResource(usage));
            this.state.updateSessionResourceState(sessionId, 'runs', this.state.setLoadedResource(runResult.runs ?? []));
            this.ensureSelectedRun(sessionId);
        } catch (error) {
            this.state.updateResourceState('agentRuntime', this.state.toResourceError(error, t('common.load_failed')));
            this.state.updateSessionResourceState(sessionId, 'tokenUsage', this.state.toResourceError(error, t('common.load_failed')));
            this.state.updateSessionResourceState(sessionId, 'runs', this.state.toResourceError(error, t('common.load_failed')));
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
            this.state.updateResourceState('agentRuntime', this.state.toResourceError(error, t('common.load_failed')));
            this.state.updateSessionResourceState(sessionId, 'runtime', this.state.toResourceError(error, t('common.load_failed')));
            this.state.updateSessionResourceState(sessionId, 'tokenUsage', this.state.toResourceError(error, t('common.load_failed')));
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
            const mergedTools = tools.map(tool => ({ ...tool, ...this.state.getToolStatus(sessionId, tool.name) }));
            this.state.updateSessionResourceState(sessionId, 'tools', this.state.setLoadedResource(mergedTools));
        } catch (error) {
            this.state.updateSessionResourceState(sessionId, 'tools', this.state.toResourceError(error, t('common.load_failed')));
        }
    }

    private async loadSkillsData(sessionId: string, force = false) {
        if (!this.state.gatewayClient || this.isDisposed) return;

        await this.loadOverviewData(sessionId, force);

        const currentSkills = this.state.getSessionResourceState(sessionId, 'skills') as ResourceState<SkillBindingView[]> | undefined;
        if (!force && currentSkills?.loaded) {
            this.renderSkills();
            return;
        }

        this.state.updateSessionResourceState(
            sessionId,
            'skills',
            this.state.setLoadingResource(currentSkills ?? this.state.createEmptyResource())
        );

        try {
            const bindings = await this.state.gatewayClient.getSessionSkillBindings(sessionId);
            this.state.setSkillBindings(sessionId, bindings);
            this.state.updateSessionResourceState(sessionId, 'skills', this.state.setLoadedResource(bindings));
        } catch (error) {
            this.state.updateSessionResourceState(sessionId, 'skills', this.state.toResourceError(error, t('common.load_failed')));
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

        const promptResult = await this.state.gatewayClient.getSessionPromptPreview(sessionId)
            .then(promptPreview => this.state.setLoadedResource(promptPreview))
            .catch(error => this.state.toResourceError<PromptPreviewView>(error, t('common.load_failed')));
        this.state.updateSessionResourceState(sessionId, 'prompt', promptResult);

        const memoryResult = await this.state.gatewayClient.getSessionMemoryHits(sessionId)
            .then(memoryHits => this.state.setLoadedResource(memoryHits))
            .catch(async error => {
                if (error instanceof GatewayRequestError && error.kind === 'unsupported') {
                    const lastUserMessage = [...this.state.messages]
                        .reverse()
                        .find(message => message.role === 'user')
                        ?.content
                        ?.trim();

                    if (lastUserMessage) {
                        const searchResult = await this.state.gatewayClient?.memorySearch(lastUserMessage, 5);
                        const approximatedHits = (searchResult?.items ?? []).map((item: Record<string, unknown>) => ({
                            content: String(item.content ?? ''),
                            score: typeof item.score === 'number' ? item.score : 0,
                            source: String(item.source ?? 'memory.search'),
                            timestamp: typeof item.timestamp === 'number' ? item.timestamp : Date.now(),
                            reason: typeof item.reason === 'string' ? item.reason : undefined,
                        })) as MemoryHitView[];

                        return {
                            ...this.state.setLoadedResource(approximatedHits),
                            unsupported: true,
                        };
                    }
                }

                return this.state.toResourceError<MemoryHitView[]>(error, t('common.load_failed'));
            });
        this.state.updateSessionResourceState(sessionId, 'memory', memoryResult);
    }

    private async loadRunsData(sessionId: string, force = false) {
        if (!this.state.gatewayClient || this.isDisposed) return;

        const currentRuns = this.state.getSessionResourceState(sessionId, 'runs') as ResourceState<RunSummaryView[]> | undefined;
        const currentArtifacts = this.state.getSessionResourceState(sessionId, 'artifacts') as ResourceState<SessionArtifactView[]> | undefined;
        if (!force && currentRuns?.loaded && currentArtifacts?.loaded) {
            this.ensureSelectedRun(sessionId);
            this.renderRuns();
            return;
        }

        this.state.updateSessionResourceState(sessionId, 'runs', this.state.setLoadingResource(currentRuns ?? this.state.createEmptyResource()));
        this.state.updateSessionResourceState(sessionId, 'artifacts', this.state.setLoadingResource(currentArtifacts ?? this.state.createEmptyResource()));

        const runsResult = await this.state.gatewayClient.getSessionRuns(sessionId)
            .then(result => this.state.setLoadedResource(result.runs ?? []))
            .catch(error => this.state.toResourceError<RunSummaryView[]>(error, t('common.load_failed')));
        this.state.updateSessionResourceState(sessionId, 'runs', runsResult);

        const artifactsResult = await this.state.gatewayClient.getSessionArtifacts(sessionId)
            .then(artifacts => this.state.setLoadedResource(artifacts))
            .catch(error => this.state.toResourceError<SessionArtifactView[]>(error, t('common.load_failed')));
        this.state.updateSessionResourceState(sessionId, 'artifacts', artifactsResult);

        this.ensureSelectedRun(sessionId);
        const selectedRunId = this.state.selectedRunId;
        if (selectedRunId && runsResult.data?.some(run => run.id === selectedRunId)) {
            await this.loadRunDetail(sessionId, selectedRunId, force);
        }
        this.renderRuns();
    }

    private async loadRunDetail(sessionId: string, runId: string, force = false) {
        if (!this.state.gatewayClient || this.isDisposed) return;

        const currentDetail = this.state.getRunDetailState(sessionId, runId);
        if (!force && currentDetail?.loaded) {
            this.renderRuns();
            return;
        }

        this.state.updateRunDetailState(sessionId, runId, this.state.setLoadingResource(currentDetail ?? this.state.createEmptyResource()));
        const detailResult = await this.state.gatewayClient.getRunDetail(runId)
            .then(detail => this.state.setLoadedResource(detail))
            .catch(error => this.state.toResourceError<RunDetailView>(error, t('common.load_failed')));
        this.state.updateRunDetailState(sessionId, runId, detailResult);
        this.renderRuns();
    }

    private async loadPermissionData(sessionId: string, force = false) {
        if (!this.state.gatewayClient || this.isDisposed) return;

        const permissionState = this.state.getSessionResourceState(sessionId, 'permissions') as ResourceState<PermissionRequestView[]> | undefined;
        const auditState = this.state.getSessionResourceState(sessionId, 'audit') as ResourceState<AuditLogView[]> | undefined;
        if (!force && permissionState?.loaded && auditState?.loaded) {
            this.renderPermissions();
            return;
        }

        this.state.updateSessionResourceState(sessionId, 'permissions', this.state.setLoadingResource(permissionState ?? this.state.createEmptyResource()));
        this.state.updateSessionResourceState(sessionId, 'audit', this.state.setLoadingResource(auditState ?? this.state.createEmptyResource()));

        const permissionResult = await this.state.gatewayClient.getPendingPermissions(sessionId)
            .then(requests => this.state.setLoadedResource(requests))
            .catch(error => this.state.toResourceError<PermissionRequestView[]>(error, t('common.load_failed')));
        this.state.updateSessionResourceState(sessionId, 'permissions', permissionResult);

        const auditResult = await this.state.gatewayClient.getAuditLogs(sessionId)
            .then(result => this.state.setLoadedResource(result.logs ?? []))
            .catch(error => this.state.toResourceError<AuditLogView[]>(error, t('common.load_failed')));
        this.state.updateSessionResourceState(sessionId, 'audit', auditResult);
        this.renderPermissions();
    }

    private async loadDiagnosticData(sessionId: string, force = false) {
        if (!this.state.gatewayClient || this.isDisposed) return;

        const diagnosticState = this.state.getSessionResourceState(sessionId, 'diagnostics') as ResourceState<DiagnosticIssueView[]> | undefined;
        if (!force && diagnosticState?.loaded && this.state.workspaceRestoreState.loaded) {
            this.renderDiagnostics();
            return;
        }

        this.state.updateSessionResourceState(sessionId, 'diagnostics', this.state.setLoadingResource(diagnosticState ?? this.state.createEmptyResource()));
        if (force || !this.state.workspaceRestoreState.loaded) {
            this.state.workspaceRestoreState = this.state.setLoadingResource(this.state.workspaceRestoreState);
        }

        const diagnosticResult = await this.state.gatewayClient.getDiagnosticsCurrent(sessionId)
            .then(result => this.state.setLoadedResource(result.issues ?? []))
            .catch(error => this.state.toResourceError<DiagnosticIssueView[]>(error, t('common.load_failed')));
        this.state.updateSessionResourceState(sessionId, 'diagnostics', diagnosticResult);

        await this.loadWorkspaceRestoreSnapshot();

        this.renderDiagnostics();
    }

    private async loadWorkspaceRestoreSnapshot() {
        if (!this.state.gatewayClient || this.isDisposed) return;

        try {
            const restore = await this.state.gatewayClient.getWorkspaceRestore();
            this.applyWorkspaceRestore(restore);
        } catch (error) {
            if (error instanceof GatewayRequestError && error.kind === 'unsupported') {
                this.state.workspaceRestoreState = {
                    ...this.state.setLoadedResource({
                        sessionId: this.state.currentSessionId ?? undefined,
                        agentId: this.state.currentAgentId ?? undefined,
                        consoleVisible: this.state.consoleVisible,
                        activeTab: this.state.consoleActiveTab,
                        selectedRunId: this.state.selectedRunId ?? undefined,
                        selectedArtifactId: this.state.selectedArtifactId ?? undefined,
                        selectedPermissionRequestId: this.state.selectedPermissionRequestId ?? undefined,
                        selectedDiagnosticId: this.state.selectedDiagnosticId ?? undefined,
                        restorableRunState: 'none',
                        updatedAt: Date.now(),
                    } satisfies WorkspaceRestoreView),
                    unsupported: true,
                };
            } else {
                this.state.workspaceRestoreState = this.state.toResourceError<WorkspaceRestoreView>(error, t('common.load_failed'));
            }
        }
    }

    private renderCurrentTab() {
        switch (this.state.consoleActiveTab) {
            case 'overview':
                this.renderOverview();
                break;
            case 'runs':
                this.renderRuns();
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
            case 'permissions':
                this.renderPermissions();
                break;
            case 'diagnostics':
                this.renderDiagnostics();
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

        this.renderRecentRuns();
    }

    private renderStatusCard(state: ResourceState<AgentRuntimeSnapshot>): string {
        if (state.loading && !state.data) {
            return '<div class="skeleton-text"></div>';
        }

        if (state.error) {
            return `<div class="empty-hint">${escapeHtml(this.getResourceHint(state))}</div>`;
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
                modelSettings.innerHTML = `<div class="empty-hint">${escapeHtml(this.getResourceHint(this.state.agentRuntimeState))}</div>`;
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
                            ${hasSessionOverride(orchestrationDetail, executionDetail) ? `<button class="model-reset-btn" data-scope="session">${escapeHtml(t('console.restore_inherit'))}</button>` : ''}
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
            tokenUsage.innerHTML = `<div class="empty-hint">${escapeHtml(this.getResourceHint(usageState))}</div>`;
            return;
        }

        if (!usageData) {
            tokenUsage.innerHTML = `<div class="empty-hint">${escapeHtml(t('console.no_data'))}</div>`;
            return;
        }

        const rows = [
            renderTokenRow('Input', usageData.inputTokens),
            renderTokenRow('Output', usageData.outputTokens),
        ];

        if (usageData.cacheCreationInputTokens) {
            rows.push(renderTokenRow('Cache (Create)', usageData.cacheCreationInputTokens, 'cache-row'));
        }
        if (usageData.cacheReadInputTokens) {
            rows.push(renderTokenRow('Cache (Read)', usageData.cacheReadInputTokens, 'cache-row'));
        }
        if (usageData.totalCost !== undefined) {
            rows.push(renderTokenRow('Est. Cost', usageData.totalCost.toFixed(4), 'cost-row'));
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
            toolsList.innerHTML = `<div class="empty-hint">${escapeHtml(this.getResourceHint(toolsState))}</div>`;
            return;
        }

        const tools = (toolsState?.data ?? []).map(tool => ({ ...tool, ...this.state.getToolStatus(sessionId ?? '', tool.name) }));
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
        const sessionId = this.state.currentSessionId;
        const skillState = sessionId
            ? (this.state.getSessionResourceState(sessionId, 'skills') as ResourceState<SkillBindingView[]> | undefined)
            : undefined;
        const skillBindings = new Map((skillState?.data ?? []).map(binding => [binding.id, binding] as const));
        const agentSkills = this.state.agentRuntimeState.data?.skills ?? [];
        const activeSkills = this.state.agentRuntimeState.data?.activeSkills ?? [];

        if (skillState?.loading && !skillState.data) {
            skillsList.innerHTML = `<div class="empty-hint">${escapeHtml(t('common.loading'))}</div>`;
            return;
        }

        if (skillState?.error) {
            skillsList.innerHTML = `<div class="empty-hint">${escapeHtml(this.getResourceHint(skillState))}</div>`;
            return;
        }

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
                promptPreview.innerHTML = `<div class="empty-hint">${escapeHtml(this.getResourceHint(promptState))}</div>`;
            } else if (promptState?.data) {
                promptPreview.innerHTML = renderPromptSegments(promptState.data);
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
            memoryHits.innerHTML = `<div class="empty-hint">${escapeHtml(this.getResourceHint(memoryState))}</div>`;
            return;
        }

        const hits = memoryState?.data ?? [];
        if (hits.length === 0) {
            memoryHits.innerHTML = `<div class="empty-hint">${escapeHtml(t('console.no_data'))}</div>`;
            return;
        }

        // 近似数据警告：后端尚未实现精确命中记录，当前为 memory.search 近似结果
        const approximateWarning = memoryState?.unsupported
            ? `<div class="memory-approximate-warning">${escapeHtml(t('console.memory_approximate'))}</div>`
            : '';

        // 统计各类命中率
        const semanticHits = hits.filter(h => h.sourceType === 'semantic').length;
        const keywordHits = hits.filter(h => h.sourceType === 'keyword').length;
        const distillationHits = hits.filter(h => h.sourceType === 'distillation').length;
        const hitSummary = [semanticHits > 0 ? `${semanticHits} ${t('memory.semantic')}` : '', keywordHits > 0 ? `${keywordHits} ${t('memory.keyword')}` : '', distillationHits > 0 ? `${distillationHits} ${t('memory.distillation')}` : ''].filter(Boolean).join(', ');
        const summaryHtml = hitSummary ? `<div class="memory-hit-summary">${escapeHtml(hitSummary)}</div>` : '';

        memoryHits.innerHTML = `
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

    private renderRuns() {
        if (this.isDisposed) return;

        const sessionId = this.state.currentSessionId;
        const runState = sessionId ? this.state.getSessionResourceState(sessionId, 'runs') as ResourceState<RunSummaryView[]> | undefined : undefined;
        const artifactsState = sessionId ? this.state.getSessionResourceState(sessionId, 'artifacts') as ResourceState<SessionArtifactView[]> | undefined : undefined;
        const runsList = document.getElementById('console-runs-list');
        const runDetail = document.getElementById('console-run-detail');
        const currentRunCard = document.getElementById('console-current-run-card');
        const filterBar = document.getElementById('console-run-filters');
        if (!runsList || !runDetail || !currentRunCard || !filterBar) return;

        filterBar.innerHTML = renderRunFilters(this.activeRunFilter);
        filterBar.querySelectorAll<HTMLButtonElement>('.console-filter-chip').forEach(button => {
            button.onclick = () => {
                this.activeRunFilter = (button.dataset.filter as RunFilter) ?? 'all';
                this.renderRuns();
            };
        });

        if (runState?.loading && !runState.data) {
            runsList.innerHTML = `<div class="empty-hint">${escapeHtml(t('common.loading'))}</div>`;
            runDetail.innerHTML = `<div class="empty-hint">${escapeHtml(t('common.loading'))}</div>`;
            currentRunCard.innerHTML = '<div class="skeleton-text"></div>';
            return;
        }

        if (runState?.error) {
            const hint = this.getResourceHint(runState);
            runsList.innerHTML = `<div class="empty-hint">${escapeHtml(hint)}</div>`;
            runDetail.innerHTML = `<div class="empty-hint">${escapeHtml(hint)}</div>`;
            currentRunCard.innerHTML = `<div class="empty-hint">${escapeHtml(hint)}</div>`;
            return;
        }

        const allRuns = runState?.data ?? [];
        const filteredRuns = filterRunsByFilter(allRuns, this.activeRunFilter);
        const selectedRunId = this.state.selectedRunId;
        const selectedRun = filteredRuns.find(run => run.id === selectedRunId) ?? allRuns.find(run => run.id === selectedRunId) ?? filteredRuns[0];
        const detailState = sessionId && selectedRun ? this.state.getRunDetailState(sessionId, selectedRun.id) : undefined;
        const detail = detailState?.data;

        currentRunCard.innerHTML = selectedRun ? renderCurrentRunCard(selectedRun) : `<div class="empty-hint">${escapeHtml(t('console.no_runs'))}</div>`;
        runsList.innerHTML = filteredRuns.length > 0
            ? filteredRuns.map(run => renderRunListItem(run, run.id === selectedRun?.id)).join('')
            : `<div class="empty-hint">${escapeHtml(t('console.no_runs'))}</div>`;

        runsList.querySelectorAll<HTMLElement>('.run-item').forEach(item => {
            item.onclick = () => {
                const runId = item.dataset.runId;
                if (!runId || !sessionId) return;
                this.state.setConsoleSelection({ runId });
                this.persistLocalUiState();
                void this.loadRunDetail(sessionId, runId);
            };
        });

        currentRunCard.querySelectorAll<HTMLButtonElement>('[data-run-action]').forEach(button => {
            button.onclick = () => {
                if (!selectedRun) return;
                void this.handleRunAction(selectedRun, button.dataset.runAction as 'stop' | 'resume_waiting');
            };
        });

        if (!selectedRun) {
            runDetail.innerHTML = `<div class="empty-hint">${escapeHtml(t('console.select_run'))}</div>`;
            return;
        }

        if (detailState?.loading && !detail) {
            runDetail.innerHTML = `<div class="empty-hint">${escapeHtml(t('common.loading'))}</div>`;
            return;
        }

        if (detailState?.error) {
            runDetail.innerHTML = `<div class="empty-hint">${escapeHtml(this.getResourceHint(detailState))}</div>`;
            return;
        }

        runDetail.innerHTML = renderRunDetailCard(selectedRun, detail, artifactsState?.data ?? []);
        runDetail.querySelectorAll<HTMLButtonElement>('[data-artifact-action]').forEach(button => {
            button.onclick = () => {
                const artifactId = button.dataset.artifactId;
                const action = button.dataset.artifactAction;
                const artifact = detail?.artifacts?.find(item => item.id === artifactId) ?? artifactsState?.data?.find(item => item.id === artifactId);
                if (!artifact || !action) return;
                void this.handleArtifactAction(artifact, action);
            };
        });
        runDetail.querySelectorAll<HTMLButtonElement>('[data-nav-run]').forEach(button => {
            button.onclick = () => {
                const runId = button.dataset.navRun;
                if (!runId || !sessionId) return;
                this.state.setConsoleSelection({ runId });
                void this.loadRunDetail(sessionId, runId);
            };
        });
        runDetail.querySelectorAll<HTMLButtonElement>('[data-nav-permission]').forEach(button => {
            button.onclick = () => {
                const requestId = button.dataset.navPermission;
                if (!requestId) return;
                this.state.setConsoleSelection({ permissionRequestId: requestId });
                this.state.setConsoleTab('permissions');
            };
        });
    }

    private renderPermissions() {
        if (this.isDisposed) return;

        const sessionId = this.state.currentSessionId;
        const permissionState = sessionId ? this.state.getSessionResourceState(sessionId, 'permissions') as ResourceState<PermissionRequestView[]> | undefined : undefined;
        const auditState = sessionId ? this.state.getSessionResourceState(sessionId, 'audit') as ResourceState<AuditLogView[]> | undefined : undefined;
        const pendingRoot = document.getElementById('console-permission-pending');
        const auditRoot = document.getElementById('console-audit-list');
        if (!pendingRoot || !auditRoot) return;

        pendingRoot.innerHTML = renderPermissionList(permissionState, state => this.getResourceHint(state));
        auditRoot.innerHTML = renderAuditList(auditState, state => this.getResourceHint(state));

        pendingRoot.querySelectorAll<HTMLButtonElement>('[data-permission-decision]').forEach(button => {
            button.onclick = () => {
                const requestId = button.dataset.permissionId;
                const approved = button.dataset.permissionDecision === 'approve';
                if (!requestId) return;
                void this.handlePermissionDecision(requestId, approved);
            };
        });

        pendingRoot.querySelectorAll<HTMLButtonElement>('[data-permission-run]').forEach(button => {
            button.onclick = () => {
                const runId = button.dataset.permissionRun;
                if (!runId) return;
                this.state.setConsoleSelection({ runId });
                this.state.setConsoleTab('runs');
            };
        });
    }

    private renderDiagnostics() {
        if (this.isDisposed) return;

        const sessionId = this.state.currentSessionId;
        const diagnosticState = sessionId ? this.state.getSessionResourceState(sessionId, 'diagnostics') as ResourceState<DiagnosticIssueView[]> | undefined : undefined;
        const restoreRoot = document.getElementById('console-restore-card');
        const diagnosticRoot = document.getElementById('console-diagnostics-list');
        if (!restoreRoot || !diagnosticRoot) return;

        restoreRoot.innerHTML = this.renderRestoreCard(this.state.workspaceRestoreState);
        diagnosticRoot.innerHTML = renderDiagnosticList(diagnosticState, state => this.getResourceHint(state));

        restoreRoot.querySelectorAll<HTMLButtonElement>('[data-restore-action]').forEach(button => {
            button.onclick = () => {
                if (button.dataset.restoreAction === 'apply') {
                    const restore = this.state.workspaceRestoreState.data;
                    if (restore) {
                        this.applyWorkspaceRestore(restore, true);
                    }
                }
            };
        });

        diagnosticRoot.querySelectorAll<HTMLButtonElement>('[data-diagnostic-action]').forEach(button => {
            button.onclick = () => {
                const action = button.dataset.diagnosticAction;
                const diagnosticId = button.dataset.diagnosticId;
                const issue = diagnosticState?.data?.find(item => item.id === diagnosticId);
                if (!issue || !action) return;
                this.handleDiagnosticAction(issue, action);
            };
        });
    }

    private renderRestoreCard(state: ResourceState<WorkspaceRestoreView>): string {
        if (state.loading && !state.data) {
            return '<div class="skeleton-text"></div>';
        }
        if (state.error) {
            return `<div class="empty-hint">${escapeHtml(this.getResourceHint(state))}</div>`;
        }
        const restore = state.data;
        if (!restore) {
            return `<div class="empty-hint">${escapeHtml(t('console.no_restore'))}</div>`;
        }
        const restoreText = restore.restorableRunState === 'reattachable'
            ? t('console.restore_reattachable')
            : t('console.restore_view_only');
        return `
            <div class="console-detail-card">
                <div class="run-item-header">
                    <div>
                        <div class="run-title">${escapeHtml(restore.sessionId ?? '—')}</div>
                        <div class="run-subtitle">${escapeHtml(formatTime(restore.updatedAt))}</div>
                    </div>
                    <span class="restore-state-pill">${escapeHtml(restoreText)}</span>
                </div>
                <div class="card-item-meta">${escapeHtml(restore.activeTab ?? 'overview')}</div>
                <div class="detail-actions">
                    <button class="console-action-btn" data-restore-action="apply">${escapeHtml(t('console.action_restore_view'))}</button>
                </div>
            </div>
        `;
    }

    private async handleRunAction(run: RunSummaryView, action: 'stop' | 'resume_waiting') {
        if (!this.state.gatewayClient || !this.state.currentSessionId) return;

        try {
            await this.state.gatewayClient.controlRun(run.id, action);
            this.state.appendAuditLog(this.state.currentSessionId, {
                id: `audit-${Date.now()}`,
                sessionId: this.state.currentSessionId,
                runId: run.id,
                actionType: 'run_control',
                actor: 'user',
                result: 'completed',
                summary: `${action} ${run.title ?? run.id}`,
                createdAt: Date.now(),
            });
            await this.loadRunsData(this.state.currentSessionId, true);
        } catch (error) {
            this.bus.emit(Events.NOTIFICATION, { type: 'error', message: error instanceof Error ? error.message : t('common.failed') });
        }
    }

    private async handlePermissionDecision(requestId: string, approved: boolean) {
        if (!this.state.gatewayClient || !this.state.currentSessionId) return;

        const remember = window.confirm(`${t('console.permission_remember')}?`);
        try {
            await this.state.gatewayClient.respondPermission(requestId, approved, remember, remember ? 'session' : undefined);
            this.state.appendAuditLog(this.state.currentSessionId, {
                id: `audit-${Date.now()}`,
                sessionId: this.state.currentSessionId,
                permissionRequestId: requestId,
                actionType: 'permission',
                actor: 'user',
                result: approved ? 'approved' : 'denied',
                summary: `${approved ? t('console.permission_approved') : t('console.permission_denied')} ${requestId}`,
                createdAt: Date.now(),
            });
            await this.loadPermissionData(this.state.currentSessionId, true);
            await this.loadRunsData(this.state.currentSessionId, true);
        } catch (error) {
            this.bus.emit(Events.NOTIFICATION, { type: 'error', message: error instanceof Error ? error.message : t('common.failed') });
        }
    }

    private async handleArtifactAction(artifact: SessionArtifactView, action: string) {
        const sessionId = this.state.currentSessionId ?? undefined;
        this.state.setConsoleSelection({ artifactId: artifact.id });
        this.persistLocalUiState();

        try {
            switch (action) {
                case 'preview':
                    if (!artifact.path) {
                        throw new Error(t('console.artifact_missing'));
                    }
                    await invoke('file_open', { filePath: artifact.path });
                    break;
                case 'reveal':
                    if (!artifact.path) {
                        throw new Error(t('console.artifact_missing'));
                    }
                    await invoke('file_reveal', { filePath: artifact.path });
                    break;
                case 'copy-path':
                    if (artifact.path) {
                        await navigator.clipboard.writeText(artifact.path);
                    }
                    break;
                case 'copy-content':
                    if (artifact.content) {
                        await navigator.clipboard.writeText(artifact.content);
                    }
                    break;
                default:
                    break;
            }

            this.state.appendAuditLog(sessionId, {
                id: `audit-${Date.now()}`,
                sessionId,
                runId: artifact.runId,
                actionType: 'artifact_open',
                actor: 'user',
                result: 'completed',
                summary: `${action} ${artifact.filename ?? artifact.id}`,
                createdAt: Date.now(),
            });
        } catch (error) {
            const issue: DiagnosticIssueView = {
                id: `diag-artifact-${Date.now()}`,
                category: 'artifact',
                severity: 'error',
                title: t('console.audit_action_failed'),
                message: error instanceof Error ? error.message : t('console.artifact_missing'),
                suggestedActions: [t('console.action_go_run')],
                relatedRunId: artifact.runId,
                relatedSessionId: sessionId,
                updatedAt: Date.now(),
            };
            this.state.upsertDiagnostic(sessionId, issue);
            this.state.appendAuditLog(sessionId, {
                id: `audit-${Date.now()}`,
                sessionId,
                runId: artifact.runId,
                actionType: 'artifact_open',
                actor: 'user',
                result: 'failed',
                summary: `${action} ${artifact.filename ?? artifact.id}`,
                createdAt: Date.now(),
            });
        }
    }

    private handleDiagnosticAction(issue: DiagnosticIssueView, action: string) {
        switch (action) {
            case 'run':
                if (issue.relatedRunId) {
                    this.state.setConsoleSelection({ runId: issue.relatedRunId, diagnosticId: issue.id });
                    this.state.setConsoleTab('runs');
                }
                break;
            case 'permission':
                if (issue.relatedPermissionRequestId) {
                    this.state.setConsoleSelection({ permissionRequestId: issue.relatedPermissionRequestId, diagnosticId: issue.id });
                    this.state.setConsoleTab('permissions');
                }
                break;
            case 'settings':
                this.navigateTo({ settingsSection: issue.category === 'memory' ? 'memory' : 'mcp' });
                break;
            case 'retry':
                if (issue.relatedRunId && this.state.currentSessionId) {
                    void this.loadRunDetail(this.state.currentSessionId, issue.relatedRunId, true);
                }
                break;
            default:
                break;
        }
    }

    private ensureSelectedRun(sessionId: string) {
        const runs = (this.state.getSessionResourceState(sessionId, 'runs') as ResourceState<RunSummaryView[]> | undefined)?.data ?? [];
        if (runs.length === 0) {
            this.state.setConsoleSelection({ runId: null });
            return;
        }

        if (!this.state.selectedRunId || !runs.some(run => run.id === this.state.selectedRunId)) {
            this.state.setConsoleSelection({ runId: runs[0].id });
        }
    }

    private persistLocalUiState() {
        const payload: WorkspaceRestoreView = {
            sessionId: this.state.currentSessionId ?? undefined,
            agentId: this.state.currentAgentId ?? undefined,
            consoleVisible: this.state.consoleVisible,
            activeTab: this.state.consoleActiveTab,
            selectedRunId: this.state.selectedRunId ?? undefined,
            selectedArtifactId: this.state.selectedArtifactId ?? undefined,
            selectedPermissionRequestId: this.state.selectedPermissionRequestId ?? undefined,
            selectedDiagnosticId: this.state.selectedDiagnosticId ?? undefined,
            restorableRunState: this.state.workspaceRestoreState.data?.restorableRunState ?? 'none',
            updatedAt: Date.now(),
        };
        localStorage.setItem(AgentConsoleView.WORKSPACE_RESTORE_KEY, JSON.stringify(payload));
    }

    private restoreLocalUiState() {
        const raw = localStorage.getItem(AgentConsoleView.WORKSPACE_RESTORE_KEY);
        if (!raw) return;
        try {
            const parsed = JSON.parse(raw) as Partial<WorkspaceRestoreView>;
            if (parsed.activeTab) {
                this.state.consoleActiveTab = parsed.activeTab;
            }
            this.state.consoleVisible = Boolean(parsed.consoleVisible);
            this.state.selectedRunId = parsed.selectedRunId ?? null;
            this.state.selectedArtifactId = parsed.selectedArtifactId ?? null;
            this.state.selectedPermissionRequestId = parsed.selectedPermissionRequestId ?? null;
            this.state.selectedDiagnosticId = parsed.selectedDiagnosticId ?? null;
            if (this.state.consoleVisible) {
                this.container?.classList.remove('hidden');
                this.syncLayout(true);
            }
        } catch {
            localStorage.removeItem(AgentConsoleView.WORKSPACE_RESTORE_KEY);
        }
    }

    private applyWorkspaceRestore(restore: WorkspaceRestoreView, fromUserAction = false) {
        this.state.setWorkspaceRestore(restore);

        if (restore.sessionId && this.state.sessions.some(session => session.id === restore.sessionId)) {
            this.state.setCurrentSession(restore.sessionId);
        } else if (restore.sessionId && fromUserAction) {
            this.bus.emit(Events.NOTIFICATION, { type: 'error', message: t('console.restore_missing_session') });
        }

        if (restore.activeTab) {
            this.state.setConsoleTab(restore.activeTab);
        }
        this.state.setConsoleSelection({
            runId: restore.selectedRunId ?? null,
            artifactId: restore.selectedArtifactId ?? null,
            permissionRequestId: restore.selectedPermissionRequestId ?? null,
            diagnosticId: restore.selectedDiagnosticId ?? null,
        });

        if (fromUserAction) {
            this.state.setConsoleVisible(true);
            this.updateTabUI(this.state.consoleActiveTab);
            void this.loadTabDataIfNeeded(this.state.consoleActiveTab, true);
            this.bus.emit(Events.NOTIFICATION, {
                type: 'info',
                message: this.state.workspaceRestoreState.unsupported ? t('console.restore_unsupported') : t('console.restore_applied'),
            });
        }

        this.persistLocalUiState();
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

    /**
     * 跨标签/跨视图统一跳转方法 (Plan 3)
     */
    navigateTo(target: NavigateTarget): void {
        if (target.settingsSection) {
            const payload: SettingsNavigatePayload = {
                visible: true,
                section: target.settingsSection,
                search: target.settingsSearch,
                itemId: target.itemId,
            };
            this.bus.emit(Events.SETTINGS_NAVIGATE, payload);
            return;
        }

        if (target.tab) {
            this.state.setConsoleTab(target.tab);

            // 延迟定位到具体条目（等待 DOM 更���）
            if (target.itemId) {
                requestAnimationFrame(() => {
                    const el = this.container?.querySelector(`[data-item-id="${target.itemId}"]`);
                    if (el) {
                        el.scrollIntoView({ behavior: 'smooth', block: 'center' });
                        el.classList.add('highlight-flash');
                        setTimeout(() => el.classList.remove('highlight-flash'), 2000);
                    }
                });
            }
        }
    }

    /**
     * 渲染概览中的近期任务列表
     */
    private renderRecentRuns() {
        const sessionId = this.state.currentSessionId;
        const runList = document.getElementById('console-recent-run-list');
        if (!runList) return;

        const runState = sessionId ? this.state.getSessionResourceState(sessionId, 'runs') as ResourceState<RunSummaryView[]> | undefined : undefined;
        if (runState?.loading && !runState.data) {
            runList.innerHTML = `<div class="empty-hint">${escapeHtml(t('common.loading'))}</div>`;
            return;
        }
        if (runState?.error) {
            runList.innerHTML = `<div class="empty-hint">${escapeHtml(this.getResourceHint(runState))}</div>`;
            return;
        }

        const runs = (runState?.data ?? []).slice(0, 3);
        if (runs.length === 0) {
            runList.innerHTML = `<div class="empty-hint">${escapeHtml(t('console.no_runs'))}</div>`;
            return;
        }

        runList.innerHTML = runs.map(run => renderRunListItem(run, run.id === this.state.selectedRunId)).join('');
        runList.querySelectorAll<HTMLElement>('.run-item').forEach(item => {
            item.onclick = () => {
                const runId = item.dataset.runId;
                if (!runId || !sessionId) return;
                this.state.setConsoleSelection({ runId });
                this.state.setConsoleTab('runs');
            };
        });
    }

    private getResourceHint<T>(state: ResourceState<T>): string {
        if (state.unsupported) {
            return t('console.backend_upgrade_required');
        }
        return state.error ?? t('common.load_failed');
    }
}
