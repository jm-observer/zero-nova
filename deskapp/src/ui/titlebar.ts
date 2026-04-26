import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { t } from '../i18n/index';
import { AppState } from '../core/state';
import { EventBus, Events } from '../core/event-bus';

export class TitleBarView {
    private statusIndicator: HTMLDivElement;
    private btnMinimize: HTMLButtonElement;
    private btnMaximize: HTMLButtonElement;
    private btnClose: HTMLButtonElement;
    private themeToggle: HTMLButtonElement;

    constructor(_state: AppState, private bus: EventBus) {
        this.statusIndicator = document.getElementById('status-indicator') as HTMLDivElement;
        this.btnMinimize = document.getElementById('btn-minimize') as HTMLButtonElement;
        this.btnMaximize = document.getElementById('btn-maximize') as HTMLButtonElement;
        this.btnClose = document.getElementById('btn-close') as HTMLButtonElement;
        this.themeToggle = document.getElementById('theme-toggle') as HTMLButtonElement;
    }

    init() {
        this.bindEvents();
        this.initDragging();
        this.initTheme();

        // 订阅状态变化
        this.bus.on(Events.THEME_CHANGED, (payload: any) => {
            this.applyThemeToDOM(payload.theme);
        });

        this.bus.on(Events.GATEWAY_STATUS, (payload: { status: string }) => {
            this.handleGatewayStatusChange(payload.status);
        });
    }

    private handleGatewayStatusChange(payload: any) {
        const { status, text } = typeof payload === 'string' ? { status: payload, text: null } : payload;
        
        switch (status) {
            case 'connected':
                this.setStatus(text || t('status.connected'), 'ready');
                break;
            case 'running':
                this.setStatus(text || t('status.running'), 'running');
                break;
            case 'connecting':
                this.setStatus(text || t('status.connecting'), 'running');
                break;
            case 'reconnecting':
                this.setStatus(text || t('status.reconnecting'), 'running');
                break;
            case 'disconnected':
                this.setStatus(text || t('status.disconnected'), 'error');
                break;
            case 'failed':
                this.setStatus(text || t('status.error'), 'error');
                break;
        }
    }

    private bindEvents() {
        this.btnMinimize.addEventListener('click', () => invoke('window_minimize'));
        this.btnMaximize.addEventListener('click', () => invoke('window_maximize'));
        this.btnClose.addEventListener('click', () => invoke('window_close'));

        this.themeToggle.addEventListener('click', () => {
            const current = document.documentElement.getAttribute('data-theme');
            const newTheme = current === 'light' ? 'dark' : 'light';
            this.bus.emit(Events.THEME_CHANGED, { theme: newTheme });
            localStorage.setItem('openflux-theme', newTheme);
        });

    }

    private initDragging() {
        const isMacOS = navigator.platform.toUpperCase().includes('MAC');
        if (isMacOS) {
            document.body.classList.add('platform-macos');
            const appWindow = getCurrentWindow();
            const titleBar = document.querySelector('.title-bar') as HTMLElement;
            if (titleBar) {
                titleBar.addEventListener('mousedown', (e) => {
                    if (e.button !== 0) return;
                    const target = e.target as HTMLElement;
                    if (target.closest('button, input, select, a, [data-no-drag]')) return;
                    e.preventDefault();
                    appWindow.startDragging();
                });
            }
        }
    }

    private initTheme() {
        const savedTheme = localStorage.getItem('openflux-theme') as 'dark' | 'light' | null;
        const theme = savedTheme || 'light';
        this.applyThemeToDOM(theme);
    }

    private applyThemeToDOM(theme: string) {
        const themeIconSun = this.themeToggle.querySelector('.theme-icon-sun') as SVGElement;
        const themeIconMoon = this.themeToggle.querySelector('.theme-icon-moon') as SVGElement;

        if (theme === 'light') {
            document.documentElement.setAttribute('data-theme', 'light');
            themeIconSun?.classList.add('hidden');
            themeIconMoon?.classList.remove('hidden');
        } else {
            document.documentElement.removeAttribute('data-theme');
            themeIconSun?.classList.remove('hidden');
            themeIconMoon?.classList.add('hidden');
        }
    }

    setStatus(text: string, type: 'ready' | 'running' | 'error'): void {
        const dot = this.statusIndicator.querySelector('.dot');
        const textEl = this.statusIndicator.querySelector('.text');
        if (dot) dot.className = `dot ${type}`;
        if (textEl) textEl.textContent = text;
    }
}
