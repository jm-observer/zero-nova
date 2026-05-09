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
    SessionFileTreeEntryView,
    ProviderHealthSnapshotView,
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

interface ChatCompleteUsagePayload {
    input_tokens?: number;
    output_tokens?: number;
    cache_creation_input_tokens?: number;
    cache_read_input_tokens?: number;
    inputTokens?: number;
    outputTokens?: number;
    cacheCreationInputTokens?: number;
    cacheReadInputTokens?: number;
}

interface RawRunModelRef {
    provider?: string;
    model?: string;
}

interface RawTurnUsage {
    inputTokens?: number;
    outputTokens?: number;
    cacheCreationInputTokens?: number;
    cacheReadInputTokens?: number;
    input_tokens?: number;
    output_tokens?: number;
    cache_creation_input_tokens?: number;
    cache_read_input_tokens?: number;
}

interface RawProviderHealthSnapshot {
    provider?: string;
    scope?: string;
    status?: string;
    checkedAt?: number;
    checked_at?: number;
    latencyMs?: number | null;
    latency_ms?: number | null;
    message?: string | null;
}

interface RawSessionRuntimeSnapshot {
    sessionId?: string;
    session_id?: string;
    activeAgent?: string;
    active_agent?: string;
    projectDir?: string | null;
    project_dir?: string | null;
    modelOverride?: {
        orchestration?: RawRunModelRef | null;
        execution?: RawRunModelRef | null;
        updatedAt?: number;
        updated_at?: number;
    };
    model_override?: {
        orchestration?: RawRunModelRef | null;
        execution?: RawRunModelRef | null;
        updatedAt?: number;
        updated_at?: number;
    };
    tokenCounters?: RawTurnUsage & { updatedAt?: number; updated_at?: number };
    token_counters?: RawTurnUsage & { updatedAt?: number; updated_at?: number };
    updatedAt?: number;
    updated_at?: number;
    systemPromptState?: {
        version?: string;
        updatedAt?: number;
        updated_at?: number;
        sourceRevision?: string;
        source_revision?: string;
    };
    system_prompt_state?: {
        version?: string;
        updatedAt?: number;
        updated_at?: number;
        sourceRevision?: string;
        source_revision?: string;
    };
}

interface SessionSystemPromptReloadResult {
    sessionId: string;
    versionBefore: string;
    versionAfter: string;
    updatedAt: number;
    changed: boolean;
}

interface RawRunRecord {
    id?: string;
    runId?: string;
    sessionId?: string;
    session_id?: string;
    turnId?: string;
    turn_id?: string;
    agentId?: string;
    agent_id?: string;
    status?: string;
    title?: string;
    startedAt?: number;
    started_at?: number;
    finishedAt?: number;
    finished_at?: number;
    durationMs?: number;
    duration_ms?: number;
    modelSummary?: string;
    orchestrationModel?: RawRunModelRef;
    orchestration_model?: RawRunModelRef;
    executionModel?: RawRunModelRef;
    execution_model?: RawRunModelRef;
    toolCount?: number;
    toolCallCount?: number;
    tool_call_count?: number;
    artifactCount?: number;
    artifact_count?: number;
    usage?: RawTurnUsage;
    tokenUsage?: RawTurnUsage;
    errorSummary?: string;
    error_summary?: string;
    waitingReason?: 'permission' | 'user_input' | 'external_callback' | string;
    waiting_reason?: 'permission' | 'user_input' | 'external_callback' | string;
    steps?: RawRunStepRecord[];
    artifacts?: SessionArtifactView[];
    permissions?: PermissionRequestView[];
    diagnostics?: DiagnosticIssueView[];
    auditLogs?: AuditLogView[];
    audit_logs?: AuditLogView[];
}

interface RawRunStepRecord {
    id?: string;
    stepId?: string;
    step_id?: string;
    runId?: string;
    run_id?: string;
    type?: string;
    stepType?: string;
    step_type?: string;
    title?: string;
    status?: string;
    toolName?: string;
    tool_name?: string;
    startedAt?: number;
    started_at?: number;
    finishedAt?: number;
    finished_at?: number;
    description?: string;
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

/** */
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
    private debugLogSubscribed = false;
    private activeSessionSubscriptionId: string | null = null;

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

    private normalizeRunStatus(status: unknown): RunSummaryView['status'] {
        switch (status) {
            case 'success':
                return 'completed';
            case 'cancelled':
                return 'stopped';
            case 'queued':
            case 'running':
            case 'waiting_user':
            case 'paused':
            case 'stopped':
            case 'failed':
            case 'completed':
                return status;
            default:
                return 'running';
        }
    }

    private buildRunModelSummary(record: RawRunRecord): string | undefined {
        if (typeof record.modelSummary === 'string' && record.modelSummary.trim()) {
            return record.modelSummary;
        }

        const orchestration = record.orchestrationModel ?? record.orchestration_model;
        const execution = record.executionModel ?? record.execution_model;
        const orchestrationModel = orchestration?.model?.trim();
        const executionModel = execution?.model?.trim();

        if (orchestrationModel && executionModel && orchestrationModel !== executionModel) {
            return `${orchestrationModel} / ${executionModel}`;
        }

        return orchestrationModel || executionModel || undefined;
    }

    private normalizeRunUsage(raw: RawTurnUsage | undefined): TokenUsageView | undefined {
        if (!raw) {
            return undefined;
        }

        return {
            inputTokens: raw.inputTokens ?? raw.input_tokens ?? 0,
            outputTokens: raw.outputTokens ?? raw.output_tokens ?? 0,
            cacheCreationInputTokens: raw.cacheCreationInputTokens ?? raw.cache_creation_input_tokens,
            cacheReadInputTokens: raw.cacheReadInputTokens ?? raw.cache_read_input_tokens,
        };
    }

    private normalizeProviderHealthSnapshot(payload: RawProviderHealthSnapshot): ProviderHealthSnapshotView {
        let status = String(payload.status ?? 'unknown') as ProviderHealthSnapshotView['status'];
        let message = typeof payload.message === 'string' ? payload.message : null;

        return {
            provider: String(payload.provider ?? 'unknown'),
            scope: String(payload.scope ?? 'orchestration'),
            status,
            checkedAt: Number(payload.checkedAt ?? payload.checked_at ?? Date.now()),
            latencyMs: typeof (payload.latencyMs ?? payload.latency_ms) === 'number'
                ? Number(payload.latencyMs ?? payload.latency_ms)
                : undefined,
            message,
        };
    }

    private normalizeSessionRuntimeSnapshot(payload: RawSessionRuntimeSnapshot): SessionRuntimeSnapshot {
        const tokenCounters = payload.tokenCounters ?? payload.token_counters;
        const modelOverride = payload.modelOverride ?? payload.model_override;
        const systemPromptState = payload.systemPromptState ?? payload.system_prompt_state;
        const totalUsage = this.normalizeRunUsage(tokenCounters) ?? { inputTokens: 0, outputTokens: 0 };

        return {
            sessionId: String(payload.sessionId ?? payload.session_id ?? ''),
            projectDir: typeof payload.projectDir === 'string'
                ? payload.projectDir
                : (typeof payload.project_dir === 'string' ? payload.project_dir : null),
            modelOverride: modelOverride
                ? {
                    orchestration: modelOverride.orchestration
                        ? {
                            provider: String(modelOverride.orchestration.provider ?? ''),
                            model: String(modelOverride.orchestration.model ?? ''),
                        }
                        : undefined,
                    execution: modelOverride.execution
                        ? {
                            provider: String(modelOverride.execution.provider ?? ''),
                            model: String(modelOverride.execution.model ?? ''),
                        }
                        : undefined,
                }
                : undefined,
            systemPromptState: systemPromptState
                ? {
                    version: String(systemPromptState.version ?? ''),
                    updatedAt: Number(systemPromptState.updatedAt ?? systemPromptState.updated_at ?? 0),
                    sourceRevision: String(systemPromptState.sourceRevision ?? systemPromptState.source_revision ?? ''),
                }
                : undefined,
            totalUsage,
        };
    }

    private normalizeRunStep(step: RawRunStepRecord): RunDetailView['steps'][number] {
        const rawType = step.type ?? step.stepType ?? step.step_type ?? 'system';
        const type = rawType === 'tool_use' ? 'tool' : rawType;
        const rawStatus = step.status ?? 'running';
        const status = rawStatus === 'success' ? 'completed' : rawStatus === 'cancelled' ? 'skipped' : rawStatus;

        return {
            id: String(step.id ?? step.stepId ?? step.step_id ?? ''),
            runId: String(step.runId ?? step.run_id ?? ''),
            type: (['thinking', 'tool', 'approval', 'message', 'artifact', 'system'].includes(type) ? type : 'system') as RunDetailView['steps'][number]['type'],
            title: String(step.title ?? rawType),
            status: (['running', 'completed', 'failed', 'skipped'].includes(status) ? status : 'running') as RunDetailView['steps'][number]['status'],
            startedAt: step.startedAt ?? step.started_at,
            finishedAt: step.finishedAt ?? step.finished_at,
            toolName: step.toolName ?? step.tool_name,
            description: step.description,
        };
    }

    private normalizeRunRecord(record: RawRunRecord): RunSummaryView {
        return {
            id: String(record.id ?? record.runId ?? ''),
            sessionId: String(record.sessionId ?? record.session_id ?? ''),
            turnId: typeof (record.turnId ?? record.turn_id) === 'string' ? String(record.turnId ?? record.turn_id) : undefined,
            agentId: typeof (record.agentId ?? record.agent_id) === 'string' ? String(record.agentId ?? record.agent_id) : undefined,
            status: this.normalizeRunStatus(record.status),
            title: typeof record.title === 'string' ? record.title : undefined,
            startedAt: Number(record.startedAt ?? record.started_at ?? Date.now()),
            finishedAt: typeof (record.finishedAt ?? record.finished_at) === 'number' ? Number(record.finishedAt ?? record.finished_at) : undefined,
            durationMs: typeof (record.durationMs ?? record.duration_ms) === 'number' ? Number(record.durationMs ?? record.duration_ms) : undefined,
            modelSummary: this.buildRunModelSummary(record),
            toolCount: Number(record.toolCount ?? record.toolCallCount ?? record.tool_call_count ?? 0),
            artifactCount: typeof (record.artifactCount ?? record.artifact_count) === 'number' ? Number(record.artifactCount ?? record.artifact_count) : undefined,
            tokenUsage: this.normalizeRunUsage(record.tokenUsage ?? record.usage),
            errorSummary: typeof (record.errorSummary ?? record.error_summary) === 'string' ? String(record.errorSummary ?? record.error_summary) : undefined,
            waitingReason: (record.waitingReason ?? record.waiting_reason) as RunSummaryView['waitingReason'],
        };
    }

    private normalizeRunDetail(record: RawRunRecord): RunDetailView {
        return {
            ...this.normalizeRunRecord(record),
            steps: Array.isArray(record.steps) ? record.steps.map(step => this.normalizeRunStep(step)) : [],
            artifacts: record.artifacts ?? [],
            permissions: record.permissions ?? [],
            diagnostics: record.diagnostics ?? [],
            auditLogs: record.auditLogs ?? record.audit_logs ?? [],
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
                        reject(new Error('WebSocket 连接失败'));
                    }
                };

                const welcomeHandler = (msg: GatewayMessage) => {
                    if (msg.type === 'welcome') {
                        this.removeMessageHandler(welcomeHandler);
                        const payload = msg.payload as { requireAuth?: boolean; setupRequired?: boolean };

                        if (payload.setupRequired) {
                        (this as unknown as { _setupRequired: boolean })._setupRequired = true;
                    }

                        if (payload.requireAuth && this.token) {
                            this.authenticate().then(() => {
                                this.restoreSubscriptionsAfterReconnect();
                                this.notifyConnectionChange('connected');
                                resolve();
                            }).catch(reject);
                        } else {
                            this.authenticated = true;
                            this.restoreSubscriptionsAfterReconnect();
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
     */
    disconnect(): void {
        this.shouldReconnect = false;
        if (this.ws) {
            this.ws.close();
            this.ws = null;
        }
    }

    /** */
    private notifyConnectionChange(status: 'connecting' | 'connected' | 'disconnected' | 'reconnecting' | 'failed'): void {
        this.connectionHandlers.forEach(handler => handler(status));
    }

    private restoreSubscriptionsAfterReconnect(): void {
        console.log('[GatewayClient] Restoring subscriptions after reconnect', {
            debugLogSubscribed: this.debugLogSubscribed,
        });
        if (this.debugLogSubscribed) {
            console.log('[GatewayClient] Re-subscribing debug log stream');
            this.send({ type: 'debug.subscribe' });
        }
        if (this.activeSessionSubscriptionId) {
            const sessionId = this.activeSessionSubscriptionId;
            void this.request('sessions.messages', { sessionId }).catch((error) => {
                console.warn('[GatewayClient] Failed to restore session subscription:', sessionId, error);
            });
        }
    }

    /** */
    onConnectionChange(handler: ConnectionHandler): () => void {
        this.connectionHandlers.push(handler);
        return () => {
            const index = this.connectionHandlers.indexOf(handler);
            if (index !== -1) {
                this.connectionHandlers.splice(index, 1);
            }
        };
    }

    /** */
    isConnected(): boolean {
        return this.ws?.readyState === WebSocket.OPEN && this.authenticated;
    }

    /** */
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

    private trackSessionSubscription(type: string, payload: unknown): void {
        if (!this.isSessionSubscriptionRequest(type) || !payload || typeof payload !== 'object') {
            return;
        }

        const sessionId = (payload as Record<string, unknown>).sessionId;
        if (typeof sessionId === 'string' && sessionId.length > 0) {
            this.activeSessionSubscriptionId = sessionId;
        }
    }

    private isSessionSubscriptionRequest(type: string): boolean {
        return type === 'chat' || type.startsWith('session.') || type === 'sessions.messages';
    }

    /**
     * 处理收到的消息
     */
    private handleMessage(data: string): void {
        try {
            const message: GatewayMessage = JSON.parse(data);
            const pendingRequest = message.id ? this.pendingRequests.get(message.id) : undefined;
            console.log('[GatewayClient] Message received:', message.type, message.id, message);

            if (message.type === 'error' || message.type.endsWith('.error')) {
                this.warnGatewayError(message, pendingRequest?.requestType);
            }

            // 通知所有消息处理器
            this.messageHandlers.forEach(handler => handler(message));

            // 处理进度事件
            if (message.type === 'chat.progress') {
                const event = message.payload as ProgressEvent;
                // 兼容 tool / toolName 字段
                if (event.toolName && !event.tool) event.tool = event.toolName;
                if (!event.toolName && event.tool) event.toolName = event.tool;

                const eventRecord = event as unknown as Record<string, unknown>;
                if (typeof eventRecord.tool_name === 'string' && !event.toolName) {
                    event.toolName = eventRecord.tool_name;
                    event.tool = eventRecord.tool_name;
                }
                if (typeof eventRecord.session_id === 'string' && !event.sessionId) {
                    event.sessionId = eventRecord.session_id;
                }
                if (event.toolUseId && !eventRecord.tool_use_id) {
                    eventRecord.tool_use_id = event.toolUseId;
                }
                if (typeof eventRecord.tool_use_id === 'string' && !event.toolUseId) {
                    event.toolUseId = eventRecord.tool_use_id;
                }
                
                this.progressHandlers.forEach(handler => handler(event));
            }
            
            // 处理聊天意图识别事件
            if (message.type === 'chat.intent') {
                const payload = message.payload as ChatIntentPayload;
                this.chatIntentHandlers.forEach(handler => handler(payload));
                return;
            }

            // 处理聊天完成事件
            if (message.type === 'chat.complete') {
                const payload = message.payload as { output?: string; sessionId?: string; usage?: ChatCompleteUsagePayload };
                const completeEvent: ProgressEvent = {
                    type: 'complete',
                    output: payload?.output,
                    sessionId: payload?.sessionId,
                };
                const usage = this.normalizeChatCompleteUsage(payload?.usage);
                if (usage) {
                    (completeEvent as ProgressEvent & { usage?: TokenUsageView }).usage = usage;
                }
                this.progressHandlers.forEach(handler => handler(completeEvent));

                // 前端 token 累加：发出 usage 更新事件
                if (usage && payload?.sessionId) {
                    const usageUpdate = {
                        sessionId: payload.sessionId,
                        usage,
                    };
                    // 通知所有消息处理器（包括 AppState）
                    this.messageHandlers.forEach(handler => handler({ type: 'chat.token_usage', payload: usageUpdate }));
                }
            }

            // 处理停止响应事件
            if (message.type === 'chat.stop.response') {
                const payload = message.payload as { sessionId: string };
                this.messageHandlers.forEach(handler => handler({ type: 'chat.stop.response', payload }));
            }

            // 处理客户端 MCP 工具调用请求
            if (message.type === 'mcp.client.call' && message.id) {
                this.handleClientMcpCall(message);
                return; // 不走 pendingRequests 逻辑
            }

            // 处理响应：只对最终消息 resolve/reject
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

    private normalizeChatCompleteUsage(usage?: ChatCompleteUsagePayload): TokenUsageView | undefined {
        if (!usage) {
            return undefined;
        }
        return {
            inputTokens: usage.inputTokens ?? usage.input_tokens ?? 0,
            outputTokens: usage.outputTokens ?? usage.output_tokens ?? 0,
            cacheCreationInputTokens: usage.cacheCreationInputTokens ?? usage.cache_creation_input_tokens,
            cacheReadInputTokens: usage.cacheReadInputTokens ?? usage.cache_read_input_tokens,
        };
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
                payload: { success: false, error: err.message || 'MCP client tool call failed' },
            });
        }
    }

    /**
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
     */
    unregisterClientMcpTools(): void {
        if (!this.isConnected()) return;
        console.log('[GatewayClient] Removing client MCP tools');
        this.send({
            type: 'mcp.client.unregister',
        });
    }

    /**
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
            this.trackSessionSubscription(type, payload);
            this.pendingRequests.set(id, {
                requestType: type,
                resolve: resolve as (value: unknown) => void,
                reject
            });
            this.send({ type, id, payload });

            // timeout=0 表示不超时，主要用于 chat 请求
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

    /** */
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

    /** */
    stopTask(sessionId: string): void {
        console.log('[GatewayClient] Stopping task:', sessionId);
        this.send({ type: 'chat.stop', payload: { sessionId } });
    }

    /**
     */
    async getSessions(): Promise<Session[]> {
        console.log('[GatewayClient] getSessions request');
        const result = await this.request<{ sessions: Session[] }>('sessions.list');
        console.log('[GatewayClient] getSessions response:', result);
        return result.sessions;
    }

    /**
     */
    async getMessages(sessionId: string): Promise<unknown[]> {
        console.log('[GatewayClient] getMessages request:', sessionId);
        const result = await this.request<{ messages: unknown[] }>('sessions.messages', { sessionId });
        console.log('[GatewayClient] getMessages response:', result);
        return result.messages;
    }

    /**
     */
    async getLogs(sessionId: string): Promise<unknown[]> {
        const result = await this.request<{ logs: unknown[] }>('sessions.logs', { sessionId });
        return result.logs;
    }

    /**
     */
    async createSession(options: { title?: string; agentId?: string; cloudChatroomId?: number; cloudAgentName?: string }): Promise<Session> {
        const result = await this.request<{ session: Session }>('sessions.create', options);
        return result.session;
    }

    /**
     */
    async deleteSession(sessionId: string): Promise<void> {
        await this.request<{ success: boolean }>('sessions.delete', { sessionId });
    }

    /**
     */
    async copySession(sessionId: string, index?: number): Promise<Session> {
        const result = await this.request<{ session: Session }>('sessions.copy', { sessionId, index });
        return result.session;
    }

    // ========================
    // Agent 绠＄悊 API
    // ========================

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

    async switchAgent(agentId: string): Promise<{ agent: Record<string, unknown>; messages: unknown[] }> {
        return this.request<{ agent: Record<string, unknown>; messages: unknown[] }>('agents.switch', { agentId });
    }

    async clearAgentHistory(agentId: string): Promise<boolean> {
        const result = await this.request<{ success: boolean }>('agents.history.clear', { agentId });
        return result.success;
    }

    /**
     */
    onAuthExpired(handler: (message: string) => void): () => void {
        const messageHandler = (msg: GatewayMessage) => {
            if (msg.type === 'nexusai.auth-expired') {
                const payload = msg.payload as { message?: string };
                handler(payload?.message || 'NexusAI access token expired');
            }
        };
        this.addMessageHandler(messageHandler);
        return () => this.removeMessageHandler(messageHandler);
    }

    /**
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

    /** */
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
     */
    async memoryStats(): Promise<{ enabled: boolean; totalCount?: number; dbSizeBytes?: number; vectorDim?: number; embeddingModel?: string }> {
        return this.request('memory.stats');
    }

    /**
     */
    async memoryList(page: number = 1, pageSize: number = 20): Promise<{ items: any[]; total: number; page: number; pageSize: number }> {
        return this.request('memory.list', { page, pageSize });
    }

    /**
     */
    async memorySearch(query: string, limit: number = 10): Promise<{ items: any[] }> {
        return this.request('memory.search', { query, limit });
    }

    /**
     */
    async memoryDelete(id: string): Promise<boolean> {
        const result = await this.request<{ success: boolean }>('memory.delete', { id });
        return result.success;
    }

    /** */
    async memoryClear(): Promise<boolean> {
        const result = await this.request<{ success: boolean }>('memory.clear');
        return result.success;
    }

    // ========================
    // Distillation API
    // ========================

    /**
     */
    async distillationStats(): Promise<any> {
        return this.request('distillation.stats');
    }

    /** */
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
     */
    async distillationTrigger(): Promise<{ success: boolean; message?: string }> {
        return this.request('distillation.trigger');
    }

    /** */
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
     */
    async getSettings(): Promise<{ outputPath: string; defaultOutputPath: string }> {
        return this.request('settings.get');
    }

    /**
     */
    async updateSettings(settings: { outputPath?: string | null }): Promise<{ outputPath: string }> {
        return this.request('settings.update', settings);
    }

    // ========================
    // Server Config API
    // ========================

    /** */
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
     */
    subscribeDebugLog(): void {
        this.debugLogSubscribed = true;
        this.send({ type: 'debug.subscribe' });
    }

    /**
     */
    unsubscribeDebugLog(): void {
        this.debugLogSubscribed = false;
        this.send({ type: 'debug.unsubscribe' });
    }

    /**
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
    // ========================

    /** */
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
     */
    respondEvolutionConfirm(requestId: string, approved: boolean): void {
        this.send({
            type: 'evolution.confirm.response',
            payload: { requestId, approved },
        });
    }

    /**
     */
    async getEvolutionStats(): Promise<{
        schemaVersion: number;
        stats: { installedSkills: number; customTools: number; forgedSkills: number; spawnedAgents: number; mcpConnections: number };
    }> {
        return this.request('evolution.stats');
    }

    /** */
    async getInstalledSkills(): Promise<{ skills: Array<{ slug: string; source: string; installedAt: string }> }> {
        return this.request('evolution.skills.list');
    }

    /** */
    async uninstallSkill(slug: string): Promise<{ success: boolean }> {
        return this.request('evolution.skills.uninstall', { slug });
    }

    /** */
    async getCustomTools(): Promise<{ tools: Array<{ name: string; description: string; scriptType: string; confirmed: boolean; validatorResult: string; createdAt: string }> }> {
        return this.request('evolution.tools.list');
    }

    /**
     * 鍒犻櫎鑷畾涔夊伐鍏?     */
    async deleteCustomTool(name: string): Promise<{ success: boolean }> {
        return this.request('evolution.tools.delete', { name });
    }

    /** */
    async acceptForgeSuggestion(suggestion: { id: string; title: string; content: string; category: string; reasoning: string }): Promise<{ success: boolean }> {
        return this.request('evolution.forge.accept', suggestion);
    }

    /** */
    async dismissForgeSuggestion(): Promise<{ success: boolean }> {
        return this.request('evolution.forge.dismiss');
    }

    /** */
    async getForgedSkills(): Promise<{ skills: Array<{ id: string; title: string; category: string; reasoning: string; createdAt: string }> }> {
        return this.request('evolution.forged.list');
    }

    /** */
    async deleteForgedSkill(id: string): Promise<{ success: boolean }> {
        return this.request('evolution.forged.delete', { id });
    }

    /** */
    onForgeSuggestion(callback: (suggestion: { id: string; title: string; content: string; category: string; reasoning: string }) => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'evolution.forge.suggest' && msg.payload) {
                callback(msg.payload as { id: string; title: string; content: string; category: string; reasoning: string });
            }
        });
    }

    /**
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
     */
    onToolUnlocked(callback: (event: ToolUnlockedEvent) => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'tool_unlocked' && msg.payload) {
                callback(this.normalizeToolUnlockedEvent(msg.payload));
            }
        });
    }

    /** */
    onSkillActivated(callback: (event: SkillActivatedEvent) => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'skill_activated' && msg.payload) {
                callback(this.normalizeSkillActivatedEvent(msg.payload));
            }
        });
    }

    /** */
    onSkillSwitched(callback: (event: SkillSwitchedEvent) => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'skill_switched' && msg.payload) {
                callback(this.normalizeSkillSwitchedEvent(msg.payload));
            }
        });
    }

    /** */
    onSkillExited(callback: (event: SkillExitedEvent) => void): void {
        this.addMessageHandler((msg: GatewayMessage) => {
            if (msg.type === 'skill_exited' && msg.payload) {
                callback(this.normalizeSkillExitedEvent(msg.payload));
            }
        });
    }

    /** */
    async getSessionSkillBindings(sessionId?: string): Promise<SkillBindingView[]> {
        const result = await this.request<{ skills?: unknown[]; bindings?: unknown[] }>('session.skill.bindings', { sessionId });
        const rawBindings = result.bindings || result.skills || [];
        return rawBindings.map((entry) => this.normalizeSkillBinding(entry));
    }

    private normalizeSkillBinding(payload: unknown): SkillBindingView {
        const record = (payload ?? {}) as Record<string, unknown>;
        const id = String(record.skillId ?? record.skill_id ?? record.id ?? '');
        const title = String(record.name ?? record.display_name ?? record.title ?? id);
        const status = String(record.status ?? '').toLowerCase();
        const source = this.normalizeSkillBindingSource(record.source);

        return {
            id,
            title,
            source,
            enabled: status === 'active' || status === 'bound' || status === 'available',
            summary: typeof record.description === 'string' ? record.description : undefined,
        };
    }

    private normalizeSkillBindingSource(source: unknown): SkillBindingView['source'] {
        if (source === 'global' || source === 'agent' || source === 'runtime') {
            return source;
        }
        return 'runtime';
    }

    /** */
    async getAgentInspect(payload: AgentInspectRequest): Promise<AgentRuntimeSnapshot> {
        const snapshot = await this.request<AgentRuntimeSnapshot>('agent.inspect', payload);
        return this.normalizeAgentRuntimeSnapshot(snapshot);
    }

    /**
     */
    async getSessionTokenUsage(sessionId: string): Promise<TokenUsageView> {
        const result = await this.request<TokenUsageView | { totalUsage?: TokenUsageView; tokenUsage?: TokenUsageView }>('sessions.token_usage', { sessionId });
        if ('inputTokens' in result && 'outputTokens' in result) {
            return result;
        }
        return result.totalUsage || result.tokenUsage || { inputTokens: 0, outputTokens: 0 };
    }

    async getProviderHealth(): Promise<ProviderHealthSnapshotView[]> {
        const result = await this.request<{ providers?: RawProviderHealthSnapshot[] }>('provider.health', {});
        return (result.providers ?? []).map((entry) => this.normalizeProviderHealthSnapshot(entry));
    }

    onProviderHealthUpdated(callback: (providers: ProviderHealthSnapshotView[]) => void): () => void {
        const handler = (msg: GatewayMessage) => {
            if (msg.type === 'provider.health.updated' && msg.payload) {
                callback([this.normalizeProviderHealthSnapshot(msg.payload as RawProviderHealthSnapshot)]);
                return;
            }

            if (msg.type === 'provider.health.response' && msg.payload) {
                const payload = msg.payload as { providers?: RawProviderHealthSnapshot[] };
                callback((payload.providers ?? []).map((entry) => this.normalizeProviderHealthSnapshot(entry)));
            }
        };
        this.addMessageHandler(handler);
        return () => this.removeMessageHandler(handler);
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
            if ((msg.type === 'session.token.usage.updated' || msg.type === 'session.token.usage') && msg.payload) {
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

    /** */
    async getSessionRuntime(sessionId: string): Promise<SessionRuntimeSnapshot> {
        const payload = await this.request<RawSessionRuntimeSnapshot>('session.runtime', { sessionId });
        return this.normalizeSessionRuntimeSnapshot(payload);
    }

    /** */
    async getAllSessionRuntimes(): Promise<SessionRuntimeSnapshot[]> {
        const result = await this.request<{ sessions: RawSessionRuntimeSnapshot[] }>('session.runtimes');
        return (result.sessions || []).map((entry) => this.normalizeSessionRuntimeSnapshot(entry));
    }

    async listSessionFileTree(sessionId: string, relativePath?: string): Promise<SessionFileTreeEntryView[]> {
        const result = await this.request<{ entries?: SessionFileTreeEntryView[] }>('session.file_tree.list', {
            sessionId,
            relativePath: relativePath || undefined,
        });
        return result.entries || [];
    }

    // ========================
    // Agent Console API (Plan 1)
    // ========================

    /**
     */
    async getSessionPromptPreview(sessionId: string): Promise<PromptPreviewView> {
        return this.request<PromptPreviewView>('session.prompt.preview', { sessionId });
    }

    async reloadSessionSystemPrompt(sessionId: string): Promise<SessionSystemPromptReloadResult> {
        return this.request<SessionSystemPromptReloadResult>('session.system_prompt.reload', { sessionId });
    }

    /**
     */
    async getSessionTools(sessionId: string): Promise<ToolDescriptorView[]> {
        const result = await this.request<{ tools: ToolDescriptorView[] }>('session.tools.list', { sessionId });
        return result.tools || [];
    }

    /**
     */
    async getSessionMemoryHits(sessionId: string, turnId?: string): Promise<MemoryHitView[]> {
        const result = await this.request<{ hits: MemoryHitView[] }>('session.memory.hits', { sessionId, turnId });
        return result.hits || [];
    }

    /** */
    async setSessionModelOverride(sessionId: string, overrides: {
        orchestration?: { provider: string; model: string };
        execution?: { provider: string; model: string };
    }): Promise<SessionRuntimeSnapshot> {
        const payload = await this.request<RawSessionRuntimeSnapshot>('session.model.override', { sessionId, ...overrides });
        return this.normalizeSessionRuntimeSnapshot(payload);
    }

    /** */
    async resetSessionModelOverride(sessionId: string): Promise<SessionRuntimeSnapshot> {
        const payload = await this.request<RawSessionRuntimeSnapshot>('session.model.override', { sessionId, reset: true });
        return this.normalizeSessionRuntimeSnapshot(payload);
    }

    /**
     */
    async getSessionRuns(sessionId: string, page = 1, pageSize = 20): Promise<{ runs: RunSummaryView[]; total: number }> {
        const result = await this.request<{ runs?: RawRunRecord[]; total?: number }>('session.runs', { sessionId, page, pageSize });
        const runs = (result.runs ?? []).map(run => this.normalizeRunRecord(run));
        return {
            runs,
            total: result.total ?? runs.length,
        };
    }

    /** */
    async getRunDetail(runId: string): Promise<RunDetailView> {
        const result = await this.request<RawRunRecord>('run.detail', { runId });
        return this.normalizeRunDetail(result);
    }

    /**
     */
    async controlRun(runId: string, action: 'stop' | 'resume_waiting' | 'pause' | 'resume' | 'retry'): Promise<{ success: boolean; run?: RunSummaryView }> {
        const result = await this.request<{ success: boolean; run?: RawRunRecord }>('run.control', { runId, action });
        return {
            success: result.success,
            run: result.run ? this.normalizeRunRecord(result.run) : undefined,
        };
    }

    /**
     */
    async getSessionArtifacts(sessionId: string, runId?: string): Promise<SessionArtifactView[]> {
        const result = await this.request<{ artifacts?: SessionArtifactView[]; items?: SessionArtifactView[] }>('session.artifacts', { sessionId, runId });
        return result.artifacts || result.items || [];
    }

    /**
     */
    async getPendingPermissions(sessionId?: string): Promise<PermissionRequestView[]> {
        const result = await this.request<{ requests: PermissionRequestView[] }>('permission.pending', { sessionId });
        return result.requests || [];
    }

    /**
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
     */
    async getAuditLogs(sessionId?: string, type?: string, page = 1, pageSize = 20): Promise<{ logs: AuditLogView[]; total: number }> {
        return this.request('audit.logs', { sessionId, type, page, pageSize });
    }

    /**
     */
    async getDiagnosticsCurrent(sessionId?: string): Promise<{ issues: DiagnosticIssueView[] }> {
        return this.request('diagnostics.current', { sessionId });
    }

    /** */
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

    /**
     * 监听会话标题更新事件 (session.summary.updated)
     * 用于 Plan 2 前端标题同步链路
     */
    onSessionSummaryUpdated(callback: (payload: { sessionId: string; title: string; updatedAt?: number; messageCount?: number; agentId?: string }) => void): () => void {
        const handler = (msg: GatewayMessage) => {
            if (msg.type === 'session.summary.updated' && msg.payload) {
                const payload = msg.payload as { sessionId: string; title: string; updatedAt?: number; messageCount?: number; agentId?: string };
                callback(payload);
            }
        };
        this.addMessageHandler(handler);
        return () => this.removeMessageHandler(handler);
    }

}

// 单例实例
let gatewayClient: GatewayClient | null = null;

/** */
export function getGatewayClient(): GatewayClient | null {
    return gatewayClient;
}

/** */
export async function initGatewayClient(url: string, token?: string): Promise<GatewayClient> {
    if (gatewayClient) {
        gatewayClient.disconnect();
    }
    gatewayClient = new GatewayClient(url, token);
    await gatewayClient.connect();
    return gatewayClient;
}






