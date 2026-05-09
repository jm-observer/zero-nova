import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { t } from '../i18n/index';
import { AppState } from '../core/state';
import { EventBus, Events } from '../core/event-bus';
import type { ProviderHealthSnapshotView } from '../core/types';

export class TitleBarView {
    private statusIndicator: HTMLDivElement;
    private btnMinimize: HTMLButtonElement;
    private btnMaximize: HTMLButtonElement;
    private btnClose: HTMLButtonElement;
    private themeToggle: HTMLButtonElement;
    private gatewayConnectionStatus: 'connecting' | 'connected' | 'disconnected' | 'reconnecting' | 'failed' = 'connecting';
    private runtimeStatusText: string | null = null;
    private providerHealthByScope = new Map<string, ProviderHealthSnapshotView>();

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

        // GATEWAY_STATUS 现在仅用于连接态
        this.bus.on(Events.GATEWAY_STATUS, (payload: { connectionStatus?: string }) => {
            if (payload.connectionStatus) {
                this.gatewayConnectionStatus = payload.connectionStatus;
                if (payload.connectionStatus !== 'connected') {
                    // 连接态变化到非 connected 时，清理临时运行文案，避免断线后显示陈旧的 running 文案
                    this.runtimeStatusText = null;
                }
                this.updateAggregateStatus();
            }
        });

        // RUNTIME_STATUS_TEXT 用于运行文案（如 Agent Running (x/y)）
        this.bus.on(Events.RUNTIME_STATUS_TEXT, (payload: { text: string }) => {
            console.debug('[TitleBar] Runtime status text updated:', payload.text);
            this.runtimeStatusText = payload.text;
            this.updateAggregateStatus();
        });

        this.bus.on(Events.RUNTIME_STATUS_TEXT_CLEAR, () => {
            this.runtimeStatusText = null;
            this.updateAggregateStatus();
        });

        this.bus.on(Events.PROVIDER_HEALTH_UPDATED, (payload: { providers?: ProviderHealthSnapshotView[] }) => {
            this.handleProviderHealthChange(payload?.providers ?? []);
        });
    }

    private handleGatewayStatusChange(payload: any) {
        const { status, text, connectionStatus } = typeof payload === 'string'
            ? { status: payload, text: null, connectionStatus: undefined }
            : payload;

        if (typeof connectionStatus === 'string') {
            this.gatewayConnectionStatus = connectionStatus;
            this.updateAggregateStatus();
            return;
        }

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

    private handleProviderHealthChange(providers: ProviderHealthSnapshotView[]) {
        this.providerHealthByScope.clear();
        providers.forEach((provider) => {
            this.providerHealthByScope.set(provider.scope, provider);
        });
        this.updateAggregateStatus();
    }

    private updateAggregateStatus() {
        if (this.gatewayConnectionStatus !== 'connected') {
            this.handleGatewayStatusChange({ status: this.gatewayConnectionStatus });
            return;
        }

        const providers = [...this.providerHealthByScope.values()];
        
        // 收集错误信息用于 Tooltip
        const issueProviders = providers.filter(p => 
            ['auth_failed', 'unreachable', 'misconfigured', 'degraded'].includes(p.status)
        );
        
        if (issueProviders.length > 0) {
            const diagnosticMsg = issueProviders
                .map(p => `${p.scope}: ${p.status}${p.message ? ` (${p.message})` : ''}`)
                .join('\n');
            this.statusIndicator.title = diagnosticMsg;
        } else {
            this.statusIndicator.title = '';
        }

        // 严重错误：如果所有 Provider 都挂了，才显示 error
        // 否则，网关连着就应该是 ready (Green) 或 running (Yellow)
        // 注意：misconfigured (如缺少 API Key) 视为非致命错误，不触发红色指示灯
        const hasFatalError = providers.length > 0 && providers.every(item => 
            ['auth_failed', 'unreachable'].includes(item.status)
        );

        if (hasFatalError) {
            this.setStatus(t('status.gateway_connected_provider_error'), 'error');
            return;
        }

        // Provider 未报错时，允许临时运行文案覆盖文案层，但不改变连接态事实来源
        if (this.runtimeStatusText) {
            this.setStatus(this.runtimeStatusText, 'running');
            return;
        }

        if (providers.length === 0) {
            this.setStatus(t('status.gateway_connected_provider_unknown'), 'running');
            return;
        }

        // 降级状态或检查中，显示 Yellow (running) 但文案提示降级
        if (providers.some((item) => item.status === 'degraded' || item.status === 'checking' || item.status === 'unknown')) {
            this.setStatus(t('status.gateway_connected_provider_degraded'), 'running');
            return;
        }

        // 只要网关连接且没有全量失效，默认就是 Ready (Green)
        this.setStatus(t('status.gateway_connected_provider_healthy'), 'ready');
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

