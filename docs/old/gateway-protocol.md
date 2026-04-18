# Gateway Protocol Specification

This document defines the WebSocket message structures used by the `GatewayClient` to communicate with the Gateway Server.

## 1. Core Message Structure

All messages follow this base format:

```typescript
interface GatewayMessage {
    type: string;
    id?: string; // Required for request-response patterns
    payload?: unknown;
}
```

---

## 2. Request-Response API (Command Pattern)

The client uses `request<T>(type, payload, timeout)` to send commands and wait for a response. The response must have the same `id` as the request.

### 2.1 Chat & Session Management
| Type | Request Payload | Expected Response Payload | Description |
| :--- | :--- | :--- | :--- |
| `chat` | `{ input, sessionId, attachments?, source?, chatroomId?, agentId? }` | `{ output?: string }` | Start or continue a chat session. |
| `chat.stop` | `{ sessionId }` | `{ success: boolean }` | Stop an ongoing chat task. |
| `sessions.list` | None | `{ sessions: Session[] }` | List all chat sessions. |
| `sessions.messages` | `{ sessionId }` | `{ messages: unknown[] }` | Get message history for a session. |
| `sessions.logs` | `{ sessionId }` | `{ logs: unknown[] }` | Get logs for a session. |
| `sessions.create` | `{ title?, cloudChatroomId?, cloudAgentName? }` | `{ session: Session }` | Create a new session. |
| `sessions.delete` | `{ sessionId }` | `{ success: boolean }` | Delete a session. |
| `sessions.artifacts` | `{ sessionId }` | `{ artifacts: SessionArtifactView[] }` | Get artifacts for a session. |
| `sessions.artifacts.save`| `{ sessionId, artifact }` | `{ artifact: SessionArtifactView }` | Save an artifact to a session. |

### 2.2 Agent Management
| Type | Request Payload | Expected Response Payload | Description |
| :--- | :--- | :--- | :--- |
| `agents.list` | None | `{ agents: Agent[] }` | List all available agents. |
| `agents.create` | `{ id, name, description, icon, systemPrompt }` | `{ agent: Record<string, unknown> }` | Create a new agent. |
| `agents.update` | `{ agentId, updates }` | `{ agent: Record<string, unknown> }` | Update agent configuration. |
| `agents.delete` | `{ agentId }` | `{ success: boolean }` | Delete an agent. |
| `agents.switch` | `{ agentId }` | `{ agent, messages: unknown[] }` | Switch active agent and get context. |
| `agents.history.clear`| `{ agentId }` | `{ success: boolean }` | Clear agent's message history. |

### 2.3 Scheduler API
| Type | Request Payload | Expected Response Payload | Description |
| :--- | :--- | :--- | :--- |
| `scheduler.list` | None | `{ tasks: ScheduledTaskView[] }` | List scheduled tasks. |
| `scheduler.runs` | `{ taskId?, limit? }` | `{ runs: TaskRunView[] }` | Get execution records. |
| `scheduler.pause` | `{ taskId }` | `{ success: boolean }` | Pause a task. |
| `scheduler.resume` | `{ taskId }` | `{ success: boolean }` | Resume a task. |
| `scheduler.delete` | `{ taskId }` | `{ success: boolean }` | Delete a task. |
| `scheduler.trigger` | `{ taskId }` | `{ run: unknown }` | Manually trigger a task. |

### 2.4 Memory & Distillation
| Type | Request Payload | Expected Response Payload | Description |
| :--- | :--- | :--- | :--- |
| `memory.stats` | None | `{ enabled, totalCount, ... }` | Get memory usage stats. |
| `memory.list` | `{ page, pageSize }` | `{ items, total, ... }` | List memory items (paginated). |
| `memory.search` | `{ query, limit }` | `{ items: any[] }` | Search memory contents. |
| `memory.delete` | `{ id }` | `{ success: boolean }` | Delete a memory entry. |
| `memory.clear` | None | `{ success: boolean }` | Clear all memory. |
| `distillation.stats` | None | `any` | Get distillation statistics. |
| `distillation.graph` | None | `{ cards, relations, topics }` | Get knowledge graph data. |
| `distillation.config.update`| `config` | `{ success, message? }` | Update distillation settings. |
| `distillation.trigger` | None | `{ success, message? }` | Trigger distillation process. |
| `distillation.cards` | `{ layer, limit, offset }` | `{ cards, total }` | List distillation cards. |
| `distillation.card.delete`| `{ cardId }` | `{ success, message? }` | Delete a specific card. |

### 2.5 System & Other
| Type | Request Payload | Expected Response Payload | Description |
| :--- | :--- | :--- | :--- |
| `auth` | `{ token }` | N/A (via `auth.success/failed`) | Authenticate session. |
| `mcp.tool.call` | `{ tool, args }` | `{ success, result?, error? }` | Execute a local MCP tool. |
| `mcp.client.register` | `{ tools }` | N/A | Register local MCP tools. |
| `mcp.client.unregister`| None | N/A | Unregister local MCP tools. |
| `settings.get` | None | `{ outputPath, defaultOutputPath }` | Get client settings. |
| `settings.update` | `{ outputPath? }` | `{ outputPath }` | Update client settings. |
| `config.get` | None | `ServerConfigView` | Get server-side configuration. |
| `config.update` | `ServerConfigUpdate` | `{ success, message? }` | Update server configuration. |
| `setup.complete` | `{ provider, apiKey, ... }` | `{ message? }` | Complete initial setup. |
| `browser.launch` | None | `{ success, message }` | Launch debug browser. |
| `debug.subscribe` | None | N/A (via `debug.log`) | Subscribe to debug logs. |
| `debug.unsubscribe` | None | N/A | Unsubscribe from debug logs. |
| `evolution.stats` | None | `{ schemaVersion, stats }` | Get evolution statistics. |
| `evolution.skills.list`| None | `{ skills }` | List installed skills. |
| `evolution.skills.uninstall`| `{ slug }` | `{ success }` | Uninstall a skill. |
| `evolution.tools.list`| None | `{ tools }` | List custom tools. |
| `evolution.tools.delete`| `{ name }` | `{ success }` | Delete a custom tool. |
| `evolution.forge.accept`| `{ id, title, content, ... }` | `{ success }` | Accept a forged suggestion. |
| `evolution.forge.dismiss`| None | `{ success }` | Dismiss current suggestion. |
| `evolution.forged.list` | None | `{ skills }` | List all forged skills. |
| `evolution.forged.delete`| `{ id }` | `{ success }` | Delete a forged skill. |
| `openflux.login` | `{ username, password }` | `{ success, message? }` | Cloud login. |
| `openflux.logout` | None | N/A | Cloud logout. |
| `openflux.status` | None | `{ loggedIn, username? }` | Check cloud status. |
| `openflux.agents` | None | `{ agents: OpenFluxAgentInfo[] }` | Get cloud agents. |
| `openflux.agent-info` | `{ appId }` | `{ agent: OpenFluxAgentInfo | null }` | Get cloud agent info. |
| `openflux.chat-history`| `{ chatroomId, page, pageSize }` | `{ messages: OpenFluxChatMessage[] }` | Get cloud chat history. |
| `router.config.get` | None | `{ connected, config }` | Get Router config. |
| `router.config.update` | `Partial<RouterConfigView>` | `{ success, message? }` | Update Router config. |
| `router.send` | `RouterOutboundView` | `{ success, message? }` | Send message to Router. |
| `router.test` | `Partial<RouterConfigView>` | `{ success, message, latencyMs? }` | Test Router connection. |
| `router.bind` | `{ code }` | `{ success, message }` | Bind Router via code. |
| `router.qr-bind` | None | `{ success, message }` | Request QR binding. |
| `weixin.config.get` | None | `any` | Get Weixin config. |
| `weixin.config.update` | `config` | `{ success, message? }` | Update Weixin config. |
| `weixin.status` | None | `{ connected, enabled, accountId }` | Get Weixin connection status. |
| `weixin.qr-login` | None | `{ success, message? }` | Start Weixin QR login. |
| `weixin.disconnect` | None | `{ success }` | Disconnect Weixin. |
| `weixin.test` | None | `{ configured, enabled, connected }` | Test Weixin connection. |
| `config.set-llm-source`| `{ source }` | `{ source, error? }` | Set LLM source. |
| `config.get-llm-source`| None | `{ source, managed? }` | Get LLM source info. |

---

## 3. Server-to-Client Events (Push)

The server pushes these messages to the client. Some are stateless, others represent progress or state changes.

### 3.1 Progress & Chat Events
| Type | Payload Structure | Description |
| :--- | :--- | :--- |
| `chat.start` | `{ sessionId }` | AI started thinking/processing for a session. |
| `chat.progress` | `ProgressEvent` | Real-time updates during chat (thinking, tool usage, tokens). |
| `chat.complete` | `{ output?: string, sessionId?: string }` | Final result of a chat session. |
| `session.updated` | `{ sessionId }` | A session was updated (e.g., by a task). |
| `collaboration_result`| `{ sessionId, agentId, agentType, task, status, mode, output?, error?, duration? }` | Result of multi-agent collaboration. |

### 3.2 System & Control Events
| Type | Payload Structure | Description |
| :--- | :--- | :--- |
| `welcome` | `{ requireAuth?, setupRequired? }` | Initial handshake message. |
| `auth.success` | None | Authentication successful. |
| `auth.failed` | `{ message }` | Authentication failed. |
| `nexusai.auth-expired`| `{ message? }` | Atlas mode token expired. |
| `scheduler.event` | `SchedulerEventView` | Generic scheduler activity. |
| `evolution.confirm` | `EvolutionConfirmRequest` | Request user to approve a new tool. |
| `evolution.skills.updated`| None | Skills list changed. |
| `evolution.forge.suggest`| `{ id, title, content, category, reasoning }` | New suggestion for skill forging. |

### 3.3 Protocol/Integration Specific
| Type | Payload Structure | Description |
| :--- | :--- | :--- |
| `mcp.client.call` | `{ tool, args }` | Gateway requesting client to run an MCP tool. |
| `router.user_message`| `RouterInboundView` | Message arriving from Router. |
| `router.status` | `{ connected, status }` | Router connection status change. |
| `router.qr_bind_code`| `{ status, qr_data?, code?, api_base?, expires_in?, message? }` | QR code data for Router binding. |
| `router.qr_bind_success`| `{ app_user_id?, platform_user_id? }` | Successful Router binding via QR. |
| `router.bind_result` | `{ action, status, message? }` | Result of a Router binding attempt. |
| `weixin.status` | `{ connected, status }` | Weixin connection status change. |
| `weixin.qr_code` | `{ qrUrl, qrImgContent?, expire }` | Weixin QR code payload. |
| `weixin.qr_status` | `{ status, message }` | Weixin QR code scan status. |
| `weixin.login_success`| `{ accountId, token, baseUrl }` | Successful Weixin login. |
| `weixin.user_message`| `any` | Message arriving via Weixin. |
| `managed-llm-config` | `{ available, provider?, model?, quota?, currentSource? }` | Router-managed LLM config update. |
| `config.rebuildProgress`| `{ progress: number }` | Progress of memory index rebuild. |
| `debug.log` | `DebugLogEntry` | Debug log entry. |

---

## 4. Supporting Data Models (Reference)

### 4.1 Session
```typescript
interface Session {
    id: string;
    agentId: string;
    title?: string;
    createdAt: number;
    updatedAt: number;
    cloudChatroomId?: number;
    cloudAgentName?: string;
}

interface Agent {
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
```

### 4.2 Progress Event
```typescript
interface ProgressEvent {
    type: 'iteration' | 'thinking' | 'tool_start' | 'tool_result' | 'token' | 'complete';
    iteration?: number;
    tool?: string;
    args?: Record<string, unknown>;
    result?: unknown;
    thinking?: string;
    token?: string;
    output?: string;
    description?: string;
    llmDescription?: string;
    sessionId?: string;
}
```

### 4.3 Scheduled Task
```typescript
interface ScheduledTaskView {
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
```

### 4.4 Server Configuration
```typescript
interface ServerConfigView {
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
    mcp?: { servers?: McpServerView[] };
    gatewayMode: 'embedded' | 'remote';
    gatewayPort: number;
    agents?: {
        globalAgentName?: string;
        globalSystemPrompt?: string;
        skills?: Array<{ id: string; title: string; content: string; enabled: boolean }>;
        list?: Array<{ id: string; name: string; description: string; model?: { provider: string; model: string } }>;
    };
    sandbox?: {
        mode?: string;
        docker?: { image?: string; memoryLimit?: string; cpuLimit?: string; networkMode?: string };
        blockedExtensions?: string[];
    };
    presetModels?: Record<string, { value: string; label: string; multimodal?: boolean }[]>;
}
```
