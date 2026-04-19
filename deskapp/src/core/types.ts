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
    type: 'iteration' | 'thinking' | 'tool_start' | 'tool_result' | 'token' | 'complete' | 'turn_complete' | 'iteration_limit';
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
