import { EventBus } from '../core/event-bus';

export class VoiceOverlayView {
    private overlay: HTMLElement;
    private closeBtn: HTMLElement;

    constructor(private bus: EventBus) {
        this.overlay = document.getElementById('voice-overlay') as HTMLElement;
        this.closeBtn = document.getElementById('voice-overlay-close') as HTMLElement;
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
