/**
 * 集中存放所有共享的类型定义
 */

// 从 main.ts 提取的类型
export interface MessageAttachment {
    name: string;
    ext: string;
    size: number;
    path?: string;          // 文件路径（用于预览/打开）
    thumbnailUrl?: string;  // 图片缩略图（仅用于UI显示）
}

export interface Message {
    id: string;
    role: 'user' | 'assistant';
    content: string;
    createdAt: number;
    toolCalls?: ToolCall[];
    attachments?: MessageAttachment[];
    metadata?: Record<string, unknown>;
}

export interface ToolCall {
    name: string;
    args: Record<string, unknown>;
    result?: string;
}

export interface LogEntry {
    id: string;
    timestamp: number;
    tool: string;
    action?: string;
    args?: Record<string, unknown>;
    success: boolean;
    result?: unknown;
    resultSummary?: string;
}

export interface Session {
    id: string;
    agentId: string;
    title: string;
    createdAt: number;
    updatedAt?: number;
    lastMessagePreview?: string;
    cloudChatroomId?: number;
    cloudAgentName?: string;
}

export interface PendingAttachment {
    path: string;
    name: string;
    size: number;
    ext: string;        // 小写扩展名，如.xlsx
    type: 'image' | 'document' | 'text';
    thumbnailUrl?: string;  // 图片缩略URL（通过 URL.createObjectURL 生成）
}

export type WorkingMode = 'standalone' | 'router' | 'managed';

// 从 gateway-client.ts 重新导出的类型或直接使用
// 注意：为了避免循环依赖，这里只放纯 Interface/Type，不放 Class 实现

export interface ProgressEvent {
    type: 'iteration' | 'thinking' | 'tool_start' | 'tool_result' | 'token' | 'complete' | 'turn_complete' | 'iteration_limit' | 'tool_log' | 'system_log';
    iteration?: number;
    tool?: string;
    toolName?: string;
    toolUseId?: string;
    args?: Record<string, unknown>;
    result?: unknown;
    thinking?: string;
    token?: string;
    output?: string;
    description?: string;
    isError?: boolean;
    llmDescription?: string;
    sessionId?: string;
    log?: string;
    stream?: string;
}

export interface ChatIntentPayload {
    sessionId: string;
    intent: 'chat' | 'resolve' | 'address_agent' | 'continue_workflow';
    agentId?: string;
}

// ... 其他从 main.ts 发现的类型
export interface AgentModelItem {
    id: string;
    name: string;
    description?: string;
    icon?: string;
    color?: string;
    default?: boolean;
    systemPrompt?: string;
    createdAt: number;
    updatedAt: number;
}

export interface SkillItem {
    id: string;
    title: string;
    content: string;
    enabled: boolean;
}

export interface SessionProgressState {
    // 根据需要补充
    [key: string]: any;
}

export interface OpenFluxAgentInfo {
    agentId: number;
    appId: number;
    name: string;
    description?: string;
    chatroomId: number;
    avatar?: string;
}

export interface OpenFluxChatMessage {
    role: 'user' | 'assistant';
    content: string;
    createdAt: number;
    agentName?: string;
}

export interface RouterConfigView {
    url: string;
    appId: string;
    appType: string;
    apiKey: string;
    appUserId: string;
    enabled: boolean;
}

export interface RouterInboundView {
    id: string;
    platform_type: string;
    platform_id: string;
    platform_user_id: string;
    app_type: string;
    app_id: string;
    app_user_id?: string;
    direction: 'inbound';
    content_type: string;
    content: string;
    metadata?: Record<string, unknown>;
    timestamp: number;
}

export interface RouterOutboundView {
    platform_type: string;
    platform_id: string;
    platform_user_id: string;
    content_type: string;
    content: string;
}

export interface EvolutionConfirmRequest {
    requestId: string;
    toolName: string;
    description: string;
    confirmMessage: string;
    validationStatus: 'PASS' | 'WARN' | 'BLOCK';
}

export interface DebugLogEntry {
    timestamp: string;
    level: 'info' | 'warn' | 'error' | 'debug';
    module: string;
    message: string;
    meta?: Record<string, unknown>;
}

export interface SchedulerEventView {
    type: string;
    taskId: string;
    taskName?: string;
    runId?: string;
    error?: string;
    timestamp: number;
}

export interface SessionArtifactView {
    id: string;
    type: 'file' | 'code' | 'output' | 'image';
    path?: string;
    filename?: string;
    title?: string;
    content?: string;
    language?: string;
    size?: number;
    timestamp: number;
    runId?: string;      // 来源任务 ID
    turnId?: string;     // 来源轮次 ID
    stepId?: string;
}

export interface McpServerView {
    name: string;
    location?: 'server' | 'client';
    transport: 'stdio' | 'sse';
    command?: string;
    args?: string[];
    url?: string;
    env?: Record<string, string>;
    enabled?: boolean;
    toolCount?: number;
    status?: 'connected' | 'disconnected' | 'error';
}

export interface ServerConfigView {
    providers: Record<string, { apiKey?: string; baseUrl?: string }>;
    llm: {
        orchestration: { provider: string; model: string };
        execution: { provider: string; model: string };
        embedding?: { provider: string; model: string };
        fallback?: { provider: string; model: string };
    };
    web?: {
        search?: { provider?: string; apiKey?: string; maxResults?: number };
        fetch?: { readability?: boolean; maxChars?: number };
    };
    mcp?: {
        servers?: McpServerView[];
    };
    gatewayMode: 'embedded' | 'remote';
    gatewayPort: number;
    agents?: {
        globalAgentName?: string;
        globalSystemPrompt?: string;
        skills?: Array<{ id: string; title: string; content: string; enabled: boolean }>;
        list?: Array<{ id: string; name: string; description?: string; model?: { provider: string; model: string } }>;
    };
    sandbox?: {
        mode?: string;
        docker?: {
            image?: string;
            memoryLimit?: string;
            cpuLimit?: string;
            networkMode?: string;
        };
        blockedExtensions?: string[];
    };
    presetModels?: Record<string, { value: string; label: string; multimodal?: boolean }[]>;
}

export interface ServerConfigUpdate {
    providers?: Record<string, { apiKey?: string; baseUrl?: string }>;
    orchestration?: { provider?: string; model?: string };
    execution?: { provider?: string; model?: string };
    embedding?: { provider?: string; model?: string };
    web?: {
        search?: { provider?: string; apiKey?: string; maxResults?: number };
        fetch?: { readability?: boolean; maxChars?: number };
    };
    mcp?: {
        servers?: Array<{
            name: string;
            location?: 'server' | 'client';
            transport: 'stdio' | 'sse';
            command?: string;
            args?: string[];
            url?: string;
            env?: Record<string, string>;
            enabled?: boolean;
        }>;
    };
    agents?: {
        globalAgentName?: string;
        globalSystemPrompt?: string;
        skills?: Array<{ id: string; title: string; content: string; enabled: boolean }>;
        list?: Array<{ id: string; model?: { provider: string; model: string } | null }>;
    };
    sandbox?: {
        mode?: string;
        docker?: {
            image?: string;
            memoryLimit?: string;
            cpuLimit?: string;
            networkMode?: string;
        };
        blockedExtensions?: string[];
    };
}

export interface ScheduledTaskView {
    id: string;
    name: string;
    trigger: {
        type: 'cron' | 'interval' | 'once';
        expression?: string;
        intervalMs?: number;
        runAt?: string | number;
    };
    target: {
        type: 'agent' | 'workflow';
        prompt?: string;
        workflowId?: string;
    };
    status: 'active' | 'paused' | 'completed' | 'error';
    createdAt: number;
    lastRunAt?: number;
    nextRunAt?: number;
    runCount: number;
    failCount: number;
}

export interface TaskRunView {
    id: string;
    taskId: string;
    taskName: string;
    status: 'running' | 'completed' | 'failed' | 'paused' | 'waiting_user';
    startedAt: number;
    completedAt?: number;
    duration?: number;
    output?: string;
    error?: string;
}

// --- Agent Console 状态模型 ---

/**
 * 资源状态包装器，用于处理异步加载的数据
 */
export interface ResourceState<T> {
    status: 'idle' | 'loading' | 'ready' | 'error';
    loaded: boolean;
    loading: boolean;
    error?: string;
    unsupported?: boolean;
    data?: T;
    updatedAt?: number;
}

export interface GatewayCapabilityErrorPayload {
    code: 'capability_not_supported' | 'invalid_request' | 'internal_error';
    message: string;
    capability?: string;
}

export interface SettingsNavigatePayload {
    visible: true;
    section?: 'models' | 'memory' | 'mcp' | 'skills';
    search?: string;
    itemId?: string;
}

export type ConsoleTab =
    | 'overview'
    | 'model'
    | 'tools'
    | 'skills'
    | 'prompt-memory'
    | 'runs'
    | 'permissions'
    | 'diagnostics';

export interface RunSummaryView {
    id: string;
    sessionId: string;
    turnId?: string;
    agentId?: string;
    status: 'queued' | 'running' | 'waiting_user' | 'paused' | 'stopped' | 'failed' | 'completed';
    title?: string;
    startedAt: number;
    finishedAt?: number;
    durationMs?: number;
    modelSummary?: string;
    toolCount?: number;
    artifactCount?: number;
    tokenUsage?: TokenUsageView;
    errorSummary?: string;
    waitingReason?: 'permission' | 'user_input' | 'external_callback';
}

export interface RunStepView {
    id: string;
    runId: string;
    type: 'thinking' | 'tool' | 'approval' | 'message' | 'artifact' | 'system';
    title: string;
    status: 'running' | 'completed' | 'failed' | 'skipped';
    startedAt?: number;
    finishedAt?: number;
    toolName?: string;
    description?: string;
    artifactIds?: string[];
    permissionRequestId?: string;
}

export interface RunDetailView extends RunSummaryView {
    steps: RunStepView[];
    artifacts: SessionArtifactView[];
    permissions: PermissionRequestView[];
    diagnostics: DiagnosticIssueView[];
    auditLogs: AuditLogView[];
}

export interface PermissionRequestView {
    id: string;
    sessionId?: string;
    runId?: string;
    stepId?: string;
    agentId?: string;
    kind: 'command' | 'file_write' | 'network' | 'mcp_tool';
    title: string;
    reason?: string;
    target?: string;
    createdAt: number;
    riskLevel: 'low' | 'medium' | 'high';
    status: 'pending' | 'approved' | 'denied' | 'expired';
    rememberScope?: 'session' | 'agent' | 'global';
}

export interface AuditLogView {
    id: string;
    sessionId?: string;
    runId?: string;
    permissionRequestId?: string;
    actionType: 'permission' | 'run_control' | 'artifact_open' | 'workspace_restore';
    actor: 'user' | 'system' | 'agent';
    result: 'approved' | 'denied' | 'failed' | 'completed';
    summary: string;
    createdAt: number;
}

export interface DiagnosticIssueView {
    id: string;
    category: 'llm' | 'mcp' | 'memory' | 'permission' | 'protocol' | 'artifact' | 'runtime' | 'unknown';
    severity: 'info' | 'warn' | 'error';
    title: string;
    message: string;
    suggestedActions: string[];
    relatedRunId?: string;
    relatedStepId?: string;
    relatedSessionId?: string;
    relatedPermissionRequestId?: string;
    updatedAt: number;
    retryable?: boolean;
}

export interface WorkspaceRestoreView {
    sessionId?: string;
    agentId?: string;
    consoleVisible: boolean;
    activeTab?: ConsoleTab;
    selectedRunId?: string;
    selectedArtifactId?: string;
    selectedPermissionRequestId?: string;
    selectedDiagnosticId?: string;
    restorableRunState?: 'none' | 'view_only' | 'reattachable';
    updatedAt: number;
}

/**
 * Agent 运行态快照 (agent.inspect)
 */
export interface AgentRuntimeSnapshot {
    agentId: string;
    name: string;
    model: { provider: string; model: string; source: 'global' | 'agent' | 'session_override' };
    systemPrompt: string;
    status?: 'idle' | 'running' | 'paused' | 'error';
    activeSkills: string[];
    availableTools: string[];
    skills?: Array<{ id: string; title: string; enabled: boolean }>;
    capabilityPolicy: Record<string, unknown>;
}

/**
 * 会话运行态快照 (session.runtime)
 */
export interface SessionRuntimeSnapshot {
    sessionId: string;
    modelOverride?: { orchestration?: { provider: string; model: string }; execution?: { provider: string; model: string } };
    /** 模型绑定详细视图（Plan 2 扩展） */
    orchestrationDetail?: ModelBindingDetailView;
    executionDetail?: ModelBindingDetailView;
    totalUsage: TokenUsageView;
    turnCount?: number;
    lastRunId?: string;
    lastStatus?: string;
}

/**
 * Prompt 预览视图 (session.prompt.preview)
 */
export interface PromptPreviewView {
    systemPrompt: string;
    skillFragments: Array<{ title: string; content: string }>;
    memoryFragments: Array<{ content: string; source: string }>;
    /** @deprecated 使用 toolDescriptions 代替 */
    toolFragments?: Array<{ name: string; description: string }>;
    /** 工具描述列表（对齐后端 ToolDefinition.description） */
    toolDescriptions: Array<{ name: string; description: string }>;
    contextSummary: string;
    conversationFragments?: Array<{ role: string; summary: string }>;
    capabilityPolicy?: string;
    activeSkill?: { id: string; title: string };
    tokenBudget?: { maxTokens: number; iterationBudget: number };
    redacted?: boolean;
}

/**
 * 工具详情视图 (session.tools.list)
 * Plan 3 扩展：添加运行时字段
 */
export interface ToolDescriptorView {
    name: string;
    description: string;
    source: 'builtin' | 'mcp_server' | 'mcp_client' | 'skill' | 'evolution' | 'manual' | 'skill_unlocked';
    sourceName?: string;
    inputSchema: Record<string, unknown>;
    enabled: boolean;
    /** Plan 3 扩展：最近调用时间 */
    lastUsedAt?: number;
    /** Plan 3 扩展：最近调用状态 */
    lastCallStatus?: 'success' | 'error' | 'running';
    /** Plan 3 扩展：工具解锁来源 */
    unlockedBy?: string;
    /** Plan 3 扩展：工具解锁原因 */
    unlockedReason?: string;
}

/**
 * 记忆命中视图 (session.memory.hits)
 * Plan 3 扩展：添加 sourceType 和 turnId
 */
export interface MemoryHitView {
    content: string;
    score: number;
    reason?: string;
    source: string;
    timestamp: number;
    /** Plan 3 扩展：命中来源类型 */
    sourceType?: 'semantic' | 'keyword' | 'distillation';
    /** Plan 3 扩展：命中的轮次 ID */
    turnId?: string;
}

/**
 * 技能绑定视图 (Plan 3 新增)
 * 用于 Agent Console 中的运行态技能展示
 */
export interface SkillBindingView {
    id: string;
    title: string;
    source: 'global' | 'agent' | 'runtime';
    enabled: boolean;
    summary?: string;
    contentPreview?: string;
    loadedFrom?: string;
    activatedAt?: number;
    sticky?: boolean;
}

/**
 * 工具解锁事件 (Plan 3 新增)
 * 对应后端 ToolUnlockedPayload
 */
export interface ToolUnlockedEvent {
    sessionId?: string;
    toolName: string;
    description?: string;
    source?: 'tool_search' | 'skill_activation' | 'manual';
    reason?: string;
    timestamp?: number;
}

/**
 * 技能激活事件 (Plan 3 新增)
 */
export interface SkillActivatedEvent {
    sessionId?: string;
    skillId: string;
    title?: string;
    content?: string;
    source?: 'global' | 'agent' | 'runtime';
    sticky?: boolean;
    timestamp?: number;
}

/**
 * 技能切换事件 (Plan 3 新增)
 */
export interface SkillSwitchedEvent {
    sessionId?: string;
    previousSkillId?: string;
    currentSkillId: string;
    currentSkillTitle?: string;
    timestamp?: number;
}

/**
 * 技能退出事件 (Plan 3 新增)
 */
export interface SkillExitedEvent {
    sessionId?: string;
    skillId: string;
    title?: string;
    sticky?: boolean;
    timestamp?: number;
}

/**
 * Token 使用情况视图 (session.token.usage)
 */
export interface TokenUsageView {
    inputTokens: number;
    outputTokens: number;
    cacheCreationInputTokens?: number;
    cacheReadInputTokens?: number;
    totalCost?: number; // 估算成本
}

/**
 * 对齐后端 nova-protocol::Usage 结构
 * 用于单轮 token 统计的聚合视图
 */
export interface UsageView {
    inputTokens: number;
    outputTokens: number;
    cacheCreationInputTokens?: number;
    cacheReadInputTokens?: number;
}

/**
 * 单轮 token 统计
 */
export interface TurnTokenUsageView {
    turnId: string;
    usage: UsageView;
    estimatedCostUsd?: number;
}

/**
 * 会话累计 token 统计
 */
export interface SessionTokenUsageView {
    sessionId: string;
    totalUsage: UsageView;
    turnCount: number;
    estimatedCostUsd?: number;
    lastUpdatedAt: number;
}

/**
 * 模型绑定视图，用于展示 orchestration / execution 绑定及其来源
 */
export interface ModelBindingView {
    provider: string;
    model: string;
    source: 'global' | 'agent' | 'session_override';
}

/**
 * 模型绑定详细视图，扩展了继承和可编辑作用域信息
 */
export interface ModelBindingDetailView extends ModelBindingView {
    inheritedFrom?: string;
    editableScopes: Array<'global' | 'agent' | 'session_override'>;
}
