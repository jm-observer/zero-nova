import { t } from '../i18n/index';
import { EventBus, Events } from '../core/event-bus';
import type { VoiceConversationPhase, VoiceConversationState } from '../core/state';

type OverlaySnapshot = VoiceConversationState;

export class VoiceOverlayView {
    private overlay: HTMLElement;
    private closeBtn!: HTMLButtonElement;
    private statusText!: HTMLElement;
    private transcriptEl!: HTMLElement;
    private errorEl!: HTMLElement;
    private durationEl!: HTMLElement;
    private primaryBtn!: HTMLButtonElement;
    private retryBtn!: HTMLButtonElement;
    private lastState: OverlaySnapshot = {
        active: false,
        phase: 'idle',
        transcript: '',
        transcriptState: 'idle',
        error: null,
        durationSeconds: 0,
        canRetry: false,
    };

    constructor(private bus: EventBus) {
        this.overlay = this.requireElement('voice-overlay');
    }

    init() {
        this.ensureOverlayMarkup();
        this.closeBtn = this.requireElement('voice-overlay-close');
        this.statusText = this.requireElement('voice-status-text');
        this.transcriptEl = this.requireElement('voice-transcript');
        this.errorEl = this.requireElement('voice-error-text');
        this.durationEl = this.requireElement('voice-duration-text');
        this.primaryBtn = this.requireElement('voice-main-btn');
        this.retryBtn = this.requireElement('voice-retry-btn');

        this.closeBtn.addEventListener('click', () => {
            this.bus.emit(Events.VOICE_CONTROL_STOP, {});
            this.bus.emit(Events.VOICE_MODE_SET_REQUEST, { active: false });
        });

        this.primaryBtn.addEventListener('click', () => {
            if (this.lastState.phase === 'recording') {
                this.bus.emit(Events.VOICE_CONTROL_STOP, {});
                return;
            }

            this.bus.emit(Events.VOICE_CONTROL_START, {});
        });

        this.retryBtn.addEventListener('click', () => {
            this.bus.emit(Events.VOICE_CONTROL_RETRY, {});
        });

        this.bus.on(Events.VOICE_MODE_TOGGLE, (payload: { active: boolean }) => {
            this.toggle(payload.active);
        });

        this.bus.on(Events.VOICE_STATE_UPDATED, (payload: OverlaySnapshot) => {
            this.renderState(payload);
        });
    }

    toggle(active: boolean) {
        this.overlay.classList.toggle('hidden', !active);
        if (!active) {
            this.renderState({
                ...this.lastState,
                active: false,
                phase: 'idle',
            });
        }
    }

    private renderState(state: OverlaySnapshot) {
        this.lastState = state;

        this.overlay.setAttribute('data-state', this.mapOverlayState(state.phase));
        this.overlay.setAttribute('data-voice-phase', state.phase);

        this.statusText.textContent = this.getStatusText(state.phase);
        this.transcriptEl.textContent = state.transcript;
        this.errorEl.textContent = state.error ?? '';
        this.errorEl.classList.toggle('hidden', !state.error);

        const showDuration = state.phase === 'recording' || state.durationSeconds > 0;
        this.durationEl.textContent = showDuration ? this.formatDuration(state.durationSeconds) : '';
        this.durationEl.classList.toggle('hidden', !showDuration);

        this.primaryBtn.disabled = this.isPrimaryDisabled(state.phase);
        this.primaryBtn.textContent = state.phase === 'recording' ? t('voice.stop_recording') : t('voice.start_recording');

        const showRetry = state.canRetry || state.phase === 'error' || state.phase === 'interrupted';
        this.retryBtn.classList.toggle('hidden', !showRetry);
    }

    private isPrimaryDisabled(phase: VoiceConversationPhase): boolean {
        return phase === 'requesting_permission'
            || phase === 'uploading_audio'
            || phase === 'recognizing'
            || phase === 'submitting_text'
            || phase === 'waiting_assistant';
    }

    private mapOverlayState(phase: VoiceConversationPhase): string {
        if (phase === 'recording') {
            return 'recording';
        }

        if (phase === 'waiting_assistant') {
            return 'answering';
        }

        if (phase === 'speaking') {
            return 'speaking';
        }

        if (phase === 'uploading_audio' || phase === 'recognizing' || phase === 'submitting_text' || phase === 'requesting_permission') {
            return 'processing';
        }

        return 'idle';
    }

    private getStatusText(phase: VoiceConversationPhase): string {
        switch (phase) {
            case 'requesting_permission':
                return t('voice.processing');
            case 'recording':
                return t('voice.listening');
            case 'uploading_audio':
            case 'recognizing':
                return t('voice.recognizing');
            case 'submitting_text':
                return t('voice.processing');
            case 'waiting_assistant':
                return t('voice.thinking');
            case 'speaking':
                return t('voice.replying');
            case 'interrupted':
                return t('voice.click_start');
            case 'error':
                return t('common.error');
            case 'idle':
            default:
                return t('voice.click_start');
        }
    }

    private formatDuration(durationSeconds: number): string {
        const minutes = Math.floor(durationSeconds / 60);
        const seconds = durationSeconds % 60;
        return `${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`;
    }

    private ensureOverlayMarkup() {
        if (this.overlay.querySelector('#voice-overlay-close')) {
            return;
        }

        this.overlay.innerHTML = `
            <div class="voice-overlay-bg"></div>
            <div class="voice-overlay-content">
                <div class="voice-overlay-header">
                    <div class="voice-overlay-title">${t('voice.title')}</div>
                    <button id="voice-overlay-close" class="voice-overlay-close" type="button" aria-label="${t('common.close')}">×</button>
                </div>
                <div class="voice-visual-area">
                    <div class="voice-ripple-container">
                        <div class="voice-ripple-ring ring-1"></div>
                        <div class="voice-ripple-ring ring-2"></div>
                        <div class="voice-ripple-ring ring-3"></div>
                        <div class="voice-ripple-core"></div>
                    </div>
                    <div id="voice-status-text" class="voice-status-text">${t('voice.click_start')}</div>
                    <div id="voice-duration-text" class="voice-duration-text hidden"></div>
                    <div id="voice-transcript" class="voice-transcript"></div>
                    <div id="voice-error-text" class="voice-error-text hidden"></div>
                </div>
                <div class="voice-controls">
                    <button id="voice-main-btn" class="voice-main-btn" type="button">${t('voice.start_recording')}</button>
                    <button id="voice-retry-btn" class="voice-secondary-btn hidden" type="button">${t('voice.retry')}</button>
                </div>
            </div>
        `;
    }

    private requireElement<T extends HTMLElement>(id: string): T {
        const element = document.getElementById(id);
        if (!(element instanceof HTMLElement)) {
            throw new Error(`Missing required voice overlay element: ${id}`);
        }

        return element as T;
    }
}
