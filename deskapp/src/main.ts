import { invoke } from '@tauri-apps/api/core';
import { initI18n } from './i18n/index';
import zhPack from './i18n/zh';
import enPack from './i18n/en';
import { GatewayClient } from './gateway-client';
import { EventBus, Events } from './core/event-bus';
import { AppState } from './core/state';
import { ChatService } from './services/chat-service';

// UI Components
import { TitleBarView } from './ui/titlebar';
import { SidebarView } from './ui/sidebar-view';
import { ChatView } from './ui/chat-view';
import { ArtifactsView } from './ui/artifacts-view';
import { SchedulerView } from './ui/scheduler-view';
import { SettingsView } from './ui/settings-view';
import { ModalsView } from './ui/modals';
import { VoiceOverlayView } from './ui/voice-overlay';

// 1. Initialize i18n
initI18n(zhPack, enPack);

async function init() {
    console.log('[Main] Initializing core modules...');
    // 2. Core Infrastructure
    const bus = new EventBus();
    const state = new AppState(bus);
    
    // 3. API Client
    console.log('[Main] Fetching gateway config...');
    try {
        const config = await invoke<{ url: string, token?: string }>('get_gateway_config');
        console.log('[Main] Gateway config received:', config.url);
        
        const gatewayClient = new GatewayClient(config.url, config.token);
        state.setGatewayClient(gatewayClient);

        // Forward connection status to EventBus
        gatewayClient.onConnectionChange((status) => {
            bus.emit(Events.GATEWAY_STATUS, { status });
        });

        // 4. UI Components Registration
        console.log('[Main] Registering UI components...');
        const ui = {
            titleBar: new TitleBarView(state, bus),
            sidebar: new SidebarView(state, bus),
            chat: new ChatView(state, bus),
            artifacts: new ArtifactsView(state, bus),
            scheduler: new SchedulerView(state, bus),
            settings: new SettingsView(state, bus),
            modals: new ModalsView(state, bus),
            voiceOverlay: new VoiceOverlayView(state, bus)
        };

        // 5. Initialize Components
        console.log('[Main] Initializing UI views...');
        Object.values(ui).forEach(comp => comp.init());

        // 6. Services
        console.log('[Main] Starting services...');
        const chatService = new ChatService(state, bus, gatewayClient);
        chatService.init();

        // 7. Initial Data Load & Connection Handling
        console.log('[Main] Establishing WebSocket connection...');
        
        let dataLoaded = false;
        const loadInitialData = async () => {
            console.log('[Main] Loading application data...');
            try {
                // Check for first-time setup
                if (gatewayClient.isSetupRequired()) {
                    console.log('[Main] First-time setup detected.');
                    const setupWizard = document.getElementById('setup-wizard');
                    if (setupWizard) setupWizard.style.display = 'block';
                }

                console.log('[Main] Fetching agents...');
                const agents = await gatewayClient.getAgents();
                state.setAgents(agents as any);

                console.log('[Main] Fetching sessions...');
                const sessions = await gatewayClient.getSessions();
                state.setSessions(sessions as any);

                // Initial session selection (only if not already set)
                if (!state.currentSessionId) {
                    if (state.currentAgentId) {
                        const filtered = state.sessions.filter(s => s.agentId === state.currentAgentId);
                        if (filtered.length > 0) {
                            const sorted = [...filtered].sort((a, b) => (b.createdAt || 0) - (a.createdAt || 0));
                            state.setCurrentSession(sorted[0].id);
                        }
                    } else if (state.sessions.length > 0) {
                        state.setCurrentSession(state.sessions[0].id);
                    }
                } else {
                    // Refresh messages for current session to ensure sync
                    try {
                        const messages = await gatewayClient.getMessages(state.currentSessionId);
                        state.setMessages(messages as any);
                    } catch (mErr) {
                        console.warn('[Main] Failed to refresh messages for session:', state.currentSessionId);
                    }
                }

                dataLoaded = true;
                
                // Hide loading overlay
                console.log('[Main] Data loaded. Hiding overlay.');
                const overlay = document.getElementById('app-loading-overlay');
                if (overlay) {
                    overlay.classList.add('fade-out');
                    setTimeout(() => overlay.remove(), 500);
                }
            } catch (err) {
                console.error('[Main] ERROR during data loading:', err);
                // Keep overlay or show error in UI if possible
            }
        };

        // Listen for connection success to trigger data load
        bus.on(Events.GATEWAY_STATUS, (payload: { status: string }) => {
            if (payload.status === 'connected') {
                console.log('[Main] Gateway connected, triggering data load...');
                loadInitialData();
            }
        });

        // Start connection (non-blocking)
        gatewayClient.connect().catch(err => {
            console.warn('[Main] Initial connection attempt failed, will retry in background:', err.message);
        });

        // 8. Event Handlers setup (Sessions, Agents, etc.)
        // Listen for session selection to load messages
        bus.on(Events.SESSION_SELECTED, async (payload: { sessionId: string }) => {
             if (!payload.sessionId) {
                 state.setMessages([]);
                 return;
             }

             console.log('[Main] Loading messages for session:', payload.sessionId);
             try {
                const messages = await gatewayClient.getMessages(payload.sessionId);
                
                // 智慧合并/冲突处理：
                // 如果发现本地已经有临时 ID 的乐观消息，且服务器返回的消息数还不如本地多，
                // 说明后端还没完成持久化或消息还在处理中。此时应保留本地状态，避免发送首条消息时 UI 闪烁。
                if (state.currentSessionId === payload.sessionId) {
                    const localMsgs = state.messages;
                    const hasTmp = localMsgs.some(m => String(m.id).startsWith('tmp-'));
                    if (hasTmp && messages.length <= localMsgs.length) {
                        console.log('[Main] Preserving optimistic messages, skipping overwrite.');
                        return;
                    }
                }

                state.setMessages(messages as any);
             } catch (err) {
                console.error('[Main] Failed to load messages:', err);
             }
        });

        // Agent 切换时，对应切换 Session
        bus.on(Events.AGENT_SWITCHED, (payload: { agentId: string }) => {
            console.log('[Main] Agent switched, selecting latest session for:', payload.agentId);
            const filtered = state.sessions.filter(s => s.agentId === payload.agentId);
            if (filtered.length > 0) {
                const sorted = [...filtered].sort((a, b) => (b.createdAt || 0) - (a.createdAt || 0));
                state.setCurrentSession(sorted[0].id);
            } else {
                state.setCurrentSession(null);
            }
        });

    } catch (err) {
        console.error('[Main] CRITICAL ERROR during initialization:', err);
    }
}

// Start Application
document.addEventListener('DOMContentLoaded', () => {
    init().catch(console.error);
});
