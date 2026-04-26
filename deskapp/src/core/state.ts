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
} from './types';
import { GatewayClient } from '../gateway-client';

/**
 * 前端 token 累加更新事件
 */
interface ChatTokenUsageUpdate {
    sessionId: string;
    usage: TokenUsageView;
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
    
    // 工作模式
    currentWorkingMode: WorkingMode = 'standalone';

    // 基础设施
    gatewayClient: GatewayClient | null = null;

    // --- Agent Console 状态 ---
    consoleVisible = false;
    consoleActiveTab: 'overview' | 'model' | 'tools' | 'skills' | 'prompt-memory' = 'overview';
    
    agentRuntimeState: ResourceState<AgentRuntimeSnapshot> = this.createEmptyResource();
    sessionRuntimeStates = new Map<string, ResourceState<SessionRuntimeSnapshot>>();
    sessionPromptStates = new Map<string, ResourceState<PromptPreviewView>>();
    sessionToolStates = new Map<string, ResourceState<ToolDescriptorView[]>>();
    sessionMemoryHitStates = new Map<string, ResourceState<MemoryHitView[]>>();
    sessionTokenUsageStates = new Map<string, ResourceState<TokenUsageView>>();

    // --- 前端 Token 累加 handlers (Plan 2) ---
    private tokenUsageHandlerRef: ((msg: import('../gateway-client').GatewayMessage) => void) | null = null;

    // --- 模型绑定缓存 (Plan 2) ---
    modelBindingCache: Record<string, ModelBindingView[]> = {};

    // --- Skill 运行态绑定缓存 (Plan 3) ---
    skillBindingStates = new Map<string, ResourceState<SkillBindingView>>();

    // --- Tool 运行时状态缓存 (Plan 3) ---
    private toolStatusCache = new Map<string, { lastCallStatus?: 'success' | 'error' | 'running'; lastUsedAt?: number; unlockedBy?: string; unlockedReason?: string }>();
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
        this.bus.emit(Events.CONSOLE_DATA_UPDATED, { sessionId, data: merged });
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
    setSkillBinding(skillId: string, binding: SkillBindingView): void {
        this.skillBindingStates.set(skillId, this.setLoadedResource(binding));
        this.bus.emit(Events.CONSOLE_DATA_UPDATED, { sessionId: this.currentSessionId, data: { type: 'skill_binding', binding } });
    }

    /**
     * 获取技能绑定状态
     */
    getSkillBinding(skillId: string): SkillBindingView | undefined {
        return this.skillBindingStates.get(skillId)?.data;
    }

    /**
     * 获取所有技能绑定状态
     */
    getAllSkillBindings(): Map<string, SkillBindingView> {
        const result = new Map<string, SkillBindingView>();
        for (const [id, resource] of this.skillBindingStates) {
            if (resource.data) {
                result.set(id, resource.data);
            }
        }
        return result;
    }

    /**
     * 处理技能激活事件
     */
    handleSkillActivated(event: SkillActivatedEvent): void {
        const binding: SkillBindingView = {
            id: event.skillId,
            title: event.title ?? event.skillId,
            source: event.source || 'runtime',
            enabled: true,
            contentPreview: event.content?.substring(0, 200),
            activatedAt: event.timestamp,
            sticky: event.sticky || false,
        };
        this.setSkillBinding(event.skillId, binding);
        // 同时更新 agentRuntime 的 activeSkills
        if (this.agentRuntimeState.data) {
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
        if (event.sticky) {
            // sticky 技能保留但可能被标记为退出
            const existing = this.skillBindingStates.get(event.skillId)?.data;
            if (existing) {
                existing.enabled = false;
                this.skillBindingStates.set(event.skillId, this.setLoadedResource(existing));
            }
        } else {
            // 非 sticky 技能从列表中移除
            this.skillBindingStates.delete(event.skillId);
        }
        // 从 agentRuntime 中移除
        if (this.agentRuntimeState.data) {
            const skills = this.agentRuntimeState.data.activeSkills || [];
            this.agentRuntimeState.data.activeSkills = skills.filter(id => id !== event.skillId);
        }
    }

    // --- Tool Status 操作 (Plan 3) ---

    /**
     * 更新工具运行时状态
     */
    updateToolStatus(toolName: string, status: { lastCallStatus?: 'success' | 'error' | 'running'; lastUsedAt?: number; unlockedBy?: string; unlockedReason?: string }): void {
        const current = this.toolStatusCache.get(toolName) || {};
        this.toolStatusCache.set(toolName, { ...current, ...status });
        this.bus.emit(Events.CONSOLE_DATA_UPDATED, { sessionId: this.currentSessionId, data: { type: 'tool_status', toolName, ...status } });
    }

    /**
     * 获取工具运行时状态
     */
    getToolStatus(toolName: string): { lastCallStatus?: 'success' | 'error' | 'running'; lastUsedAt?: number; unlockedBy?: string; unlockedReason?: string } | undefined {
        return this.toolStatusCache.get(toolName);
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

            this.updateToolStatus(event.toolName, {
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

    setConsoleTab(tab: 'overview' | 'model' | 'tools' | 'skills' | 'prompt-memory') {
        if (this.consoleActiveTab !== tab) {
            this.consoleActiveTab = tab;
            this.bus.emit(Events.CONSOLE_TAB_CHANGED, { tab });
        }
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
        key: 'runtime' | 'prompt' | 'tools' | 'memory' | 'tokenUsage'
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
            default:
                return undefined;
        }
    }

    /**
     * 更新特定会话的资源状态
     */
    updateSessionResourceState<K extends 'runtime' | 'prompt' | 'tools' | 'memory' | 'tokenUsage'>(
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
            default: return;
        }

        const current = map.get(sessionId) || this.createEmptyResource();
        const next = { ...current, ...update };
        map.set(sessionId, next);
        
        if (this.currentSessionId === sessionId) {
            this.bus.emit(Events.CONSOLE_DATA_UPDATED, { sessionId, data: next });
        }
    }

    createEmptyResource<T>(): ResourceState<T> {
        return { loaded: false, loading: false };
    }

    setLoadingResource<T>(state: ResourceState<T>): ResourceState<T> {
        return { ...state, loading: true, error: undefined };
    }

    setLoadedResource<T>(data: T): ResourceState<T> {
        return { loaded: true, loading: false, data, updatedAt: Date.now() };
    }

    setErrorResource<T>(error: string): ResourceState<T> {
        return { loaded: true, loading: false, error, updatedAt: Date.now() };
    }
}
