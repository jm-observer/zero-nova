import { EventBus, Events } from './event-bus';
import {
    Session,
    Message,
    AgentModelItem,
    PendingAttachment,
    McpServerView,
    WorkingMode,
    ResourceState,
    AgentRuntimeSnapshot,
    SessionRuntimeSnapshot,
    PromptPreviewView,
    ToolDescriptorView,
    MemoryHitView,
    TokenUsageView,
    ModelBindingView,
    SkillBindingView,
    ToolUnlockedEvent,
    SkillActivatedEvent,
    SkillExitedEvent,
    ConsoleTab,
    RunSummaryView,
    RunDetailView,
    SessionArtifactView,
    PermissionRequestView,
    AuditLogView,
    DiagnosticIssueView,
    WorkspaceRestoreView,
} from './types';
import { GatewayClient, GatewayRequestError } from '../gateway-client';

/**
 * 前端 token 累加更新事件
 */
interface ChatTokenUsageUpdate {
    sessionId: string;
    usage: TokenUsageView;
}

export type VoiceConversationPhase =
    | 'idle'
    | 'requesting_permission'
    | 'recording'
    | 'uploading_audio'
    | 'recognizing'
    | 'submitting_text'
    | 'waiting_assistant'
    | 'speaking'
    | 'interrupted'
    | 'error';

export interface VoiceConversationState {
    active: boolean;
    phase: VoiceConversationPhase;
    transcript: string;
    transcriptState: 'idle' | 'pending' | 'final';
    error: string | null;
    durationSeconds: number;
    canRetry: boolean;
}

/**
 * 全局状态管理类
 */
export class AppState {
    private bus: EventBus;
    
    // 会话相关
    currentSessionId: string | null = null;
    sessions: Session[] = [];
    loadingSessions = new Set<string>(); // 正在加载的会话（支持多会话并发）
    chatTargetSessionIds = new Set<string>(); // 正在进行中的聊天会话集（用于进度事件隔离）
    unreadSessionIds = new Set<string>(); // 有未读消息的会话
    sessionToChatroomMap = new Map<string, number>(); // sessionId → chatroomId 映射
    sessionDrafts = new Map<string, string>(); // 按会话保存输入框草稿
    messages: Message[] = []; // 当前选中的会话的消息
    
    // Agent 相关
    currentAgentId: string | null = null;
    agentsList: AgentModelItem[] = [];
    
    // 附件相关
    pendingAttachments: PendingAttachment[] = [];
    
    // MCP 相关
    mcpServers: McpServerView[] = [];
    
    // 语音相关
    voiceStatus: { 
        stt: { enabled: boolean; available: boolean }; 
        tts: { enabled: boolean; available: boolean; voice: string; autoPlay: boolean } 
    } | null = null;
    ttsAutoPlay = false;
    voiceModeActive = false;
    voiceConversation: VoiceConversationState = {
        active: false,
        phase: 'idle',
        transcript: '',
        transcriptState: 'idle',
        error: null,
        durationSeconds: 0,
        canRetry: false,
    };
    
    // 工作模式
    currentWorkingMode: WorkingMode = 'standalone';

    // 基础设施
    gatewayClient: GatewayClient | null = null;

    // --- Agent Console 状态 ---
    consoleVisible = false;
    consoleActiveTab: ConsoleTab = 'overview';
    selectedRunId: string | null = null;
    selectedArtifactId: string | null = null;
    selectedPermissionRequestId: string | null = null;
    selectedDiagnosticId: string | null = null;

    agentRuntimeState: ResourceState<AgentRuntimeSnapshot> = this.createEmptyResource();
    sessionRuntimeStates = new Map<string, ResourceState<SessionRuntimeSnapshot>>();
    sessionPromptStates = new Map<string, ResourceState<PromptPreviewView>>();
    sessionToolStates = new Map<string, ResourceState<ToolDescriptorView[]>>();
    sessionMemoryHitStates = new Map<string, ResourceState<MemoryHitView[]>>();
    sessionTokenUsageStates = new Map<string, ResourceState<TokenUsageView>>();
    sessionSkillBindingStates = new Map<string, ResourceState<SkillBindingView[]>>();
    sessionRunStates = new Map<string, ResourceState<RunSummaryView[]>>();
    sessionArtifactStates = new Map<string, ResourceState<SessionArtifactView[]>>();
    sessionPermissionStates = new Map<string, ResourceState<PermissionRequestView[]>>();
    sessionAuditStates = new Map<string, ResourceState<AuditLogView[]>>();
    sessionDiagnosticStates = new Map<string, ResourceState<DiagnosticIssueView[]>>();
    sessionRunDetailStates = new Map<string, Map<string, ResourceState<RunDetailView>>>();
    workspaceRestoreState: ResourceState<WorkspaceRestoreView> = this.createEmptyResource();

    // --- 前端 Token 累加 handlers (Plan 2) ---
    private tokenUsageHandlerRef: ((msg: import('../gateway-client').GatewayMessage) => void) | null = null;
    /** 连续累加计数，每 3 次触发一次校正拉取 */
    private tokenAccumulationCount = new Map<string, number>();

    // --- 模型绑定缓存 (Plan 2) ---
    modelBindingCache: Record<string, ModelBindingView[]> = {};

    // --- Skill 运行态绑定缓存 (Plan 3) ---
    // --- Tool 运行时状态缓存 (Plan 3) ---
    private toolStatusCache = new Map<string, Map<string, {
        lastCallStatus?: 'success' | 'error' | 'running';
        lastUsedAt?: number;
        unlockedBy?: string;
        unlockedReason?: string;
    }>>();
    private toolUnlockedHandlerRef: ((event: ToolUnlockedEvent) => void) | null = null;
    private skillActivatedHandlerRef: ((event: SkillActivatedEvent) => void) | null = null;
    private skillExitedHandlerRef: ((event: SkillExitedEvent) => void) | null = null;

    constructor(bus: EventBus) {
        this.bus = bus;
    }

    /**
     * 初始化前端 token 累加与模型绑定事件监听
     * 需要在 GatewayClient 连接后调用
     */
    initTokenTracking(): void {
        // 停止之前的监听器
        if (this.gatewayClient && this.tokenUsageHandlerRef) {
            this.gatewayClient.removeMessageHandler(this.tokenUsageHandlerRef);
            this.tokenUsageHandlerRef = null;
        }

        if (!this.gatewayClient) return;

        // 监听 GatewayClient 发出的 chat.token_usage 事件
        this.tokenUsageHandlerRef = (msg) => {
            if (msg.type === 'chat.token_usage') {
                const payload = msg.payload as ChatTokenUsageUpdate;
                this.handleTokenUsageUpdate(payload);
            }
        };
        this.gatewayClient.addMessageHandler(this.tokenUsageHandlerRef);
    }

    /**
     * 处理前端 token 累加更新
     */
    private handleTokenUsageUpdate(update: ChatTokenUsageUpdate): void {
        const { sessionId, usage } = update;
        const current = this.sessionTokenUsageStates.get(sessionId) || this.createEmptyResource<TokenUsageView>();
        const prev = current.data || { inputTokens: 0, outputTokens: 0, cacheCreationInputTokens: 0, cacheReadInputTokens: 0 };

        // 累加 token 值
        const merged: TokenUsageView = {
            inputTokens: prev.inputTokens + (usage.inputTokens ?? 0),
            outputTokens: prev.outputTokens + (usage.outputTokens ?? 0),
            cacheCreationInputTokens: (prev.cacheCreationInputTokens ?? 0) + (usage.cacheCreationInputTokens ?? 0),
            cacheReadInputTokens: (prev.cacheReadInputTokens ?? 0) + (usage.cacheReadInputTokens ?? 0),
        };

        this.sessionTokenUsageStates.set(sessionId, this.setLoadedResource(merged));
        this.bus.emit(Events.CONSOLE_TOKEN_UPDATED, { sessionId, data: merged });
        this.bus.emit(Events.CONSOLE_DATA_UPDATED, { sessionId, data: merged });

        // 累加误差校正：每 3 轮累加后触发一次后端校正
        const accumulationCount = (this.tokenAccumulationCount.get(sessionId) ?? 0) + 1;
        this.tokenAccumulationCount.set(sessionId, accumulationCount);
        if (accumulationCount >= 3 && this.gatewayClient) {
            this.tokenAccumulationCount.set(sessionId, 0);
            this.gatewayClient.getSessionTokenUsage(sessionId)
                .then(corrected => {
                    this.sessionTokenUsageStates.set(sessionId, this.setLoadedResource(corrected));
                    this.bus.emit(Events.CONSOLE_TOKEN_UPDATED, { sessionId, data: corrected });
                    this.bus.emit(Events.CONSOLE_DATA_UPDATED, { sessionId, data: corrected });
                })
                .catch(() => { /* 校正失败时保留当前累加值 */ });
        }
    }

    /**
     * 设置模型绑定缓存
     */
    setModelBinding(provider: string, scope: string, binding: ModelBindingView): void {
        if (!this.modelBindingCache[provider]) {
            this.modelBindingCache[provider] = [];
        }
        const existing = this.modelBindingCache[provider].find(b => b.source === scope);
        if (existing) {
            Object.assign(existing, binding);
        } else {
            this.modelBindingCache[provider].push(binding);
        }
    }

    /**
     * 获取模型绑定缓存
     */
    getModelBinding(provider: string, scope: 'global' | 'agent' | 'session_override'): ModelBindingView | undefined {
        return this.modelBindingCache[provider]?.find(b => b.source === scope);
    }

    // --- Skill Binding 操作 (Plan 3) ---

    /**
     * 设置技能绑定状态
     */
    setSkillBindings(sessionId: string, bindings: SkillBindingView[]): void {
        this.sessionSkillBindingStates.set(sessionId, this.setLoadedResource(bindings));
        this.bus.emit(Events.CONSOLE_SKILLS_UPDATED, { sessionId, data: bindings });
        this.bus.emit(Events.CONSOLE_DATA_UPDATED, { sessionId, data: { type: 'skill_bindings', bindings } });
    }

    /**
     * 获取技能绑定状态
     */
    getSkillBindings(sessionId: string): SkillBindingView[] {
        return this.sessionSkillBindingStates.get(sessionId)?.data ?? [];
    }

    /**
     * 处理技能激活事件
     */
    handleSkillActivated(event: SkillActivatedEvent): void {
        if (!event.sessionId) {
            return;
        }
        const binding: SkillBindingView = {
            id: event.skillId,
            title: event.title ?? event.skillId,
            source: event.source || 'runtime',
            enabled: true,
            contentPreview: event.content?.substring(0, 200),
            activatedAt: event.timestamp,
            sticky: event.sticky || false,
        };
        const currentBindings = this.getSkillBindings(event.sessionId).filter(item => item.id !== event.skillId);
        this.setSkillBindings(event.sessionId, [...currentBindings, binding]);
        // 同时更新 agentRuntime 的 activeSkills
        if (event.sessionId === this.currentSessionId && this.agentRuntimeState.data) {
            const skills = this.agentRuntimeState.data.activeSkills || [];
            if (!skills.includes(event.skillId)) {
                this.agentRuntimeState.data.activeSkills = [...skills, event.skillId];
            }
        }
    }

    /**
     * 处理技能退出事件
     */
    handleSkillExited(event: SkillExitedEvent): void {
        if (!event.sessionId) {
            return;
        }
        const nextBindings = this.getSkillBindings(event.sessionId)
            .map(binding => binding.id === event.skillId ? { ...binding, enabled: false } : binding)
            .filter(binding => event.sticky || binding.id !== event.skillId);
        this.setSkillBindings(event.sessionId, nextBindings);
        // 从 agentRuntime 中移除
        if (event.sessionId === this.currentSessionId && this.agentRuntimeState.data) {
            const skills = this.agentRuntimeState.data.activeSkills || [];
            this.agentRuntimeState.data.activeSkills = skills.filter(id => id !== event.skillId);
        }
    }

    // --- Tool Status 操作 (Plan 3) ---

    /**
     * 更新工具运行时状态
     */
    updateToolStatus(sessionId: string, toolName: string, status: { lastCallStatus?: 'success' | 'error' | 'running'; lastUsedAt?: number; unlockedBy?: string; unlockedReason?: string }): void {
        const sessionCache = this.toolStatusCache.get(sessionId) ?? new Map<string, typeof status>();
        const current = sessionCache.get(toolName) || {};
        sessionCache.set(toolName, { ...current, ...status });
        this.toolStatusCache.set(sessionId, sessionCache);
        this.bus.emit(Events.CONSOLE_TOOLS_UPDATED, { sessionId, data: { type: 'tool_status', toolName, ...status } });
        this.bus.emit(Events.CONSOLE_DATA_UPDATED, { sessionId, data: { type: 'tool_status', toolName, ...status } });
    }

    /**
     * 获取工具运行时状态
     */
    getToolStatus(sessionId: string, toolName: string): { lastCallStatus?: 'success' | 'error' | 'running'; lastUsedAt?: number; unlockedBy?: string; unlockedReason?: string } | undefined {
        return this.toolStatusCache.get(sessionId)?.get(toolName);
    }

    /**
     * 清空工具运行时状态缓存（会话切换时调用）
     */
    clearToolStatusCache(sessionId?: string): void {
        if (sessionId) {
            this.toolStatusCache.delete(sessionId);
            return;
        }
        this.toolStatusCache.clear();
    }

    setWorkspaceRestore(restore: WorkspaceRestoreView): void {
        this.workspaceRestoreState = this.setLoadedResource(restore);
        this.bus.emit(Events.CONSOLE_DATA_UPDATED, { sessionId: restore.sessionId ?? null, data: restore });
    }

    getRunDetailState(sessionId: string, runId: string): ResourceState<RunDetailView> | undefined {
        return this.sessionRunDetailStates.get(sessionId)?.get(runId);
    }

    updateRunDetailState(sessionId: string, runId: string, update: Partial<ResourceState<RunDetailView>>): void {
        const sessionMap = this.sessionRunDetailStates.get(sessionId) ?? new Map<string, ResourceState<RunDetailView>>();
        const current = sessionMap.get(runId) ?? this.createEmptyResource<RunDetailView>();
        if (typeof update.updatedAt === 'number' && typeof current.updatedAt === 'number' && update.updatedAt < current.updatedAt) {
            return;
        }
        sessionMap.set(runId, { ...current, ...update });
        this.sessionRunDetailStates.set(sessionId, sessionMap);

        if (this.currentSessionId === sessionId) {
            this.bus.emit(Events.CONSOLE_DATA_UPDATED, { sessionId, data: sessionMap.get(runId) });
        }
    }

    appendAuditLog(sessionId: string | undefined, log: AuditLogView): void {
        if (!sessionId) {
            return;
        }
        const current = this.sessionAuditStates.get(sessionId)?.data ?? [];
        const next = [log, ...current.filter(item => item.id !== log.id)];
        this.sessionAuditStates.set(sessionId, this.setLoadedResource(next));
        if (this.currentSessionId === sessionId) {
            this.bus.emit(Events.CONSOLE_DATA_UPDATED, { sessionId, data: next });
        }
    }

    upsertDiagnostic(sessionId: string | undefined, issue: DiagnosticIssueView): void {
        if (!sessionId) {
            return;
        }
        const current = this.sessionDiagnosticStates.get(sessionId)?.data ?? [];
        const next = [issue, ...current.filter(item => item.id !== issue.id)].sort((left, right) => right.updatedAt - left.updatedAt);
        this.sessionDiagnosticStates.set(sessionId, this.setLoadedResource(next));
        if (this.currentSessionId === sessionId) {
            this.bus.emit(Events.CONSOLE_DATA_UPDATED, { sessionId, data: next });
        }
    }

    /**
     * 初始化 Skill/Tool 事件监听器 (Plan 3)
     */
    initSkillToolTracking(): void {
        if (!this.gatewayClient) return;

        // 监听 ToolUnlocked 事件
        this.toolUnlockedHandlerRef = (event: ToolUnlockedEvent) => {
            if (!event.toolName) {
                return;
            }

            const sessionId = event.sessionId ?? this.currentSessionId;
            if (!sessionId) {
                return;
            }

            this.updateToolStatus(sessionId, event.toolName, {
                lastUsedAt: event.timestamp,
                unlockedBy: event.source,
                unlockedReason: event.reason,
            });
            this.bus.emit(Events.NOTIFICATION, { type: 'info', message: `工具解锁: ${event.toolName}` });
        };
        this.gatewayClient.onToolUnlocked(this.toolUnlockedHandlerRef);

        // 监听 SkillActivated 事件
        this.skillActivatedHandlerRef = (event: SkillActivatedEvent) => {
            this.handleSkillActivated(event);
        };
        this.gatewayClient.onSkillActivated(this.skillActivatedHandlerRef);

        // 监听 SkillExited 事件
        this.skillExitedHandlerRef = (event: SkillExitedEvent) => {
            this.handleSkillExited(event);
        };
        this.gatewayClient.onSkillExited(this.skillExitedHandlerRef);
    }

    // --- 状态操作方法 ---

    setGatewayClient(client: GatewayClient) {
        this.gatewayClient = client;
    }

    setCurrentSession(id: string | null) {
        if (this.currentSessionId !== id) {
            const previousSessionId = this.currentSessionId;
            this.currentSessionId = id;

            // 切换会话时处理消息清理
            // 如果是从 null (初始状态) 切换到某个会话，保留当前的乐观消息
            // 只有在不同会话之间切换，或者关闭会话时，才真正清空消息列表
            if (previousSessionId !== null) {
                this.messages = [];
            }
            
            this.bus.emit(Events.SESSION_SELECTED, { sessionId: id });
            this.bus.emit(Events.SESSION_CHANGED, { 
                sessionId: id, 
                previousSessionId,
                messages: this.messages 
            });
        }
    }

    setMessages(messages: Message[]) {
        this.messages = messages;
        this.bus.emit(Events.MESSAGES_UPDATED, { sessionId: this.currentSessionId, messages });
    }

    addMessage(message: Message) {
        this.messages.push(message);
        this.bus.emit(Events.MESSAGE_ADDED, { sessionId: this.currentSessionId, message });
    }

    setSessions(sessions: Session[]) {
        this.sessions = sessions;
        this.bus.emit(Events.SESSION_UPDATED, { sessions });
    }

    addSession(session: Session) {
        this.sessions.unshift(session);
        this.bus.emit(Events.SESSION_CREATED, { session });
    }

    deleteSession(sessionId: string) {
        this.sessions = this.sessions.filter(s => s.id !== sessionId);
        if (this.currentSessionId === sessionId) {
            this.setCurrentSession(this.sessions[0]?.id || null);
        }
        this.bus.emit(Events.SESSION_DELETED, { sessionId });
    }

    setLoading(sessionId: string, isLoading: boolean) {
        if (isLoading) {
            this.loadingSessions.add(sessionId);
        } else {
            this.loadingSessions.delete(sessionId);
        }
        if (this.currentSessionId === sessionId) {
            this.bus.emit('session:loading_changed', { sessionId, isLoading });
        }
    }

    setAgents(agents: AgentModelItem[]) {
        this.agentsList = agents;
        // 如果当前没有选中 Agent，默认选中第一个或者标记为 default 的
        if (!this.currentAgentId && agents.length > 0) {
            const defaultAgent = agents.find(a => a.default) || agents[0];
            this.setCurrentAgent(defaultAgent.id);
        }
        this.bus.emit('agents:updated', { agents });
    }

    setCurrentAgent(agentId: string | null) {
        if (this.currentAgentId !== agentId) {
            this.currentAgentId = agentId;
            this.bus.emit(Events.AGENT_SWITCHED, { agentId });
        }
    }

    setVoiceMode(active: boolean) {
        if (this.voiceModeActive !== active) {
            this.voiceModeActive = active;
            this.bus.emit(Events.VOICE_MODE_TOGGLE, { active });
        }
    }

    setVoiceCapabilities(status: AppState['voiceStatus']) {
        this.voiceStatus = status;
        this.ttsAutoPlay = status?.tts.autoPlay ?? false;
        this.bus.emit(Events.VOICE_CAPABILITIES_UPDATED, {
            status,
            ttsAutoPlay: this.ttsAutoPlay,
        });
    }

    updateVoiceConversation(patch: Partial<VoiceConversationState>) {
        this.voiceConversation = {
            ...this.voiceConversation,
            ...patch,
        };

        this.bus.emit(Events.VOICE_STATE_UPDATED, {
            ...this.voiceConversation,
        });
    }

    resetVoiceConversation() {
        this.updateVoiceConversation({
            active: false,
            phase: 'idle',
            transcript: '',
            transcriptState: 'idle',
            error: null,
            durationSeconds: 0,
            canRetry: false,
        });
    }

    upsertMessage(message: Message) {
        const index = this.messages.findIndex(item => item.id === message.id);
        if (index >= 0) {
            this.messages[index] = message;
            this.bus.emit(Events.MESSAGES_UPDATED, { sessionId: this.currentSessionId, messages: [...this.messages] });
            return;
        }

        this.addMessage(message);
    }

    removeMessageById(messageId: string) {
        const nextMessages = this.messages.filter(message => message.id !== messageId);
        if (nextMessages.length === this.messages.length) {
            return;
        }

        this.messages = nextMessages;
        this.bus.emit(Events.MESSAGES_UPDATED, { sessionId: this.currentSessionId, messages: [...this.messages] });
    }

    upsertVoiceTranscriptMessage(messageId: string, text: string, transcriptState: 'pending' | 'final') {
        const content = text || (transcriptState === 'pending' ? '...' : '');

        this.upsertMessage({
            id: messageId,
            role: 'user',
            content,
            createdAt: Date.now(),
            metadata: {
                voiceTranscriptState: transcriptState,
            },
        });
    }

    addPendingAttachment(attachment: PendingAttachment) {
        this.pendingAttachments.push(attachment);
        this.bus.emit('attachments:updated', { attachments: this.pendingAttachments });
    }

    removePendingAttachment(path: string) {
        this.pendingAttachments = this.pendingAttachments.filter(a => a.path !== path);
        this.bus.emit('attachments:updated', { attachments: this.pendingAttachments });
    }

    clearPendingAttachments() {
        this.pendingAttachments = [];
        this.bus.emit('attachments:updated', { attachments: [] });
    }

    setMcpServers(servers: McpServerView[]) {
        this.mcpServers = servers;
        this.bus.emit('mcp:updated', { servers });
    }

    // --- Agent Console 操作 ---

    setConsoleVisible(visible: boolean) {
        if (this.consoleVisible !== visible) {
            this.consoleVisible = visible;
            this.bus.emit(Events.CONSOLE_TOGGLED, { visible });
        }
    }

    setConsoleTab(tab: ConsoleTab) {
        if (this.consoleActiveTab !== tab) {
            this.consoleActiveTab = tab;
            this.bus.emit(Events.CONSOLE_TAB_CHANGED, { tab });
        }
    }

    setConsoleSelection(selection: {
        runId?: string | null;
        artifactId?: string | null;
        permissionRequestId?: string | null;
        diagnosticId?: string | null;
    }): void {
        if (selection.runId !== undefined) {
            this.selectedRunId = selection.runId;
        }
        if (selection.artifactId !== undefined) {
            this.selectedArtifactId = selection.artifactId;
        }
        if (selection.permissionRequestId !== undefined) {
            this.selectedPermissionRequestId = selection.permissionRequestId;
        }
        if (selection.diagnosticId !== undefined) {
            this.selectedDiagnosticId = selection.diagnosticId;
        }
        this.bus.emit(Events.CONSOLE_DATA_UPDATED, { sessionId: this.currentSessionId, data: selection });
    }

    /**
     * 更新全局 Agent 资源状态 (非会话绑定)
     */
    updateResourceState<K extends 'agentRuntime'>(
        key: K, 
        update: Partial<ResourceState<K extends 'agentRuntime' ? AgentRuntimeSnapshot : any>>
    ) {
        if (key === 'agentRuntime') {
            const current = this.agentRuntimeState;
            const next = { ...current, ...update };
            this.agentRuntimeState = next as any;
            this.bus.emit(Events.CONSOLE_DATA_UPDATED, { sessionId: null, data: next });
        }
    }

    getSessionResourceState(
        sessionId: string,
        key: 'runtime' | 'prompt' | 'tools' | 'memory' | 'tokenUsage' | 'skills' | 'runs' | 'artifacts' | 'permissions' | 'audit' | 'diagnostics'
    ): ResourceState<unknown> | undefined {
        switch (key) {
            case 'runtime':
                return this.sessionRuntimeStates.get(sessionId);
            case 'prompt':
                return this.sessionPromptStates.get(sessionId);
            case 'tools':
                return this.sessionToolStates.get(sessionId);
            case 'memory':
                return this.sessionMemoryHitStates.get(sessionId);
            case 'tokenUsage':
                return this.sessionTokenUsageStates.get(sessionId);
            case 'skills':
                return this.sessionSkillBindingStates.get(sessionId);
            case 'runs':
                return this.sessionRunStates.get(sessionId);
            case 'artifacts':
                return this.sessionArtifactStates.get(sessionId);
            case 'permissions':
                return this.sessionPermissionStates.get(sessionId);
            case 'audit':
                return this.sessionAuditStates.get(sessionId);
            case 'diagnostics':
                return this.sessionDiagnosticStates.get(sessionId);
            default:
                return undefined;
        }
    }

    /**
     * 更新特定会话的资源状态
     */
    updateSessionResourceState<K extends 'runtime' | 'prompt' | 'tools' | 'memory' | 'tokenUsage' | 'skills' | 'runs' | 'artifacts' | 'permissions' | 'audit' | 'diagnostics'>(
        sessionId: string,
        key: K,
        update: Partial<ResourceState<any>>
    ) {
        let map: Map<string, ResourceState<any>>;
        switch (key) {
            case 'runtime': map = this.sessionRuntimeStates; break;
            case 'prompt': map = this.sessionPromptStates; break;
            case 'tools': map = this.sessionToolStates; break;
            case 'memory': map = this.sessionMemoryHitStates; break;
            case 'tokenUsage': map = this.sessionTokenUsageStates; break;
            case 'skills': map = this.sessionSkillBindingStates; break;
            case 'runs': map = this.sessionRunStates; break;
            case 'artifacts': map = this.sessionArtifactStates; break;
            case 'permissions': map = this.sessionPermissionStates; break;
            case 'audit': map = this.sessionAuditStates; break;
            case 'diagnostics': map = this.sessionDiagnosticStates; break;
            default: return;
        }

        const current = map.get(sessionId) || this.createEmptyResource();
        if (typeof update.updatedAt === 'number' && typeof current.updatedAt === 'number' && update.updatedAt < current.updatedAt) {
            return;
        }
        const next = { ...current, ...update };
        map.set(sessionId, next);

        // LRU 淘汰：非当前会话缓存最多保留 3 个
        this.evictOldSessionCache(map, sessionId);

        if (this.currentSessionId === sessionId) {
            this.bus.emit(Events.CONSOLE_DATA_UPDATED, { sessionId, data: next });
        }
    }

    /**
     * LRU 淘汰：保留当前会话 + 最近 3 个非当前会话的缓存
     * JS Map 天然保持插入顺序，最早插入的排在前面
     */
    private evictOldSessionCache(map: Map<string, ResourceState<unknown>>, currentWriteId: string): void {
        const maxNonCurrentEntries = 3;
        const nonCurrentKeys: string[] = [];

        for (const key of map.keys()) {
            if (key !== this.currentSessionId && key !== currentWriteId) {
                nonCurrentKeys.push(key);
            }
        }

        while (nonCurrentKeys.length > maxNonCurrentEntries) {
            const oldest = nonCurrentKeys.shift()!;
            map.delete(oldest);
        }
    }

    createEmptyResource<T>(): ResourceState<T> {
        return { status: 'idle', loaded: false, loading: false, unsupported: false };
    }

    setLoadingResource<T>(state: ResourceState<T>): ResourceState<T> {
        return { ...state, status: 'loading', loading: true, error: undefined, unsupported: false };
    }

    setLoadedResource<T>(data: T): ResourceState<T> {
        return { status: 'ready', loaded: true, loading: false, data, unsupported: false, updatedAt: Date.now() };
    }

    setErrorResource<T>(error: string): ResourceState<T> {
        return { status: 'error', loaded: true, loading: false, error, unsupported: false, updatedAt: Date.now() };
    }

    setUnsupportedResource<T>(error: string, capability?: string): ResourceState<T> {
        const message = capability ? `${error} (${capability})` : error;
        return { status: 'error', loaded: true, loading: false, error: message, unsupported: true, updatedAt: Date.now() };
    }

    toResourceError<T>(error: unknown, fallbackMessage: string): ResourceState<T> {
        if (error instanceof GatewayRequestError && error.kind === 'unsupported') {
            return this.setUnsupportedResource<T>(error.message, error.capability);
        }
        if (error instanceof Error) {
            return this.setErrorResource<T>(error.message);
        }
        return this.setErrorResource<T>(fallbackMessage);
    }
}
