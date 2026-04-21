import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { t } from '../i18n/index';
import { AppState } from '../core/state';
import { EventBus, Events } from '../core/event-bus';

export class TitleBarView {
    private statusIndicator: HTMLDivElement;
    private browserLaunchBtn: HTMLButtonElement | null;
    private btnMinimize: HTMLButtonElement;
    private btnMaximize: HTMLButtonElement;
    private btnClose: HTMLButtonElement;
    private themeToggle: HTMLButtonElement;
    private artifactsToggle: HTMLButtonElement;

    constructor(private state: AppState, private bus: EventBus) {
        this.statusIndicator = document.getElementById('status-indicator') as HTMLDivElement;
        this.browserLaunchBtn = document.getElementById('browser-launch-btn') as HTMLButtonElement;
        this.btnMinimize = document.getElementById('btn-minimize') as HTMLButtonElement;
        this.btnMaximize = document.getElementById('btn-maximize') as HTMLButtonElement;
        this.btnClose = document.getElementById('btn-close') as HTMLButtonElement;
        this.themeToggle = document.getElementById('theme-toggle') as HTMLButtonElement;
        this.artifactsToggle = document.getElementById('artifacts-toggle') as HTMLButtonElement;
    }

    init() {
        this.bindEvents();
        this.initDragging();
        this.initTheme();

        // 订阅状态变化
        this.bus.on(Events.THEME_CHANGED, (payload: any) => {
            this.applyThemeToDOM(payload.theme);
        });

        this.bus.on('browser:status_changed', (payload: { connected: boolean }) => {
            this.updateBrowserStatusIndicator(payload.connected);
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

        this.browserLaunchBtn?.addEventListener('click', async () => {
            if (!this.state.gatewayClient) return;
            this.browserLaunchBtn!.classList.add('loading');
            this.browserLaunchBtn!.disabled = true;
            try {
                const result = await this.state.gatewayClient.launchBrowser();
                this.browserLaunchBtn!.classList.remove('loading');
                if (result.success) {
                    this.browserLaunchBtn!.classList.add('success');
                    setTimeout(() => this.browserLaunchBtn!.classList.remove('success'), 2000);
                } else {
                    this.browserLaunchBtn!.classList.add('error');
                    setTimeout(() => this.browserLaunchBtn!.classList.remove('error'), 2000);
                }
            } catch (err) {
                this.browserLaunchBtn!.classList.remove('loading');
                this.browserLaunchBtn!.classList.add('error');
                setTimeout(() => this.browserLaunchBtn!.classList.remove('error'), 2000);
            } finally {
                this.browserLaunchBtn!.disabled = false;
            }
        });

        this.artifactsToggle.addEventListener('click', () => {
            const panel = document.getElementById('artifacts-panel');
            if (panel) {
                panel.classList.toggle('collapsed');
                if (!panel.classList.contains('collapsed')) {
                    const saved = localStorage.getItem('artifacts-panel-width');
                    if (saved) panel.style.width = saved + 'px';
                } else {
                    panel.style.width = '';
                }
            }
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

    updateBrowserStatusIndicator(connected: boolean): void {
        if (!this.browserLaunchBtn) return;
        if (connected) {
            this.browserLaunchBtn.classList.add('connected');
            this.browserLaunchBtn.title = 'Browser Connected (CDP)';
        } else {
            this.browserLaunchBtn.classList.remove('connected');
            this.browserLaunchBtn.title = 'Launch Debug Browser';
        }
    }
}
