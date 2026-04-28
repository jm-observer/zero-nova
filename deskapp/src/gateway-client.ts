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
 * WebSocket 瀹㈡埛绔皝瑁? * 鐢ㄤ簬娓叉煋杩涚▼杩炴帴 Gateway Server
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

interface PendingRequest {
    requestType: string;
    resolve: (value: unknown) => void;
    reject: (error: Error) => void;
}

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
 * Gateway WebSocket 瀹㈡埛绔? */
export class GatewayClient {

    private normalizeAgentRuntimeSnapshot(snapshot: AgentRuntimeSnapshot): AgentRuntimeSnapshot {
        return {
            ...snapshot,
            activeSkills: snapshot.activeSkills ?? [],
            availableTools: snapshot.availableTools ?? [],
            skills: snapshot.skills ?? [],
        };
    }

    private getDefaultVoiceCapabilities(): VoiceCapabilitiesView {
        return {
            stt: { enabled: false, available: false },
            tts: { enabled: false, available: false, voice: '', autoPlay: false },
        };
    }

    private ws: WebSocket | null = null;
    private url: string;
    private token?: string;
    private authenticated = false;
    private pendingRequests = new Map<string, PendingRequest>();
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
            : '璇锋眰澶辫触';

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
     * 杩炴帴鍒?Gateway
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
                        // 棣栨杩炴帴澶辫触鎵?reject
                        reject(new Error('WebSocket 杩炴帴澶辫触'));
                    }
                };

                // 绛夊緟 welcome 娑堟伅
                const welcomeHandler = (msg: GatewayMessage) => {
                    if (msg.type === 'welcome') {
                        this.removeMessageHandler(welcomeHandler);
                        const payload = msg.payload as { requireAuth?: boolean; setupRequired?: boolean };

                        // 淇濆瓨棣栨杩愯鏍囧織
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
     * 璁よ瘉
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
                    reject(new Error('璁よ瘉澶辫触'));
                }
            };
            this.addMessageHandler(authHandler);
            this.send({ type: 'auth', payload: { token: this.token } });
        });
    }

    /**
     * 灏濊瘯閲嶈繛
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
     * 鏂紑杩炴帴
     */
    disconnect(): void {
        this.shouldReconnect = false;
        if (this.ws) {
            this.ws.close();
            this.ws = null;
        }
    }

    /**
     * 閫氱煡杩炴帴鐘舵€佸彉鍖?     */
    private notifyConnectionChange(status: 'connecting' | 'connected' | 'disconnected' | 'reconnecting' | 'failed'): void {
        this.connectionHandlers.forEach(handler => handler(status));
    }

    /**
     * 鐩戝惉杩炴帴鐘舵€佸彉鍖?     */
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
     * 鏄惁宸茶繛鎺?     */
    isConnected(): boolean {
        return this.ws?.readyState === WebSocket.OPEN && this.authenticated;
    }

    /**
     * 鍙戦€佹秷鎭?     */
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

    private warnGatewayError(message: GatewayMessage, requestType?: string): void {
        const payload = (message.payload ?? {}) as { message?: unknown; code?: unknown };
        const errorMessage = typeof payload.message === 'string' ? payload.message : 'Unknown gateway error';
        const errorCode = typeof payload.code === 'string' ? payload.code : 'none';
        const requestInfo = requestType ? ` request=${requestType}` : '';
        const messageId = message.id ?? 'n/a';

        console.warn(
            `[GatewayClient] Gateway warning${requestInfo} id=${messageId} code=${errorCode} type=${message.type}: ${errorMessage}`,
            message.payload,
        );
    }

    /**
     * 澶勭悊鏀跺埌鐨勬秷鎭?     */
    private handleMessage(data: string): void {
        try {
            const message: GatewayMessage = JSON.parse(data);
            const pendingRequest = message.id ? this.pendingRequests.get(message.id) : undefined;
            console.log('[GatewayClient] Message received:', message.type, message.id, message);

            if (message.type === 'error' || message.type.endsWith('.error')) {
                this.warnGatewayError(message, pendingRequest?.requestType);
            }

            // 閫氱煡鎵€鏈夋秷鎭鐞嗗櫒
            this.messageHandlers.forEach(handler => handler(message));

            // 澶勭悊杩涘害浜嬩欢
            if (message.type === 'chat.progress') {
                const event = message.payload as ProgressEvent;
                // ??????????? tool/toolName
                if (event.toolName && !event.tool) event.tool = event.toolName;
                if (!event.toolName && event.tool) event.toolName = event.tool;

                const eventRecord = event as unknown as Record<string, unknown>;
                if (event.toolUseId && !eventRecord.tool_use_id) {
                    eventRecord.tool_use_id = event.toolUseId;
                }
                
                this.progressHandlers.forEach(handler => handler(event));
            }
            
            // 澶勭悊鑱婂ぉ鎰忓悜璇嗗埆浜嬩欢
            if (message.type === 'chat.intent') {
                const payload = message.payload as ChatIntentPayload;
                this.chatIntentHandlers.forEach(handler => handler(payload));
                return;
            }

            // 澶勭悊鑱婂ぉ瀹屾垚浜嬩欢
            if (message.type === 'chat.complete') {
                const payload = message.payload as { output?: string; sessionId?: string; usage?: { input_tokens?: number; output_tokens?: number; cache_creation_input_tokens?: number; cache_read_input_tokens?: number } };
                const completeEvent: ProgressEvent = {
                    type: 'complete',
                    output: payload?.output,
                    sessionId: payload?.sessionId,
                };
                this.progressHandlers.forEach(handler => handler(completeEvent));

                // 鍓嶇 token 绱姞锛氬彂閫?usage 鏇存柊浜嬩欢
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
                    // 閫氱煡鎵€鏈夋秷鎭鐞嗗櫒锛堝寘鎷?AppState锛?                    this.messageHandlers.forEach(handler => handler({ type: 'chat.token_usage', payload: usageUpdate }));
                }
            }

            // 澶勭悊瀹㈡埛绔?MCP 宸ュ叿璋冪敤璇锋眰
            if (message.type === 'mcp.client.call' && message.id) {
                this.handleClientMcpCall(message);
                return; // 涓嶈蛋 pendingRequests 閫昏緫
            }

            // 澶勭悊鍝嶅簲 鈥斺€?鍙銆屾渶缁堛€嶆秷鎭?resolve/reject
            // chat.start / chat.progress / config.progress 鏄腑闂寸姸鎬佹秷鎭紝涓嶅簲瑙﹀彂 resolve
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
     * 娣诲姞娑堟伅澶勭悊鍣?     */
    addMessageHandler(handler: MessageHandler): void {
        this.messageHandlers.push(handler);
    }

    /**
     * 绉婚櫎娑堟伅澶勭悊鍣?     */
    removeMessageHandler(handler: MessageHandler): void {
        const index = this.messageHandlers.indexOf(handler);
        if (index !== -1) {
            this.messageHandlers.splice(index, 1);
        }
    }

    /**
     * 澶勭悊 Gateway 鍙戞潵鐨勫鎴风 MCP 宸ュ叿璋冪敤璇锋眰
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
                payload: { success: false, error: err.message || '?????????' },
            });
        }
    }

    /**
     * 灏嗗鎴风鏈満 MCP 宸ュ叿娉ㄥ唽鍒?Gateway
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
     * 閫氱煡 Gateway 绉婚櫎瀹㈡埛绔?MCP 宸ュ叿
     */
    unregisterClientMcpTools(): void {
        if (!this.isConnected()) return;
        console.log('[GatewayClient] Removing client MCP tools');
        this.send({
            type: 'mcp.client.unregister',
        });
    }

    /**
     * 鐩戝惉杩涘害浜嬩欢
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
     * 鐩戝惉鑱婂ぉ鎰忓悜璇嗗埆浜嬩欢
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
     * 鍙戣捣璇锋眰骞剁瓑寰呭搷搴?     * @param timeout 瓒呮椂姣鏁帮紝0 琛ㄧず涓嶈秴鏃讹紙榛樿 120 绉掞級
     */
    public request<T>(type: string, payload?: unknown, timeout: number = 120000): Promise<T> {
        return new Promise((resolve, reject) => {
            const id = crypto.randomUUID();
            this.pendingRequests.set(id, {
                requestType: type,
                resolve: resolve as (value: unknown) => void,
                reject
            });
            this.send({ type, id, payload });

            // timeout=0 ?????????? chat ??????
            if (timeout > 0) {
                setTimeout(() => {
                    if (this.pendingRequests.has(id)) {
                        this.pendingRequests.delete(id);
                        reject(new Error('璇锋眰瓒呮椂'));
                    }
                }, timeout);
            }
        });
    }

    /**
     * 鍙戦€佽亰澶╂秷鎭紙鏀寔闄勪欢銆佷簯绔?Agent锛?     * 涓嶈瓒呮椂锛欰gent 澶氭鎵ц鍙兘鑰楁椂寰堥暱锛岃繘搴﹂€氳繃 chat.progress 瀹炴椂鎺ㄩ€?     */
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
        try {
            return await this.request<VoiceCapabilitiesView>('voice.capabilities.get', {});
        } catch (error) {
            if (
                error instanceof GatewayRequestError
                && (
                    error.kind === 'unsupported'
                    || error.code === 'not_implemented'
                    || error.message === 'Not implemented'
                )
            ) {
                return this.getDefaultVoiceCapabilities();
            }

            throw error;
        }
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
     * 鍋滄姝ｅ湪鎵ц鐨勪换鍔?     */
    stopTask(sessionId: string): void {
        console.log('[GatewayClient] Stopping task:', sessionId);
        this.send({ type: 'chat.stop', payload: { sessionId } });
    }

    /**
     * 鑾峰彇浼氳瘽鍒楄〃
     */
    async getSessions(): Promise<Session[]> {
        console.log('[GatewayClient] getSessions request');
        const result = await this.request<{ sessions: Session[] }>('sessions.list');
        console.log('[GatewayClient] getSessions response:', result);
        return result.sessions;
    }

    /**
     * 鑾峰彇浼氳瘽娑堟伅
     */
    async getMessages(sessionId: string): Promise<unknown[]> {
        console.log('[GatewayClient] getMessages request:', sessionId);
        const result = await this.request<{ messages: unknown[] }>('sessions.messages', { sessionId });
        console.log('[GatewayClient] getMessages response:', result);
        return result.messages;
    }

    /**
     * 鑾峰彇浼氳瘽鏃ュ織
     */
    async getLogs(sessionId: string): Promise<unknown[]> {
        const result = await this.request<{ logs: unknown[] }>('sessions.logs', { sessionId });
        return result.logs;
    }

    /**
     * 鍒涘缓浼氳瘽
     */
    async createSession(options: { title?: string; agentId?: string; cloudChatroomId?: number; cloudAgentName?: string }): Promise<Session> {
        const result = await this.request<{ session: Session }>('sessions.create', options);
        return result.session;
    }

    /**
     * 鍒犻櫎浼氳瘽
     */
    async deleteSession(sessionId: string): Promise<void> {
        await this.request<{ success: boolean }>('sessions.delete', { sessionId });
    }

    /**
     * 澶嶅埗浼氳瘽
     */
    async copySession(sessionId: string, index?: number): Promise<Session> {
        const result = await this.request<{ session: Session }>('sessions.copy', { sessionId, index });
        return result.session;
    }

    // ========================
    // Agent 绠＄悊 API
    // ========================

    /** 鑾峰彇鎵€鏈夌敤鎴?Agent 鍒楄〃 */
    async getAgents(): Promise<Array<{ id: string; name: string; description?: string; icon?: string; color?: string; default?: boolean; systemPrompt?: string; createdAt: number; updatedAt: number }>> {
        const result = await this.request<{ agents: Array<{ id: string; name: string; description?: string; icon?: string; color?: string; default?: boolean; systemPrompt?: string; createdAt: number; updatedAt: number }> }>('agents.list');
        return result.agents || [];
    }

    /** 鍒涘缓鏂?Agent */
    async createAgent(config: { id: string; name?: string; description?: string; icon?: string; color?: string; systemPrompt?: string }): Promise<Record<string, unknown>> {
        const result = await this.request<{ agent: Record<string, unknown> }>('agents.create', config);
        return result.agent;
    }

    /** 鏇存柊 Agent 閰嶇疆 */
    async updateAgent(agentId: string, updates: Record<string, unknown>): Promise<Record<string, unknown>> {
        const result = await this.request<{ agent: Record<string, unknown> }>('agents.update', { agentId, updates });
        return result.agent;
    }

    /** 鍒犻櫎 Agent */
    async deleteAgent(agentId: string): Promise<boolean> {
        const result = await this.request<{ success: boolean }>('agents.delete', { agentId });
        return result.success;
    }

    /** 鍒囨崲 Agent锛堣繑鍥?Agent 淇℃伅 + 浼氳瘽鍘嗗彶锛?*/
    async switchAgent(agentId: string): Promise<{ agent: Record<string, unknown>; messages: unknown[] }> {
        return this.request<{ agent: Record<string, unknown>; messages: unknown[] }>('agents.switch', { agentId });
    }

    /** 娓呴櫎 Agent 鍘嗗彶娑堟伅 */
    async clearAgentHistory(agentId: string): Promise<boolean> {
        const result = await this.request<{ success: boolean }>('agents.history.clear', { agentId });
        return result.success;
    }

    /**
     * 鐩戝惉 NexusAI 璁よ瘉杩囨湡浜嬩欢锛圓tlas 妯″紡 token 澶辨晥鏃惰Е鍙戯級
     */
    onAuthExpired(handler: (message: string) => void): () => void {
        const messageHandler = (msg: GatewayMessage) => {
            if (msg.type === 'nexusai.auth-expired') {
                const payload = msg.payload as { message?: string };
                handler(payload?.message || 'NexusAI access token ?????????');
            }
        };
        this.addMessageHandler(messageHandler);
        return () => this.removeMessageHandler(messageHandler);
    }

    /**
     * 鐩戝惉浼氳瘽鏇存柊浜嬩欢锛堝畾鏃朵换鍔℃墽琛岀粨鏋滃綊闆嗗埌浼氳瘽鏃惰Е鍙戯級
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
     * 鐩戝惉鍗忎綔瀹屾垚浜嬩欢锛圓gent 闂村崗浣滅粨鏋滈€氱煡锛?     */
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
     * 鑾峰彇璁板繂缁熻淇℃伅
     */
    async memoryStats(): Promise<{ enabled: boolean; totalCount?: number; dbSizeBytes?: number; vectorDim?: number; embeddingModel?: string }> {
        return this.request('memory.stats');
    }

    /**
     * 鍒嗛〉鍒楀嚭璁板繂
     */
    async memoryList(page: number = 1, pageSize: number = 20): Promise<{ items: any[]; total: number; page: number; pageSize: number }> {
        return this.request('memory.list', { page, pageSize });
    }

    /**
     * 鎼滅储璁板繂
     */
    async memorySearch(query: string, limit: number = 10): Promise<{ items: any[] }> {
        return this.request('memory.search', { query, limit });
    }

    /**
     * 鍒犻櫎鍗曟潯璁板繂
     */
    async memoryDelete(id: string): Promise<boolean> {
        const result = await this.request<{ success: boolean }>('memory.delete', { id });
        return result.success;
    }

    /**
     * 娓呯┖鎵€鏈夎蹇?     */
    async memoryClear(): Promise<boolean> {
        const result = await this.request<{ success: boolean }>('memory.clear');
        return result.success;
    }

    // ========================
    // Distillation API
    // ========================

    /**
     * 鑾峰彇钂搁缁熻淇℃伅
     */
    async distillationStats(): Promise<any> {
        return this.request('distillation.stats');
    }

    /**
     * 鑾峰彇鍗＄墖鍏崇郴鍥炬暟鎹?     */
    async distillationGraph(): Promise<{ cards: any[]; relations: any[]; topics: any[] }> {
        return this.request('distillation.graph');
    }

    /**
     * 鏇存柊钂搁閰嶇疆
     */
    async distillationUpdateConfig(config: Record<string, any>): Promise<{ success: boolean; message?: string }> {
        return this.request('distillation.config.update', config);
    }

    /**
     * 鎵嬪姩瑙﹀彂钂搁
     */
    async distillationTrigger(): Promise<{ success: boolean; message?: string }> {
        return this.request('distillation.trigger');
    }

    /**
     * 鑾峰彇鍗＄墖鍒楄〃锛堟敮鎸佸眰绾х瓫閫夊拰鍒嗛〉锛?     */
    async distillationCards(layer?: string, limit = 100, offset = 0): Promise<{ cards: any[]; total: number }> {
        return this.request('distillation.cards', { layer, limit, offset });
    }

    /**
     * 鍒犻櫎鎸囧畾鍗＄墖
     */
    async distillationDeleteCard(cardId: string): Promise<{ success: boolean; message?: string }> {
        return this.request('distillation.card.delete', { cardId });
    }

    // ========================
    // Settings API
    // ========================

    /**
     * 鑾峰彇褰撳墠璁剧疆
     */
    async getSettings(): Promise<{ outputPath: string; defaultOutputPath: string }> {
        return this.request('settings.get');
    }

    /**
     * 鏇存柊璁剧疆锛堜紶 null 閲嶇疆涓洪粯璁ゅ€硷級
     */
    async updateSettings(settings: { outputPath?: string | null }): Promise<{ outputPath: string }> {
        return this.request('settings.update', settings);
    }

    // ========================
    // Server Config API
    // ========================

    /**
     * 鑾峰彇鏈嶅姟绔厤缃?     */
    async getServerConfig(): Promise<ServerConfigView> {
        return this.request('config.get');
    }

    /**
     * 鏇存柊鏈嶅姟绔厤缃?     */
    async updateServerConfig(updates: ServerConfigUpdate): Promise<{ success: boolean; message?: string }> {
        return this.request('config.update', updates);
    }

    isSetupRequired(): boolean {
        return !!(this as unknown as { _setupRequired: boolean })._setupRequired;
    }

    /**
     * 鎻愪氦棣栨鍚姩璁剧疆
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
     * 璁㈤槄 debug 鏃ュ織
     */
    subscribeDebugLog(): void {
        this.send({ type: 'debug.subscribe' });
    }

    /**
     * 鍙栨秷璁㈤槄 debug 鏃ュ織
     */
    unsubscribeDebugLog(): void {
        this.send({ type: 'debug.unsubscribe' });
    }

    /**
     * 鐩戝惉 debug 鏃ュ織浜嬩欢
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
     * 鐩戝惉璁板繂绱㈠紩閲嶅缓杩涘害
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
    // Evolution API (鑷垜杩涘寲)
    // ========================

    /**
     * 鐩戝惉宸ュ叿鍒涘缓纭璇锋眰
     * Gateway 鍦?Agent 鍒涘缓鏂板伐鍏锋椂鎺ㄩ€侊紝鍓嶇寮瑰嚭纭瀵硅瘽妗?     */
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
     * 鍝嶅簲宸ュ叿纭璇锋眰
     */
    respondEvolutionConfirm(requestId: string, approved: boolean): void {
        this.send({
            type: 'evolution.confirm.response',
            payload: { requestId, approved },
        });
    }

    /**
     * 鑾峰彇杩涘寲鏁版嵁缁熻
     */
    async getEvolutionStats(): Promise<{
        schemaVersion: number;
        stats: { installedSkills: number; customTools: number; forgedSkills: number; spawnedAgents: number; mcpConnections: number };
    }> {
        return this.request('evolution.stats');
    }

    /**
     * 鑾峰彇宸插畨瑁呮妧鑳藉垪琛?     */
    async getInstalledSkills(): Promise<{ skills: Array<{ slug: string; source: string; installedAt: string }> }> {
        return this.request('evolution.skills.list');
    }

    /**
     * 鍗歌浇鎶€鑳?     */
    async uninstallSkill(slug: string): Promise<{ success: boolean }> {
        return this.request('evolution.skills.uninstall', { slug });
    }

    /**
     * 鑾峰彇鑷畾涔夊伐鍏峰垪琛?     */
    async getCustomTools(): Promise<{ tools: Array<{ name: string; description: string; scriptType: string; confirmed: boolean; validatorResult: string; createdAt: string }> }> {
        return this.request('evolution.tools.list');
    }

    /**
     * 鍒犻櫎鑷畾涔夊伐鍏?     */
    async deleteCustomTool(name: string): Promise<{ success: boolean }> {
        return this.request('evolution.tools.delete', { name });
    }

    /**
     * 鎺ュ彈閿婚€犲缓璁?     */
    async acceptForgeSuggestion(suggestion: { id: string; title: string; content: string; category: string; reasoning: string }): Promise<{ success: boolean }> {
        return this.request('evolution.forge.accept', suggestion);
    }

    /**
     * 蹇界暐閿婚€犲缓璁?     */
    async dismissForgeSuggestion(): Promise<{ success: boolean }> {
        return this.request('evolution.forge.dismiss');
    }

    /**
     * 鑾峰彇宸查敾閫犳妧鑳藉垪琛?     */
    async getForgedSkills(): Promise<{ skills: Array<{ id: string; title: string; category: string; reasoning: string; createdAt: string }> }> {
        return this.request('evolution.forged.list');
    }

    /**
     * 鍒犻櫎閿婚€犳妧鑳?     */
    async deleteForgedSkill(id: string): Promise<{ success: boolean }> {
        return this.request('evolution.forged.delete', { id });
    }

    /**
     * 鐩戝惉閿婚€犲缓璁簨浠?     */
    onForgeSuggestion(callback: (suggestion: { id: string; title: string; content: string; category: string; reasoning: string }) => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'evolution.forge.suggest' && msg.payload) {
                callback(msg.payload as { id: string; title: string; content: string; category: string; reasoning: string });
            }
        });
    }

    /**
     * 鐩戝惉鎶€鑳藉垪琛ㄥ彉鏇翠簨浠讹紙瀹夎/鍗歌浇鏃惰嚜鍔ㄥ箍鎾級
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
     * 鐩戝惉宸ュ叿瑙ｉ攣浜嬩欢
     */
    onToolUnlocked(callback: (event: ToolUnlockedEvent) => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'tool_unlocked' && msg.payload) {
                callback(this.normalizeToolUnlockedEvent(msg.payload));
            }
        });
    }

    /**
     * 鐩戝惉鎶€鑳芥縺娲讳簨浠?     */
    onSkillActivated(callback: (event: SkillActivatedEvent) => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'skill_activated' && msg.payload) {
                callback(this.normalizeSkillActivatedEvent(msg.payload));
            }
        });
    }

    /**
     * 鐩戝惉鎶€鑳藉垏鎹簨浠?     */
    onSkillSwitched(callback: (event: SkillSwitchedEvent) => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'skill_switched' && msg.payload) {
                callback(this.normalizeSkillSwitchedEvent(msg.payload));
            }
        });
    }

    /**
     * 鐩戝惉鎶€鑳介€€鍑轰簨浠?     */
    onSkillExited(callback: (event: SkillExitedEvent) => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'skill_exited' && msg.payload) {
                callback(this.normalizeSkillExitedEvent(msg.payload));
            }
        });
    }

    /**
     * 鑾峰彇褰撳墠浼氳瘽鐨勬妧鑳界粦瀹氬垪琛?     */
    async getSessionSkillBindings(sessionId?: string): Promise<SkillBindingView[]> {
        const result = await this.request<{ skills?: SkillBindingView[]; bindings?: SkillBindingView[] }>('session.skill.bindings', { sessionId });
        return result.bindings || result.skills || [];
    }

    /**
     * 鑾峰彇 Agent 鐨勮繍琛屾€佸ぇ灏忓啓锛堝惈 Skill/Tool 淇℃伅锛?     */
    async getAgentInspect(payload: AgentInspectRequest): Promise<AgentRuntimeSnapshot> {
        const snapshot = await this.request<AgentRuntimeSnapshot>('agent.inspect', payload);
        return this.normalizeAgentRuntimeSnapshot(snapshot);
    }

    /**
     * 鑾峰彇浼氳瘽鐨?Token 浣跨敤缁熻
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
     * 鑾峰彇浼氳瘽鐨勮繍琛屾椂蹇収锛堝惈妯″瀷缁戝畾鍜?token 绱锛?     */
    async getSessionRuntime(sessionId: string): Promise<SessionRuntimeSnapshot> {
        return this.request<SessionRuntimeSnapshot>('session.runtime', { sessionId });
    }

    /**
     * 鑾峰彇浼氳瘽杩愯鎬佸揩鐓у垪琛紙鐢ㄤ簬浼氳瘽閫夋嫨鍣ㄤ腑鏄剧ず妯″瀷淇℃伅锛?     */
    async getAllSessionRuntimes(): Promise<SessionRuntimeSnapshot[]> {
        const result = await this.request<{ sessions: SessionRuntimeSnapshot[] }>('session.runtimes');
        return result.sessions || [];
    }

    // ========================
    // Agent Console API (Plan 1)
    // ========================

    /**
     * 鑾峰彇浼氳瘽鐨?Prompt 棰勮瑙嗗浘
     */
    async getSessionPromptPreview(sessionId: string): Promise<PromptPreviewView> {
        return this.request<PromptPreviewView>('session.prompt.preview', { sessionId });
    }

    /**
     * 鑾峰彇浼氳瘽褰撳墠鍙敤宸ュ叿蹇収
     */
    async getSessionTools(sessionId: string): Promise<ToolDescriptorView[]> {
        const result = await this.request<{ tools: ToolDescriptorView[] }>('session.tools.list', { sessionId });
        return result.tools || [];
    }

    /**
     * 鑾峰彇浼氳瘽璁板繂鍛戒腑缁撴灉
     */
    async getSessionMemoryHits(sessionId: string, turnId?: string): Promise<MemoryHitView[]> {
        const result = await this.request<{ hits: MemoryHitView[] }>('session.memory.hits', { sessionId, turnId });
        return result.hits || [];
    }

    /**
     * 璁剧疆浼氳瘽绾фā鍨嬭鐩?     */
    async setSessionModelOverride(sessionId: string, overrides: {
        orchestration?: { provider: string; model: string };
        execution?: { provider: string; model: string };
    }): Promise<SessionRuntimeSnapshot> {
        return this.request('session.model.override', { sessionId, ...overrides });
    }

    /**
     * 閲嶇疆浼氳瘽绾фā鍨嬭鐩?     */
    async resetSessionModelOverride(sessionId: string): Promise<SessionRuntimeSnapshot> {
        return this.request('session.model.override', { sessionId, reset: true });
    }

    /**
     * 鑾峰彇浼氳瘽鎵ц鍘嗗彶鍒楄〃
     */
    async getSessionRuns(sessionId: string, page = 1, pageSize = 20): Promise<{ runs: RunSummaryView[]; total: number }> {
        return this.request('session.runs', { sessionId, page, pageSize });
    }

    /**
     * 鑾峰彇鏌愭鎵ц鐨勮缁嗘楠や俊鎭?     */
    async getRunDetail(runId: string): Promise<RunDetailView> {
        return this.request('run.detail', { runId });
    }

    /**
     * 鎺у埗鏌愭鎵ц
     */
    async controlRun(runId: string, action: 'stop' | 'resume_waiting' | 'pause' | 'resume' | 'retry'): Promise<{ success: boolean; run?: RunSummaryView }> {
        return this.request('run.control', { runId, action });
    }

    /**
     * 鑾峰彇浼氳瘽绾?artifact 鍒楄〃锛屽彲鎸?run 杩囨护
     */
    async getSessionArtifacts(sessionId: string, runId?: string): Promise<SessionArtifactView[]> {
        const result = await this.request<{ artifacts?: SessionArtifactView[]; items?: SessionArtifactView[] }>('session.artifacts', { sessionId, runId });
        return result.artifacts || result.items || [];
    }

    /**
     * 鑾峰彇寰呯‘璁ょ殑鏉冮檺璇锋眰鍒楄〃
     */
    async getPendingPermissions(sessionId?: string): Promise<PermissionRequestView[]> {
        const result = await this.request<{ requests: PermissionRequestView[] }>('permission.pending', { sessionId });
        return result.requests || [];
    }

    /**
     * 鍝嶅簲鏉冮檺纭璇锋眰
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
     * 鑾峰彇瀹¤鏃ュ織
     */
    async getAuditLogs(sessionId?: string, type?: string, page = 1, pageSize = 20): Promise<{ logs: AuditLogView[]; total: number }> {
        return this.request('audit.logs', { sessionId, type, page, pageSize });
    }

    /**
     * 鑾峰彇褰撳墠浼氳瘽璇婃柇鎽樿
     */
    async getDiagnosticsCurrent(sessionId?: string): Promise<{ issues: DiagnosticIssueView[] }> {
        return this.request('diagnostics.current', { sessionId });
    }

    /**
     * 鑾峰彇宸ヤ綔鍖烘仮澶嶄俊鎭?     */
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

// ???????
let gatewayClient: GatewayClient | null = null;

/**
 * 鑾峰彇鎴栧垱寤?Gateway 瀹㈡埛绔? */
export function getGatewayClient(): GatewayClient | null {
    return gatewayClient;
}

/**
 * 鍒濆鍖?Gateway 瀹㈡埛绔? */
export async function initGatewayClient(url: string, token?: string): Promise<GatewayClient> {
    if (gatewayClient) {
        gatewayClient.disconnect();
    }
    gatewayClient = new GatewayClient(url, token);
    await gatewayClient.connect();
    return gatewayClient;
}





