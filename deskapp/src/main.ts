import { invoke } from '@tauri-apps/api/core';
import { initI18n, t } from './i18n/index';
import zhPack from './i18n/zh';
import enPack from './i18n/en';
import { GatewayClient, GatewayRequestError } from './gateway-client';
import { EventBus, Events } from './core/event-bus';
import { AppState } from './core/state';
import { ChatService } from './services/chat-service';
import { bargeInDetector, player, recorder, setVoiceSynthesizeCallback, streamingTtsManager, ttsManager } from './voice';

import type { Session } from './core/types';

// UI Components
import { TitleBarView } from './ui/titlebar';
import { SidebarView } from './ui/sidebar-view';
import { ChatView } from './ui/chat-view';
import { SettingsView } from './ui/settings-view';
import { ModalsView } from './ui/modals';
import { VoiceOverlayView } from './ui/voice-overlay';
import { AgentConsoleView } from './ui/agent-console-view';

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
        // 初始化前端 token 追踪（Plan 2）
        state.initTokenTracking();
        // 初始化 Skill/Tool 事件追踪（Plan 3）
        state.initSkillToolTracking();

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
            settings: new SettingsView(state, bus),
            modals: new ModalsView(bus),
            voiceOverlay: new VoiceOverlayView(bus),
            agentConsole: new AgentConsoleView(state, bus)
        };

        // 5. Initialize Components
        console.log('[Main] Initializing UI views...');
        Object.values(ui).forEach(comp => comp.init());

        // 6. Services
        console.log('[Main] Starting services...');
        const chatService = new ChatService(state, bus, gatewayClient);
        chatService.init();

        setVoiceSynthesizeCallback(async (text: string) => {
            try {
                const audio = await gatewayClient.synthesizeVoice(text, state.currentSessionId || undefined, state.voiceStatus?.tts.voice);
                return { audio };
            } catch (error) {
                const message = error instanceof Error ? error.message : String(error);
                return { error: message };
            }
        });

        let voiceTurnId = 0;
        let voiceTranscriptMessageId: string | null = null;
        let voiceAwaitingAssistant = false;
        let voiceInterrupted = false;

        const nextVoiceTurn = () => {
            voiceTurnId += 1;
            return voiceTurnId;
        };

        const stopVoiceOutput = () => {
            streamingTtsManager.cancel();
            ttsManager.cancelAll();
            player.stop();
            bargeInDetector.stop();
        };

        const setVoiceError = (message: string) => {
            state.updateVoiceConversation({
                active: state.voiceModeActive,
                phase: 'error',
                error: message,
                canRetry: true,
            });
        };

        const startVoiceTurn = async () => {
            if (recorder.getState() !== 'idle' || state.voiceConversation.phase === 'requesting_permission') {
                return;
            }

            if (!state.voiceStatus?.stt.enabled || !state.voiceStatus.stt.available) {
                setVoiceError(t('voice.chat_unavailable'));
                return;
            }

            const turnId = nextVoiceTurn();
            voiceInterrupted = false;

            state.updateVoiceConversation({
                active: true,
                phase: 'requesting_permission',
                transcript: '',
                transcriptState: 'idle',
                error: null,
                durationSeconds: 0,
                canRetry: false,
            });

            recorder.setAutoStopCallback(() => {
                void finishVoiceTurn(turnId);
            });

            try {
                await recorder.start({
                    vad: true,
                    vadSilenceMs: 1500,
                    minDurationMs: 800,
                });
            } catch (error) {
                setVoiceError(t('voice.mic_failed'));
            }
        };

        const finishVoiceTurn = async (turnId: number) => {
            if (turnId !== voiceTurnId || recorder.getState() !== 'recording') {
                return;
            }

            state.updateVoiceConversation({
                phase: 'uploading_audio',
                canRetry: false,
            });

            try {
                const audio = await recorder.stop();
                if (turnId !== voiceTurnId) {
                    return;
                }

                voiceTranscriptMessageId = `voice-transcript-${Date.now()}`;
                state.upsertVoiceTranscriptMessage(voiceTranscriptMessageId, '', 'pending');
                state.updateVoiceConversation({
                    phase: 'recognizing',
                    transcriptState: 'pending',
                });

                const result = await gatewayClient.transcribeVoice({
                    sessionId: state.currentSessionId || undefined,
                    audioFormat: 'audio/webm',
                    sampleRate: 16000,
                    channelCount: 1,
                    mode: 'once',
                    audio,
                });

                if (turnId !== voiceTurnId) {
                    return;
                }

                const transcript = result.text.trim();
                if (!transcript) {
                    if (voiceTranscriptMessageId) {
                        state.removeMessageById(voiceTranscriptMessageId);
                        voiceTranscriptMessageId = null;
                    }
                    setVoiceError(t('voice.not_recognized'));
                    return;
                }

                if (voiceTranscriptMessageId) {
                    state.upsertVoiceTranscriptMessage(voiceTranscriptMessageId, transcript, 'final');
                }

                state.updateVoiceConversation({
                    phase: 'submitting_text',
                    transcript,
                    transcriptState: 'final',
                    error: null,
                });

                voiceAwaitingAssistant = true;
                bus.emit('message:send', { text: transcript, skipOptimisticMessage: true });
                state.updateVoiceConversation({
                    phase: 'waiting_assistant',
                    canRetry: false,
                });
            } catch (error) {
                if (error instanceof GatewayRequestError) {
                    setVoiceError(error.kind === 'unsupported' ? t('voice.chat_unavailable') : t('voice.recognition_failed'));
                    return;
                }

                setVoiceError(t('voice.process_failed'));
            }
        };

        const closeVoiceMode = () => {
            nextVoiceTurn();
            voiceAwaitingAssistant = false;
            voiceInterrupted = false;
            recorder.setAutoStopCallback(null);
            recorder.cancel();
            if (voiceTranscriptMessageId) {
                state.removeMessageById(voiceTranscriptMessageId);
                voiceTranscriptMessageId = null;
            }
            stopVoiceOutput();
            state.resetVoiceConversation();
        };

        recorder.setStateCallback((recordingState, duration) => {
            if (recordingState === 'recording') {
                state.updateVoiceConversation({
                    active: true,
                    phase: 'recording',
                    error: null,
                    durationSeconds: duration ?? 0,
                });
            }
        });

        player.setStateCallback((playbackState) => {
            if (playbackState === 'playing') {
                state.updateVoiceConversation({
                    active: true,
                    phase: 'speaking',
                    canRetry: false,
                });

                if (!bargeInDetector.isActive()) {
                    void bargeInDetector.start();
                }
                return;
            }

            if (playbackState === 'idle') {
                bargeInDetector.stop();
            }
        });

        bargeInDetector.setCallback(() => {
            voiceInterrupted = true;
            stopVoiceOutput();
            state.updateVoiceConversation({
                active: state.voiceModeActive,
                phase: 'interrupted',
                canRetry: true,
            });
        });

        bus.on(Events.VOICE_MODE_SET_REQUEST, (payload: { active: boolean }) => {
            state.setVoiceMode(payload.active);
        });

        bus.on(Events.VOICE_MODE_TOGGLE, (payload: { active: boolean }) => {
            if (!payload.active) {
                closeVoiceMode();
                return;
            }

            state.updateVoiceConversation({
                active: true,
                phase: 'idle',
                transcript: '',
                transcriptState: 'idle',
                error: null,
                durationSeconds: 0,
                canRetry: false,
            });

            void startVoiceTurn();
        });

        bus.on(Events.VOICE_CONTROL_START, () => {
            if (!state.voiceModeActive) {
                state.setVoiceMode(true);
                return;
            }

            void startVoiceTurn();
        });

        bus.on(Events.VOICE_CONTROL_STOP, () => {
            if (recorder.getState() === 'recording') {
                void finishVoiceTurn(voiceTurnId);
                return;
            }

            stopVoiceOutput();
            state.updateVoiceConversation({
                active: state.voiceModeActive,
                phase: 'idle',
                canRetry: false,
                error: null,
            });
        });

        bus.on(Events.VOICE_CONTROL_RETRY, () => {
            if (!state.voiceModeActive) {
                state.setVoiceMode(true);
                return;
            }

            void startVoiceTurn();
        });

        bus.on('chat:complete', async (payload: { sessionId?: string }) => {
            if (!voiceAwaitingAssistant || payload.sessionId !== state.currentSessionId) {
                return;
            }

            voiceAwaitingAssistant = false;
            let assistantMessage = [...state.messages].reverse().find(message => message.role === 'assistant' && typeof message.content === 'string' && message.content.trim());

            if (!assistantMessage && payload.sessionId) {
                try {
                    const refreshedMessages = await gatewayClient.getMessages(payload.sessionId);
                    if (payload.sessionId === state.currentSessionId) {
                        state.setMessages(refreshedMessages as any[]);
                        assistantMessage = [...state.messages].reverse().find(message => message.role === 'assistant' && typeof message.content === 'string' && message.content.trim());
                    }
                } catch (error) {
                    console.warn('[Main] Failed to refresh messages before voice autoplay:', error);
                }
            }

            if (assistantMessage && state.ttsAutoPlay && state.voiceStatus?.tts.enabled && state.voiceStatus.tts.available) {
                try {
                    await ttsManager.speak(String(assistantMessage.content), assistantMessage.id);
                } catch (error) {
                    console.error('[Main] Voice autoplay failed:', error);
                    setVoiceError(t('voice.process_failed'));
                    return;
                }
            }

            state.updateVoiceConversation({
                active: state.voiceModeActive,
                phase: 'idle',
                durationSeconds: 0,
                canRetry: false,
            });

            voiceTranscriptMessageId = null;

            if (state.voiceModeActive && !voiceInterrupted) {
                void startVoiceTurn();
            }
        });

        // 7. Initial Data Load & Connection Handling
        console.log('[Main] Establishing WebSocket connection...');
        
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
                state.setAgents(agents as Array<{ id: string; name: string; description?: string; icon?: string; color?: string; default?: boolean; systemPrompt?: string; createdAt: number; updatedAt: number }>);

                console.log('[Main] Fetching sessions...');
                const sessions = await gatewayClient.getSessions();
                state.setSessions(sessions as Session[]);

                try {
                    const voiceCapabilities = await gatewayClient.getVoiceCapabilities();
                    state.setVoiceCapabilities(voiceCapabilities);
                } catch (error) {
                    if (error instanceof GatewayRequestError && error.kind === 'unsupported') {
                        state.setVoiceCapabilities(null);
                    } else {
                        console.warn('[Main] Failed to fetch voice capabilities:', error);
                    }
                }

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
                    // 已经在会话中，主动触发一次消息刷新以同步状态
                    bus.emit(Events.SESSION_SELECTED, { sessionId: state.currentSessionId });
                }

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

                state.setMessages(messages as any[]);
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
