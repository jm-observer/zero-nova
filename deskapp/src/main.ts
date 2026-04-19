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

        // 7. Initial Data Load
        console.log('[Main] Establishing WebSocket connection...');
        await gatewayClient.connect();
        console.log('[Main] Connected successfully.');
        
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

        // Listen for session selection to load messages
        bus.on(Events.SESSION_SELECTED, async (payload: { sessionId: string }) => {
             if (!payload.sessionId) {
                 state.setMessages([]);
                 return;
             }
             console.log('[Main] Loading messages for session:', payload.sessionId);
             try {
                const messages = await gatewayClient.getMessages(payload.sessionId);
                state.setMessages(messages as any);
             } catch (err) {
                console.error('[Main] Failed to load messages:', err);
             }
        });

        // Initial session selection
        if (state.currentAgentId) {
            const filtered = sessions.filter(s => s.agentId === state.currentAgentId);
            if (filtered.length > 0) {
                state.setCurrentSession(filtered[0].id);
            }
        } else if (sessions.length > 0) {
             state.setCurrentSession(sessions[0].id);
        }
        
        // 8. Hide Loading Overlay
        console.log('[Main] Initialization complete. Hiding overlay.');
        const overlay = document.getElementById('app-loading-overlay');
        if (overlay) {
            overlay.classList.add('fade-out');
            setTimeout(() => overlay.remove(), 500);
        }
    } catch (err) {
        console.error('[Main] ERROR during initialization:', err);
    }
}

// Start Application
document.addEventListener('DOMContentLoaded', () => {
    init().catch(console.error);
});
