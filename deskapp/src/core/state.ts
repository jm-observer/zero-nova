import { EventBus, Events } from './event-bus';
import { 
    Session, 
    Message, 
    AgentModelItem, 
    PendingAttachment, 
    McpServerView,
    WorkingMode
} from './types';
import { GatewayClient } from '../gateway-client';

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

    constructor(bus: EventBus) {
        this.bus = bus;
    }

    // --- 状态操作方法 ---

    setGatewayClient(client: GatewayClient) {
        this.gatewayClient = client;
    }

    setCurrentSession(id: string | null) {
        if (this.currentSessionId !== id) {
            this.currentSessionId = id;
            this.bus.emit(Events.SESSION_SELECTED, { sessionId: id });
            this.bus.emit(Events.SESSION_CHANGED, { sessionId: id, messages: this.messages });
        }
    }

    setMessages(messages: Message[]) {
        this.messages = messages;
        this.bus.emit(Events.MESSAGES_UPDATED, { sessionId: this.currentSessionId, messages });
        this.bus.emit(Events.SESSION_CHANGED, { sessionId: this.currentSessionId, messages });
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
}
