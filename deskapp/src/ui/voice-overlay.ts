import { t } from '../i18n/index';
import { EventBus } from '../core/event-bus';

export class VoiceOverlayView {
    private overlay: HTMLElement;
    private closeBtn!: HTMLElement;

    constructor(private bus: EventBus) {
        this.overlay = this.requireElement('voice-overlay');
    }

    init() {
        this.ensureOverlayMarkup();
        this.closeBtn = this.requireElement('voice-overlay-close');
        this.closeBtn.addEventListener('click', () => this.toggle(false));
        
        this.bus.on('voice:toggle', (payload: { active: boolean }) => {
            this.toggle(payload.active);
        });

        this.bus.on('voice:status', (payload: { state: string }) => {
            this.updateState(payload.state);
        });
    }

    toggle(active: boolean) {
        this.overlay.classList.toggle('hidden', !active);
        if (active) {
            this.updateState('ready');
        } else {
            this.bus.emit('voice:stop', {});
        }
    }

    private updateState(state: string) {
        this.overlay.setAttribute('data-state', state);
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
                </div>
                <div class="voice-status-text">${t('voice.click_start')}</div>
                <div class="voice-transcript"></div>
                <div class="voice-controls"></div>
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
