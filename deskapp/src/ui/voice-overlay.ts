import { t } from '../i18n/index';
import { AppState } from '../core/state';
import { EventBus } from '../core/event-bus';

export class VoiceOverlayView {
    private overlay: HTMLElement;
    private closeBtn: HTMLElement;
    private statusText: HTMLElement;
    
    private active = false;

    constructor(private state: AppState, private bus: EventBus) {
        this.overlay = document.getElementById('voice-overlay') as HTMLElement;
        this.closeBtn = document.getElementById('voice-overlay-close') as HTMLElement;
        this.statusText = this.overlay.querySelector('.voice-overlay-status') as HTMLElement;
    }

    init() {
        this.closeBtn.addEventListener('click', () => this.toggle(false));
        
        this.bus.on('voice:toggle', (payload: { active: boolean }) => {
            this.toggle(payload.active);
        });

        this.bus.on('voice:status', (payload: { state: string }) => {
            this.updateState(payload.state);
        });
    }

    toggle(active: boolean) {
        this.active = active;
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
}
