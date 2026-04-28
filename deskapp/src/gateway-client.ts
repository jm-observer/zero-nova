import type {
    ProgressEvent,
    ChatIntentPayload,
    Session,
    McpServerView,
    ServerConfigView,
    ServerConfigUpdate,
    AgentRuntimeSnapshot,
    SessionRuntimeSnapshot,
    TokenUsageView,
    PromptPreviewView,
    ToolDescriptorView,
    MemoryHitView,
    SkillBindingView,
    ToolUnlockedEvent,
    SkillActivatedEvent,
    SkillSwitchedEvent,
    SkillExitedEvent,
    GatewayCapabilityErrorPayload,
    DebugLogEntry,
    EvolutionConfirmRequest,
    RunSummaryView,
    RunDetailView,
    SessionArtifactView,
    PermissionRequestView,
    AuditLogView,
    DiagnosticIssueView,
    WorkspaceRestoreView,
} from './core/types';
import type { AgentInspectRequest, WorkspaceRestoreRequest } from './generated/generated-types';
import { validateOutboundMessage } from './gateway-messages';

export type {
    ProgressEvent,
    ChatIntentPayload,
    Session,
    McpServerView,
    ServerConfigView,
    ServerConfigUpdate,
    DebugLogEntry,
    EvolutionConfirmRequest,
};


/**
 * WebSocket 客户端封装
 * 用于渲染进程连接 Gateway Server
 */

export interface GatewayMessage {
    type: string;
    id?: string;
    payload?: unknown;
}

type MessageHandler = (message: GatewayMessage) => void;
type ProgressHandler = (event: ProgressEvent) => void;
type ChatIntentHandler = (payload: ChatIntentPayload) => void;
type ConnectionHandler = (status: 'connecting' | 'connected' | 'disconnected' | 'reconnecting' | 'failed') => void;

interface VoiceCapabilitiesView {
    stt: { enabled: boolean; available: boolean };
    tts: { enabled: boolean; available: boolean; voice: string; autoPlay: boolean };
}

interface VoiceTranscribePayload {
    sessionId?: string;
    audioFormat: string;
    sampleRate?: number;
    channelCount?: number;
    language?: string;
    mode?: 'once';
    audio: ArrayBuffer;
}

interface VoiceTranscribeResult {
    text: string;
    confidence?: number;
    durationMs?: number;
    segments?: Array<{ startMs: number; endMs: number; text: string }>;
}

export class GatewayRequestError extends Error {
    kind: 'unsupported' | 'request_failed';
    capability?: string;
    code?: string;

    constructor(message: string, options?: { kind?: 'unsupported' | 'request_failed'; capability?: string; code?: string }) {
        super(message);
        this.name = 'GatewayRequestError';
        this.kind = options?.kind ?? 'request_failed';
        this.capability = options?.capability;
        this.code = options?.code;
    }
}

/**
 * Gateway WebSocket 客户端
 */
export class GatewayClient {

    private ws: WebSocket | null = null;
    private url: string;
    private token?: string;
    private authenticated = false;
    private pendingRequests = new Map<string, {
        resolve: (value: unknown) => void;
        reject: (error: Error) => void;
    }>();
    private progressHandlers: ProgressHandler[] = [];
    private chatIntentHandlers: ChatIntentHandler[] = [];
    private messageHandlers: MessageHandler[] = [];
    private connectionHandlers: ConnectionHandler[] = [];
    private reconnectAttempts = 0;
    private maxReconnectAttempts = 10;
    private reconnectDelay = 1000;
    private shouldReconnect = true;

    private encodeAudioBase64(audio: ArrayBuffer): string {
        let binary = '';
        const bytes = new Uint8Array(audio);

        bytes.forEach(byte => {
            binary += String.fromCharCode(byte);
        });

        return btoa(binary);
    }

    private decodeAudioBase64(audioBase64: string): ArrayBuffer {
        const binary = atob(audioBase64);
        const bytes = new Uint8Array(binary.length);

        for (let index = 0; index < binary.length; index += 1) {
            bytes[index] = binary.charCodeAt(index);
        }

        return bytes.buffer;
    }

    private normalizeToolUnlockedEvent(payload: unknown): ToolUnlockedEvent {
        const record = (payload ?? {}) as Record<string, unknown>;

        return {
            sessionId: typeof record.sessionId === 'string'
                ? record.sessionId
                : (typeof record.session_id === 'string' ? record.session_id : undefined),
            toolName: String(record.toolName ?? record.tool_name ?? ''),
            description: typeof record.description === 'string' ? record.description : undefined,
            source: this.normalizeUnlockedSource(record.source),
            reason: typeof record.reason === 'string' ? record.reason : undefined,
            timestamp: typeof record.timestamp === 'number' ? record.timestamp : Date.now(),
        };
    }

    private normalizeSkillActivatedEvent(payload: unknown): SkillActivatedEvent {
        const record = (payload ?? {}) as Record<string, unknown>;

        return {
            sessionId: typeof record.sessionId === 'string'
                ? record.sessionId
                : (typeof record.session_id === 'string' ? record.session_id : undefined),
            skillId: String(record.skillId ?? record.skill_id ?? ''),
            title: typeof record.title === 'string'
                ? record.title
                : (typeof record.skill_name === 'string' ? record.skill_name : undefined),
            content: typeof record.content === 'string' ? record.content : undefined,
            source: this.normalizeSkillSource(record.source),
            sticky: typeof record.sticky === 'boolean' ? record.sticky : undefined,
            timestamp: typeof record.timestamp === 'number' ? record.timestamp : Date.now(),
        };
    }

    private normalizeSkillSwitchedEvent(payload: unknown): SkillSwitchedEvent {
        const record = (payload ?? {}) as Record<string, unknown>;

        return {
            sessionId: typeof record.sessionId === 'string'
                ? record.sessionId
                : (typeof record.session_id === 'string' ? record.session_id : undefined),
            previousSkillId: typeof record.previousSkillId === 'string'
                ? record.previousSkillId
                : (typeof record.from_skill === 'string' ? record.from_skill : undefined),
            currentSkillId: String(record.currentSkillId ?? record.to_skill ?? ''),
            currentSkillTitle: typeof record.currentSkillTitle === 'string'
                ? record.currentSkillTitle
                : (typeof record.to_skill === 'string' ? record.to_skill : undefined),
            timestamp: typeof record.timestamp === 'number' ? record.timestamp : Date.now(),
        };
    }

    private normalizeSkillExitedEvent(payload: unknown): SkillExitedEvent {
        const record = (payload ?? {}) as Record<string, unknown>;

        return {
            sessionId: typeof record.sessionId === 'string'
                ? record.sessionId
                : (typeof record.session_id === 'string' ? record.session_id : undefined),
            skillId: String(record.skillId ?? record.skill_id ?? ''),
            title: typeof record.title === 'string'
                ? record.title
                : (typeof record.skill_name === 'string' ? record.skill_name : undefined),
            sticky: typeof record.sticky === 'boolean' ? record.sticky : undefined,
            timestamp: typeof record.timestamp === 'number' ? record.timestamp : Date.now(),
        };
    }

    private normalizeUnlockedSource(source: unknown): ToolUnlockedEvent['source'] {
        if (source === 'tool_search' || source === 'skill_activation' || source === 'manual') {
            return source;
        }
        return undefined;
    }

    private normalizeSkillSource(source: unknown): SkillActivatedEvent['source'] {
        if (source === 'global' || source === 'agent' || source === 'runtime') {
            return source;
        }
        return undefined;
    }

    private toRequestError(payload: unknown): GatewayRequestError {
        const errorPayload = (payload ?? {}) as Partial<GatewayCapabilityErrorPayload>;
        const code = typeof errorPayload.code === 'string' ? errorPayload.code : undefined;
        const capability = typeof errorPayload.capability === 'string' ? errorPayload.capability : undefined;
        const message = typeof errorPayload.message === 'string' && errorPayload.message
            ? errorPayload.message
            : '请求失败';

        return new GatewayRequestError(message, {
            kind: code === 'capability_not_supported' ? 'unsupported' : 'request_failed',
            capability,
            code,
        });
    }

    constructor(url: string, token?: string) {
        this.url = url;
        this.token = token;
    }

    /**
     * 连接到 Gateway
     */
    async connect(): Promise<void> {
        return new Promise((resolve, reject) => {
            try {
                this.notifyConnectionChange('connecting');
                this.ws = new WebSocket(this.url);

                this.ws.onopen = () => {
                    console.log('[GatewayClient] Connected, waiting for welcome message...');
                    this.reconnectAttempts = 0;
                };

                this.ws.onmessage = (event) => {
                    console.log('[GatewayClient] Raw message received:', typeof event.data === 'string' ? event.data.substring(0, 500) : event.data);
                    this.handleMessage(event.data);
                };

                this.ws.onclose = () => {
                    console.log('[GatewayClient] Connection closed');
                    this.authenticated = false;
                    this.notifyConnectionChange('disconnected');
                    if (this.shouldReconnect) {
                        this.tryReconnect();
                    }
                };

                this.ws.onerror = (error) => {
                    console.error('[GatewayClient] Connection error:', error);
                    if (this.reconnectAttempts === 0) {
                        // 首次连接失败才 reject
                        reject(new Error('WebSocket 连接失败'));
                    }
                };

                // 等待 welcome 消息
                const welcomeHandler = (msg: GatewayMessage) => {
                    if (msg.type === 'welcome') {
                        this.removeMessageHandler(welcomeHandler);
                        const payload = msg.payload as { requireAuth?: boolean; setupRequired?: boolean };

                        // 保存首次运行标志
                        if (payload.setupRequired) {
                        (this as unknown as { _setupRequired: boolean })._setupRequired = true;
                    }

                        if (payload.requireAuth && this.token) {
                            this.authenticate().then(() => {
                                this.notifyConnectionChange('connected');
                                resolve();
                            }).catch(reject);
                        } else {
                            this.authenticated = true;
                            this.notifyConnectionChange('connected');
                            resolve();
                        }
                    }
                };
                this.addMessageHandler(welcomeHandler);

            } catch (error) {
                reject(error);
            }
        });
    }

    /**
     * 认证
     */
    private async authenticate(): Promise<void> {
        return new Promise((resolve, reject) => {
            const authHandler = (msg: GatewayMessage) => {
                if (msg.type === 'auth.success') {
                    this.removeMessageHandler(authHandler);
                    this.authenticated = true;
                    resolve();
                } else if (msg.type === 'auth.failed') {
                    this.removeMessageHandler(authHandler);
                    reject(new Error('认证失败'));
                }
            };
            this.addMessageHandler(authHandler);
            this.send({ type: 'auth', payload: { token: this.token } });
        });
    }

    /**
     * 尝试重连
     */
    private tryReconnect(): void {
        if (this.reconnectAttempts >= this.maxReconnectAttempts) {
            console.error('[GatewayClient] Max reconnect attempts reached');
            this.notifyConnectionChange('failed');
            return;
        }

        this.reconnectAttempts++;
        const delay = Math.min(this.reconnectDelay * Math.pow(2, this.reconnectAttempts - 1), 30000);
        console.log(`[GatewayClient] Reconnecting in ${delay}ms (${this.reconnectAttempts}/${this.maxReconnectAttempts})`);

        this.notifyConnectionChange('reconnecting');

        setTimeout(() => {
            if (this.shouldReconnect) {
                this.connect().catch(console.error);
            }
        }, delay);
    }

    /**
     * 断开连接
     */
    disconnect(): void {
        this.shouldReconnect = false;
        if (this.ws) {
            this.ws.close();
            this.ws = null;
        }
    }

    /**
     * 通知连接状态变化
     */
    private notifyConnectionChange(status: 'connecting' | 'connected' | 'disconnected' | 'reconnecting' | 'failed'): void {
        this.connectionHandlers.forEach(handler => handler(status));
    }

    /**
     * 监听连接状态变化
     */
    onConnectionChange(handler: ConnectionHandler): () => void {
        this.connectionHandlers.push(handler);
        return () => {
            const index = this.connectionHandlers.indexOf(handler);
            if (index !== -1) {
                this.connectionHandlers.splice(index, 1);
            }
        };
    }

    /**
     * 是否已连接
     */
    isConnected(): boolean {
        return this.ws?.readyState === WebSocket.OPEN && this.authenticated;
    }

    /**
     * 发送消息
     */
    private send(message: GatewayMessage): void {
        this.assertOutboundMessage(message.type, message.payload);
        if (this.ws?.readyState === WebSocket.OPEN) {
            this.ws.send(JSON.stringify(message));
        }
    }

    private assertOutboundMessage(type: string, payload: unknown): void {
        const hints = validateOutboundMessage(type, payload ?? {});
        if (hints.length === 0) {
            return;
        }

        const details = hints
            .map(hint => `${hint.path}: expected ${hint.expected}`)
            .join('; ');
        throw new Error(`[GatewayClient] outbound message validation failed for ${type}: ${details}`);
    }

    /**
     * 处理收到的消息
     */
    private handleMessage(data: string): void {
        try {
            const message: GatewayMessage = JSON.parse(data);
            console.log('[GatewayClient] Message received:', message.type, message.id, message);

            // 通知所有消息处理器
            this.messageHandlers.forEach(handler => handler(message));

            // 处理进度事件
            if (message.type === 'chat.progress') {
                const event = message.payload as ProgressEvent;
                // 兼容性与规范化处理
                if (event.toolName && !event.tool) event.tool = event.toolName;
                if (!event.toolName && event.tool) event.toolName = event.tool;

                const eventRecord = event as unknown as Record<string, unknown>;
                if (event.toolUseId && !eventRecord.tool_use_id) {
                    eventRecord.tool_use_id = event.toolUseId;
                }
                
                this.progressHandlers.forEach(handler => handler(event));
            }
            
            // 处理聊天意向识别事件
            if (message.type === 'chat.intent') {
                const payload = message.payload as ChatIntentPayload;
                this.chatIntentHandlers.forEach(handler => handler(payload));
                return;
            }

            // 处理聊天完成事件
            if (message.type === 'chat.complete') {
                const payload = message.payload as { output?: string; sessionId?: string; usage?: { input_tokens?: number; output_tokens?: number; cache_creation_input_tokens?: number; cache_read_input_tokens?: number } };
                const completeEvent: ProgressEvent = {
                    type: 'complete',
                    output: payload?.output,
                    sessionId: payload?.sessionId,
                };
                this.progressHandlers.forEach(handler => handler(completeEvent));

                // 前端 token 累加：发送 usage 更新事件
                if (payload?.usage && payload?.sessionId) {
                    const usageUpdate = {
                        sessionId: payload.sessionId,
                        usage: {
                            inputTokens: payload.usage.input_tokens ?? 0,
                            outputTokens: payload.usage.output_tokens ?? 0,
                            cacheCreationInputTokens: payload.usage.cache_creation_input_tokens,
                            cacheReadInputTokens: payload.usage.cache_read_input_tokens,
                        },
                    };
                    // 通知所有消息处理器（包括 AppState）
                    this.messageHandlers.forEach(handler => handler({ type: 'chat.token_usage', payload: usageUpdate }));
                }
            }

            // 处理客户端 MCP 工具调用请求
            if (message.type === 'mcp.client.call' && message.id) {
                this.handleClientMcpCall(message);
                return; // 不走 pendingRequests 逻辑
            }

            // 处理响应 —— 只对「最终」消息 resolve/reject
            // chat.start / chat.progress / config.progress 是中间状态消息，不应触发 resolve
            const isIntermediateMessage =
                message.type === 'chat.start' || message.type === 'chat.progress' || message.type === 'config.progress' || message.type === 'nexusai.auth-expired';

            if (message.id && this.pendingRequests.has(message.id) && !isIntermediateMessage) {
                console.log('[GatewayClient] Matched pending request (final):', message.id, message.type);
                const { resolve, reject } = this.pendingRequests.get(message.id)!;
                this.pendingRequests.delete(message.id);

                if (message.type === 'error' || message.type.endsWith('.error')) {
                    reject(this.toRequestError(message.payload));
                } else {
                    resolve(message.payload);
                }
            }
        } catch (error) {
            console.error('[GatewayClient] Failed to parse message:', error);
        }
    }

    /**
     * 添加消息处理器
     */
    addMessageHandler(handler: MessageHandler): void {
        this.messageHandlers.push(handler);
    }

    /**
     * 移除消息处理器
     */
    removeMessageHandler(handler: MessageHandler): void {
        const index = this.messageHandlers.indexOf(handler);
        if (index !== -1) {
            this.messageHandlers.splice(index, 1);
        }
    }

    /**
     * 处理 Gateway 发来的客户端 MCP 工具调用请求
     */
    private async handleClientMcpCall(message: GatewayMessage): Promise<void> {
        const { tool, args } = message.payload as { tool: string; args: Record<string, unknown> };
        console.log('[GatewayClient] Client MCP tool invocation received:', tool);

        try {
            const response = await this.request<{ success: boolean; result?: unknown; error?: string }>('mcp.tool.call', { tool, args });
            this.send({
                type: 'mcp.client.result',
                id: message.id,
                payload: response.success
                    ? { success: true, result: response.result }
                    : { success: false, error: response.error },
            });
        } catch (err: any) {
            this.send({
                type: 'mcp.client.result',
                id: message.id,
                payload: { success: false, error: err.message || '客户端工具调用失败' },
            });
        }
    }

    /**
     * 将客户端本机 MCP 工具注册到 Gateway
     */
    registerClientMcpTools(tools: Array<{ name: string; description: string; parameters: Record<string, unknown> }>): void {
        if (!this.isConnected()) {
            console.warn('[GatewayClient] Not connected, cannot register client MCP tools');
            return;
        }
        console.log(`[GatewayClient] Registering client MCP tools: ${tools.length}`);
        this.send({
            type: 'mcp.client.register',
            payload: { tools },
        });
    }

    /**
     * 通知 Gateway 移除客户端 MCP 工具
     */
    unregisterClientMcpTools(): void {
        if (!this.isConnected()) return;
        console.log('[GatewayClient] Removing client MCP tools');
        this.send({
            type: 'mcp.client.unregister',
        });
    }

    /**
     * 监听进度事件
     */
    onProgress(handler: ProgressHandler): () => void {
        this.progressHandlers.push(handler);
        return () => {
            const index = this.progressHandlers.indexOf(handler);
            if (index !== -1) {
                this.progressHandlers.splice(index, 1);
            }
        };
    }
    
    /**
     * 监听聊天意向识别事件
     */
    onChatIntent(handler: ChatIntentHandler): () => void {
        this.chatIntentHandlers.push(handler);
        return () => {
            const index = this.chatIntentHandlers.indexOf(handler);
            if (index !== -1) {
                this.chatIntentHandlers.splice(index, 1);
            }
        };
    }

    /**
     * 发起请求并等待响应
     * @param timeout 超时毫秒数，0 表示不超时（默认 120 秒）
     */
    public request<T>(type: string, payload?: unknown, timeout: number = 120000): Promise<T> {
        return new Promise((resolve, reject) => {
            const id = crypto.randomUUID();
            this.pendingRequests.set(id, {
                resolve: resolve as (value: unknown) => void,
                reject
            });
            this.send({ type, id, payload });

            // 超时（0 表示不限时，适用于 chat 等长时间执行场景）
            if (timeout > 0) {
                setTimeout(() => {
                    if (this.pendingRequests.has(id)) {
                        this.pendingRequests.delete(id);
                        reject(new Error('请求超时'));
                    }
                }, timeout);
            }
        });
    }

    /**
     * 发送聊天消息（支持附件、云端 Agent）
     * 不设超时：Agent 多步执行可能耗时很长，进度通过 chat.progress 实时推送
     */
    async chat(
        input: string,
        sessionId?: string,
        attachments?: Array<{ path: string; name: string; size: number; ext: string }>,
        options?: { source?: 'local' | 'cloud'; chatroomId?: number; agentId?: string }
    ): Promise<string> {
        const payload: Record<string, unknown> = { input, sessionId };
        if (attachments?.length) {
            payload.attachments = attachments;
        }
        if (options?.source) {
            payload.source = options.source;
        }
        if (options?.chatroomId) {
            payload.chatroomId = options.chatroomId;
        }
        if (options?.agentId) {
            payload.agentId = options.agentId;
        }
        const result = await this.request<{ output?: string }>('chat', payload, 0);
        console.log('[GatewayClient] Chat response:', result);
        return result?.output || '';
    }

    async getVoiceCapabilities(): Promise<VoiceCapabilitiesView> {
        return this.request<VoiceCapabilitiesView>('voice.capabilities.get', {});
    }

    async transcribeVoice(payload: VoiceTranscribePayload): Promise<VoiceTranscribeResult> {
        return this.request<VoiceTranscribeResult>('voice.transcribe.request', {
            sessionId: payload.sessionId,
            audioFormat: payload.audioFormat,
            sampleRate: payload.sampleRate,
            channelCount: payload.channelCount,
            language: payload.language,
            mode: payload.mode ?? 'once',
            audioBase64: this.encodeAudioBase64(payload.audio),
        });
    }

    async synthesizeVoice(text: string, sessionId?: string, voice?: string): Promise<ArrayBuffer> {
        const response = await this.request<{ audioFormat: string; audioBase64: string }>('voice.tts.request', {
            text,
            sessionId,
            voice,
        });

        return this.decodeAudioBase64(response.audioBase64);
    }

    /**
     * 停止正在执行的任务
     */
    stopTask(sessionId: string): void {
        console.log('[GatewayClient] Stopping task:', sessionId);
        this.send({ type: 'chat.stop', payload: { sessionId } });
    }

    /**
     * 获取会话列表
     */
    async getSessions(): Promise<Session[]> {
        console.log('[GatewayClient] getSessions request');
        const result = await this.request<{ sessions: Session[] }>('sessions.list');
        console.log('[GatewayClient] getSessions response:', result);
        return result.sessions;
    }

    /**
     * 获取会话消息
     */
    async getMessages(sessionId: string): Promise<unknown[]> {
        console.log('[GatewayClient] getMessages request:', sessionId);
        const result = await this.request<{ messages: unknown[] }>('sessions.messages', { sessionId });
        console.log('[GatewayClient] getMessages response:', result);
        return result.messages;
    }

    /**
     * 获取会话日志
     */
    async getLogs(sessionId: string): Promise<unknown[]> {
        const result = await this.request<{ logs: unknown[] }>('sessions.logs', { sessionId });
        return result.logs;
    }

    /**
     * 创建会话
     */
    async createSession(options: { title?: string; agentId?: string; cloudChatroomId?: number; cloudAgentName?: string }): Promise<Session> {
        const result = await this.request<{ session: Session }>('sessions.create', options);
        return result.session;
    }

    /**
     * 删除会话
     */
    async deleteSession(sessionId: string): Promise<void> {
        await this.request<{ success: boolean }>('sessions.delete', { sessionId });
    }

    /**
     * 复制会话
     */
    async copySession(sessionId: string, index?: number): Promise<Session> {
        const result = await this.request<{ session: Session }>('sessions.copy', { sessionId, index });
        return result.session;
    }

    // ========================
    // Agent 管理 API
    // ========================

    /** 获取所有用户 Agent 列表 */
    async getAgents(): Promise<Array<{ id: string; name: string; description?: string; icon?: string; color?: string; default?: boolean; systemPrompt?: string; createdAt: number; updatedAt: number }>> {
        const result = await this.request<{ agents: Array<{ id: string; name: string; description?: string; icon?: string; color?: string; default?: boolean; systemPrompt?: string; createdAt: number; updatedAt: number }> }>('agents.list');
        return result.agents || [];
    }

    /** 创建新 Agent */
    async createAgent(config: { id: string; name?: string; description?: string; icon?: string; color?: string; systemPrompt?: string }): Promise<Record<string, unknown>> {
        const result = await this.request<{ agent: Record<string, unknown> }>('agents.create', config);
        return result.agent;
    }

    /** 更新 Agent 配置 */
    async updateAgent(agentId: string, updates: Record<string, unknown>): Promise<Record<string, unknown>> {
        const result = await this.request<{ agent: Record<string, unknown> }>('agents.update', { agentId, updates });
        return result.agent;
    }

    /** 删除 Agent */
    async deleteAgent(agentId: string): Promise<boolean> {
        const result = await this.request<{ success: boolean }>('agents.delete', { agentId });
        return result.success;
    }

    /** 切换 Agent（返回 Agent 信息 + 会话历史） */
    async switchAgent(agentId: string): Promise<{ agent: Record<string, unknown>; messages: unknown[] }> {
        return this.request<{ agent: Record<string, unknown>; messages: unknown[] }>('agents.switch', { agentId });
    }

    /** 清除 Agent 历史消息 */
    async clearAgentHistory(agentId: string): Promise<boolean> {
        const result = await this.request<{ success: boolean }>('agents.history.clear', { agentId });
        return result.success;
    }

    /**
     * 监听 NexusAI 认证过期事件（Atlas 模式 token 失效时触发）
     */
    onAuthExpired(handler: (message: string) => void): () => void {
        const messageHandler = (msg: GatewayMessage) => {
            if (msg.type === 'nexusai.auth-expired') {
                const payload = msg.payload as { message?: string };
                handler(payload?.message || 'NexusAI access token 已过期，请重新登录');
            }
        };
        this.addMessageHandler(messageHandler);
        return () => this.removeMessageHandler(messageHandler);
    }

    /**
     * 监听会话更新事件（定时任务执行结果归集到会话时触发）
     */
    onSessionUpdated(handler: (sessionId: string) => void): () => void {
        const messageHandler = (msg: GatewayMessage) => {
            if (msg.type === 'session.updated') {
                const payload = msg.payload as { sessionId: string };
                handler(payload.sessionId);
            }
        };
        this.addMessageHandler(messageHandler);
        return () => this.removeMessageHandler(messageHandler);
    }

    /**
     * 监听协作完成事件（Agent 间协作结果通知）
     */
    onCollaborationResult(handler: (event: {
        sessionId: string;
        agentId: string;
        agentType: string;
        task: string;
        status: string;
        mode: string;
        output?: string;
        error?: string;
        duration?: number;
    }) => void): () => void {
        const messageHandler = (msg: GatewayMessage) => {
            if (msg.type === 'collaboration_result') {
                handler(msg.payload as {
                    sessionId: string;
                    agentId: string;
                    agentType: string;
                    task: string;
                    status: string;
                    mode: string;
                    output?: string;
                    error?: string;
                    duration?: number;
                });
            }
        };
        this.addMessageHandler(messageHandler);
        return () => this.removeMessageHandler(messageHandler);
    }

    // ========================
    // Memory API
    // ========================

    /**
     * 获取记忆统计信息
     */
    async memoryStats(): Promise<{ enabled: boolean; totalCount?: number; dbSizeBytes?: number; vectorDim?: number; embeddingModel?: string }> {
        return this.request('memory.stats');
    }

    /**
     * 分页列出记忆
     */
    async memoryList(page: number = 1, pageSize: number = 20): Promise<{ items: any[]; total: number; page: number; pageSize: number }> {
        return this.request('memory.list', { page, pageSize });
    }

    /**
     * 搜索记忆
     */
    async memorySearch(query: string, limit: number = 10): Promise<{ items: any[] }> {
        return this.request('memory.search', { query, limit });
    }

    /**
     * 删除单条记忆
     */
    async memoryDelete(id: string): Promise<boolean> {
        const result = await this.request<{ success: boolean }>('memory.delete', { id });
        return result.success;
    }

    /**
     * 清空所有记忆
     */
    async memoryClear(): Promise<boolean> {
        const result = await this.request<{ success: boolean }>('memory.clear');
        return result.success;
    }

    // ========================
    // Distillation API
    // ========================

    /**
     * 获取蒸馏统计信息
     */
    async distillationStats(): Promise<any> {
        return this.request('distillation.stats');
    }

    /**
     * 获取卡片关系图数据
     */
    async distillationGraph(): Promise<{ cards: any[]; relations: any[]; topics: any[] }> {
        return this.request('distillation.graph');
    }

    /**
     * 更新蒸馏配置
     */
    async distillationUpdateConfig(config: Record<string, any>): Promise<{ success: boolean; message?: string }> {
        return this.request('distillation.config.update', config);
    }

    /**
     * 手动触发蒸馏
     */
    async distillationTrigger(): Promise<{ success: boolean; message?: string }> {
        return this.request('distillation.trigger');
    }

    /**
     * 获取卡片列表（支持层级筛选和分页）
     */
    async distillationCards(layer?: string, limit = 100, offset = 0): Promise<{ cards: any[]; total: number }> {
        return this.request('distillation.cards', { layer, limit, offset });
    }

    /**
     * 删除指定卡片
     */
    async distillationDeleteCard(cardId: string): Promise<{ success: boolean; message?: string }> {
        return this.request('distillation.card.delete', { cardId });
    }

    // ========================
    // Settings API
    // ========================

    /**
     * 获取当前设置
     */
    async getSettings(): Promise<{ outputPath: string; defaultOutputPath: string }> {
        return this.request('settings.get');
    }

    /**
     * 更新设置（传 null 重置为默认值）
     */
    async updateSettings(settings: { outputPath?: string | null }): Promise<{ outputPath: string }> {
        return this.request('settings.update', settings);
    }

    // ========================
    // Server Config API
    // ========================

    /**
     * 获取服务端配置
     */
    async getServerConfig(): Promise<ServerConfigView> {
        return this.request('config.get');
    }

    /**
     * 更新服务端配置
     */
    async updateServerConfig(updates: ServerConfigUpdate): Promise<{ success: boolean; message?: string }> {
        return this.request('config.update', updates);
    }

    isSetupRequired(): boolean {
        return !!(this as unknown as { _setupRequired: boolean })._setupRequired;
    }

    /**
     * 提交首次启动设置
     */
    async setupComplete(config: {
        provider: string;
        apiKey: string;
        baseUrl?: string;
        model?: string;
        agentName?: string;
        agentPrompt?: string;
        router?: {
            enabled: boolean;
            url?: string;
            appId?: string;
            appSecret?: string;
        };
    }): Promise<{ success: boolean; message?: string }> {
        const result = await this.request<{ message?: string }>('setup.complete', config);
        (this as unknown as { _setupRequired: boolean })._setupRequired = false;
        return { success: true, message: result?.message };
    }

    // ========================
    // Browser API
    // ========================

    // ========================
    // Debug API
    // ========================

    /**
     * 订阅 debug 日志
     */
    subscribeDebugLog(): void {
        this.send({ type: 'debug.subscribe' });
    }

    /**
     * 取消订阅 debug 日志
     */
    unsubscribeDebugLog(): void {
        this.send({ type: 'debug.unsubscribe' });
    }

    /**
     * 监听 debug 日志事件
     */
    onDebugLog(handler: (entry: DebugLogEntry) => void): () => void {
        const messageHandler = (msg: GatewayMessage) => {
            if (msg.type === 'debug.log') {
                handler(msg.payload as DebugLogEntry);
            }
        };
        this.addMessageHandler(messageHandler);
        return () => this.removeMessageHandler(messageHandler);
    }

    /**
     * 监听记忆索引重建进度
     */
    onRebuildProgress(handler: (progress: number) => void): () => void {
        const messageHandler = (msg: GatewayMessage) => {
            if (msg.type === 'config.rebuildProgress') {
                const payload = msg.payload as { progress: number };
                handler(payload.progress);
            }
        };
        this.addMessageHandler(messageHandler);
        return () => this.removeMessageHandler(messageHandler);
    }
    // ========================
    // Evolution API (自我进化)
    // ========================

    /**
     * 监听工具创建确认请求
     * Gateway 在 Agent 创建新工具时推送，前端弹出确认对话框
     */
    onEvolutionConfirm(handler: (request: EvolutionConfirmRequest) => void): () => void {
        const messageHandler = (msg: GatewayMessage) => {
            if (msg.type === 'evolution.confirm') {
                handler(msg.payload as EvolutionConfirmRequest);
            }
        };
        this.addMessageHandler(messageHandler);
        return () => this.removeMessageHandler(messageHandler);
    }

    /**
     * 响应工具确认请求
     */
    respondEvolutionConfirm(requestId: string, approved: boolean): void {
        this.send({
            type: 'evolution.confirm.response',
            payload: { requestId, approved },
        });
    }

    /**
     * 获取进化数据统计
     */
    async getEvolutionStats(): Promise<{
        schemaVersion: number;
        stats: { installedSkills: number; customTools: number; forgedSkills: number; spawnedAgents: number; mcpConnections: number };
    }> {
        return this.request('evolution.stats');
    }

    /**
     * 获取已安装技能列表
     */
    async getInstalledSkills(): Promise<{ skills: Array<{ slug: string; source: string; installedAt: string }> }> {
        return this.request('evolution.skills.list');
    }

    /**
     * 卸载技能
     */
    async uninstallSkill(slug: string): Promise<{ success: boolean }> {
        return this.request('evolution.skills.uninstall', { slug });
    }

    /**
     * 获取自定义工具列表
     */
    async getCustomTools(): Promise<{ tools: Array<{ name: string; description: string; scriptType: string; confirmed: boolean; validatorResult: string; createdAt: string }> }> {
        return this.request('evolution.tools.list');
    }

    /**
     * 删除自定义工具
     */
    async deleteCustomTool(name: string): Promise<{ success: boolean }> {
        return this.request('evolution.tools.delete', { name });
    }

    /**
     * 接受锻造建议
     */
    async acceptForgeSuggestion(suggestion: { id: string; title: string; content: string; category: string; reasoning: string }): Promise<{ success: boolean }> {
        return this.request('evolution.forge.accept', suggestion);
    }

    /**
     * 忽略锻造建议
     */
    async dismissForgeSuggestion(): Promise<{ success: boolean }> {
        return this.request('evolution.forge.dismiss');
    }

    /**
     * 获取已锻造技能列表
     */
    async getForgedSkills(): Promise<{ skills: Array<{ id: string; title: string; category: string; reasoning: string; createdAt: string }> }> {
        return this.request('evolution.forged.list');
    }

    /**
     * 删除锻造技能
     */
    async deleteForgedSkill(id: string): Promise<{ success: boolean }> {
        return this.request('evolution.forged.delete', { id });
    }

    /**
     * 监听锻造建议事件
     */
    onForgeSuggestion(callback: (suggestion: { id: string; title: string; content: string; category: string; reasoning: string }) => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'evolution.forge.suggest' && msg.payload) {
                callback(msg.payload as { id: string; title: string; content: string; category: string; reasoning: string });
            }
        });
    }

    /**
     * 监听技能列表变更事件（安装/卸载时自动广播）
     */
    onSkillsUpdated(callback: () => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'evolution.skills.updated') {
                callback();
            }
        });
    }

    // ========================
    // Plan 3: Skill/Tool Event Handlers
    // ========================

    /**
     * 监听工具解锁事件
     */
    onToolUnlocked(callback: (event: ToolUnlockedEvent) => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'tool_unlocked' && msg.payload) {
                callback(this.normalizeToolUnlockedEvent(msg.payload));
            }
        });
    }

    /**
     * 监听技能激活事件
     */
    onSkillActivated(callback: (event: SkillActivatedEvent) => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'skill_activated' && msg.payload) {
                callback(this.normalizeSkillActivatedEvent(msg.payload));
            }
        });
    }

    /**
     * 监听技能切换事件
     */
    onSkillSwitched(callback: (event: SkillSwitchedEvent) => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'skill_switched' && msg.payload) {
                callback(this.normalizeSkillSwitchedEvent(msg.payload));
            }
        });
    }

    /**
     * 监听技能退出事件
     */
    onSkillExited(callback: (event: SkillExitedEvent) => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'skill_exited' && msg.payload) {
                callback(this.normalizeSkillExitedEvent(msg.payload));
            }
        });
    }

    /**
     * 获取当前会话的技能绑定列表
     */
    async getSessionSkillBindings(sessionId?: string): Promise<SkillBindingView[]> {
        const result = await this.request<{ skills?: SkillBindingView[]; bindings?: SkillBindingView[] }>('session.skill.bindings', { sessionId });
        return result.bindings || result.skills || [];
    }

    /**
     * 获取 Agent 的运行态大小写（含 Skill/Tool 信息）
     */
    async getAgentInspect(payload: AgentInspectRequest): Promise<AgentRuntimeSnapshot> {
        return this.request<AgentRuntimeSnapshot>('agent.inspect', payload);
    }

    /**
     * 获取会话的 Token 使用统计
     */
    async getSessionTokenUsage(sessionId: string): Promise<TokenUsageView> {
        const result = await this.request<TokenUsageView | { totalUsage?: TokenUsageView; tokenUsage?: TokenUsageView }>('sessions.token_usage', { sessionId });
        if ('inputTokens' in result && 'outputTokens' in result) {
            return result;
        }
        return result.totalUsage || result.tokenUsage || { inputTokens: 0, outputTokens: 0 };
    }

    onSessionRuntimeUpdated(callback: (payload: Record<string, unknown>) => void): () => void {
        const handler = (msg: GatewayMessage) => {
            if (msg.type === 'session.runtime.updated' && msg.payload) {
                callback(msg.payload as Record<string, unknown>);
            }
        };
        this.addMessageHandler(handler);
        return () => this.removeMessageHandler(handler);
    }

    onSessionTokenUsage(callback: (payload: Record<string, unknown>) => void): () => void {
        const handler = (msg: GatewayMessage) => {
            if (msg.type === 'session.token.usage' && msg.payload) {
                callback(msg.payload as Record<string, unknown>);
            }
        };
        this.addMessageHandler(handler);
        return () => this.removeMessageHandler(handler);
    }

    onSessionToolsUpdated(callback: (payload: Record<string, unknown>) => void): () => void {
        const handler = (msg: GatewayMessage) => {
            if (msg.type === 'session.tools.updated' && msg.payload) {
                callback(msg.payload as Record<string, unknown>);
            }
        };
        this.addMessageHandler(handler);
        return () => this.removeMessageHandler(handler);
    }

    onSessionSkillBindingsUpdated(callback: (payload: Record<string, unknown>) => void): () => void {
        const handler = (msg: GatewayMessage) => {
            if (msg.type === 'session.skill.bindings.updated' && msg.payload) {
                callback(msg.payload as Record<string, unknown>);
            }
        };
        this.addMessageHandler(handler);
        return () => this.removeMessageHandler(handler);
    }

    onSessionMemoryHit(callback: (payload: Record<string, unknown>) => void): () => void {
        const handler = (msg: GatewayMessage) => {
            if (msg.type === 'session.memory.hit' && msg.payload) {
                callback(msg.payload as Record<string, unknown>);
            }
        };
        this.addMessageHandler(handler);
        return () => this.removeMessageHandler(handler);
    }

    // ========================
    // Session Runtime API (Plan 2)
    // ========================

    /**
     * 获取会话的运行时快照（含模型绑定和 token 累计）
     */
    async getSessionRuntime(sessionId: string): Promise<SessionRuntimeSnapshot> {
        return this.request<SessionRuntimeSnapshot>('session.runtime', { sessionId });
    }

    /**
     * 获取会话运行态快照列表（用于会话选择器中显示模型信息）
     */
    async getAllSessionRuntimes(): Promise<SessionRuntimeSnapshot[]> {
        const result = await this.request<{ sessions: SessionRuntimeSnapshot[] }>('session.runtimes');
        return result.sessions || [];
    }

    // ========================
    // Agent Console API (Plan 1)
    // ========================

    /**
     * 获取会话的 Prompt 预览视图
     */
    async getSessionPromptPreview(sessionId: string): Promise<PromptPreviewView> {
        return this.request<PromptPreviewView>('session.prompt.preview', { sessionId });
    }

    /**
     * 获取会话当前可用工具快照
     */
    async getSessionTools(sessionId: string): Promise<ToolDescriptorView[]> {
        const result = await this.request<{ tools: ToolDescriptorView[] }>('session.tools.list', { sessionId });
        return result.tools || [];
    }

    /**
     * 获取会话记忆命中结果
     */
    async getSessionMemoryHits(sessionId: string, turnId?: string): Promise<MemoryHitView[]> {
        const result = await this.request<{ hits: MemoryHitView[] }>('session.memory.hits', { sessionId, turnId });
        return result.hits || [];
    }

    /**
     * 设置会话级模型覆盖
     */
    async setSessionModelOverride(sessionId: string, overrides: {
        orchestration?: { provider: string; model: string };
        execution?: { provider: string; model: string };
    }): Promise<SessionRuntimeSnapshot> {
        return this.request('session.model.override', { sessionId, ...overrides });
    }

    /**
     * 重置会话级模型覆盖
     */
    async resetSessionModelOverride(sessionId: string): Promise<SessionRuntimeSnapshot> {
        return this.request('session.model.override', { sessionId, reset: true });
    }

    /**
     * 获取会话执行历史列表
     */
    async getSessionRuns(sessionId: string, page = 1, pageSize = 20): Promise<{ runs: RunSummaryView[]; total: number }> {
        return this.request('session.runs', { sessionId, page, pageSize });
    }

    /**
     * 获取某次执行的详细步骤信息
     */
    async getRunDetail(runId: string): Promise<RunDetailView> {
        return this.request('run.detail', { runId });
    }

    /**
     * 控制某次执行
     */
    async controlRun(runId: string, action: 'stop' | 'resume_waiting' | 'pause' | 'resume' | 'retry'): Promise<{ success: boolean; run?: RunSummaryView }> {
        return this.request('run.control', { runId, action });
    }

    /**
     * 获取会话级 artifact 列表，可按 run 过滤
     */
    async getSessionArtifacts(sessionId: string, runId?: string): Promise<SessionArtifactView[]> {
        const result = await this.request<{ artifacts?: SessionArtifactView[]; items?: SessionArtifactView[] }>('session.artifacts', { sessionId, runId });
        return result.artifacts || result.items || [];
    }

    /**
     * 获取待确认的权限请求列表
     */
    async getPendingPermissions(sessionId?: string): Promise<PermissionRequestView[]> {
        const result = await this.request<{ requests: PermissionRequestView[] }>('permission.pending', { sessionId });
        return result.requests || [];
    }

    /**
     * 响应权限确认请求
     */
    async respondPermission(
        requestId: string,
        approved: boolean,
        remember = false,
        rememberScope?: 'session' | 'agent' | 'global'
    ): Promise<{ success: boolean; request?: PermissionRequestView }> {
        return this.request('permission.respond', { requestId, approved, remember, rememberScope });
    }

    /**
     * 获取审计日志
     */
    async getAuditLogs(sessionId?: string, type?: string, page = 1, pageSize = 20): Promise<{ logs: AuditLogView[]; total: number }> {
        return this.request('audit.logs', { sessionId, type, page, pageSize });
    }

    /**
     * 获取当前会话诊断摘要
     */
    async getDiagnosticsCurrent(sessionId?: string): Promise<{ issues: DiagnosticIssueView[] }> {
        return this.request('diagnostics.current', { sessionId });
    }

    /**
     * 获取工作区恢复信息
     */
    async getWorkspaceRestore(payload: WorkspaceRestoreRequest = {}): Promise<WorkspaceRestoreView> {
        return this.request('workspace.restore', payload);
    }

    onRunStatusUpdated(callback: (payload: Record<string, unknown>) => void): () => void {
        const handler = (msg: GatewayMessage) => {
            if (msg.type === 'run.status.updated' && msg.payload) {
                callback(msg.payload as Record<string, unknown>);
            }
        };
        this.addMessageHandler(handler);
        return () => this.removeMessageHandler(handler);
    }

    onRunStepUpdated(callback: (payload: Record<string, unknown>) => void): () => void {
        const handler = (msg: GatewayMessage) => {
            if (msg.type === 'run.step.updated' && msg.payload) {
                callback(msg.payload as Record<string, unknown>);
            }
        };
        this.addMessageHandler(handler);
        return () => this.removeMessageHandler(handler);
    }

    onSessionArtifactsUpdated(callback: (payload: Record<string, unknown>) => void): () => void {
        const handler = (msg: GatewayMessage) => {
            if (msg.type === 'session.artifacts.updated' && msg.payload) {
                callback(msg.payload as Record<string, unknown>);
            }
        };
        this.addMessageHandler(handler);
        return () => this.removeMessageHandler(handler);
    }

    onPermissionRequested(callback: (payload: Record<string, unknown>) => void): () => void {
        const handler = (msg: GatewayMessage) => {
            if (msg.type === 'permission.requested' && msg.payload) {
                callback(msg.payload as Record<string, unknown>);
            }
        };
        this.addMessageHandler(handler);
        return () => this.removeMessageHandler(handler);
    }

    onPermissionResolved(callback: (payload: Record<string, unknown>) => void): () => void {
        const handler = (msg: GatewayMessage) => {
            if (msg.type === 'permission.resolved' && msg.payload) {
                callback(msg.payload as Record<string, unknown>);
            }
        };
        this.addMessageHandler(handler);
        return () => this.removeMessageHandler(handler);
    }

    onAuditLogsUpdated(callback: (payload: Record<string, unknown>) => void): () => void {
        const handler = (msg: GatewayMessage) => {
            if (msg.type === 'audit.logs.updated' && msg.payload) {
                callback(msg.payload as Record<string, unknown>);
            }
        };
        this.addMessageHandler(handler);
        return () => this.removeMessageHandler(handler);
    }

    onDiagnosticsUpdated(callback: (payload: Record<string, unknown>) => void): () => void {
        const handler = (msg: GatewayMessage) => {
            if (msg.type === 'diagnostics.updated' && msg.payload) {
                callback(msg.payload as Record<string, unknown>);
            }
        };
        this.addMessageHandler(handler);
        return () => this.removeMessageHandler(handler);
    }

    onWorkspaceRestoreAvailable(callback: (payload: Record<string, unknown>) => void): () => void {
        const handler = (msg: GatewayMessage) => {
            if (msg.type === 'workspace.restore.available' && msg.payload) {
                callback(msg.payload as Record<string, unknown>);
            }
        };
        this.addMessageHandler(handler);
        return () => this.removeMessageHandler(handler);
    }

}

// 全局客户端实例
let gatewayClient: GatewayClient | null = null;

/**
 * 获取或创建 Gateway 客户端
 */
export function getGatewayClient(): GatewayClient | null {
    return gatewayClient;
}

/**
 * 初始化 Gateway 客户端
 */
export async function initGatewayClient(url: string, token?: string): Promise<GatewayClient> {
    if (gatewayClient) {
        gatewayClient.disconnect();
    }
    gatewayClient = new GatewayClient(url, token);
    await gatewayClient.connect();
    return gatewayClient;
}
