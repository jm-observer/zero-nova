import { t } from '../i18n/index';
import { AppState } from '../core/state';
import { EventBus, Events } from '../core/event-bus';
import type { SettingsNavigatePayload } from '../core/types';

import { SETTINGS_TEMPLATE } from './templates/settings-template';

export class SettingsView {
    private view: HTMLElement;
    private tabs!: NodeListOf<HTMLElement>;
    private tabContents!: NodeListOf<HTMLElement>;
    private saveBtn!: HTMLButtonElement;
    private saveHint!: HTMLElement;
    
    constructor(private state: AppState, private bus: EventBus) {
        this.view = document.getElementById('settings-view') as HTMLElement;
        if (this.view) {
            this.view.innerHTML = SETTINGS_TEMPLATE;
            this.refreshElements();
        }
    }

    private refreshElements() {
        this.tabs = this.view.querySelectorAll('.settings-tab');
        this.tabContents = this.view.querySelectorAll('.settings-tab-content');
        this.saveBtn = document.getElementById('server-save-btn') as HTMLButtonElement;
        this.saveHint = document.getElementById('server-save-hint') as HTMLElement;
    }

    init() {
        this.tabs.forEach(tab => {
            tab.addEventListener('click', () => this.switchTab(tab.dataset.tab || 'general'));
        });

        this.saveBtn.addEventListener('click', () => this.saveConfig());
        
        this.bus.on('view:toggle', (payload: { name: string; active: boolean }) => {
            if (payload.name === 'settings') {
                this.toggle(payload.active);
            } else if (payload.active) {
                this.toggle(false); // Close settings if another view opens
            }
        });

        this.bus.on<SettingsNavigatePayload>(Events.SETTINGS_NAVIGATE, payload => {
            if (!payload?.visible) {
                return;
            }

            this.toggle(true);
            this.bus.emit('view:toggle', { name: 'settings', active: true });
            const targetTab = this.resolveTabName(payload.section);
            this.switchTab(targetTab);
            this.focusItem(payload.itemId, payload.search);
        });
    }

    toggle(active: boolean) {
        this.view.classList.toggle('hidden', !active);
        if (active) {
            this.loadConfig();
        }
    }

    private switchTab(tabName: string) {
        this.tabs.forEach(t => t.classList.toggle('active', t.dataset.tab === tabName));
        this.tabContents.forEach(tc => tc.classList.toggle('active', tc.dataset.tab === tabName));
    }

    private resolveTabName(section?: SettingsNavigatePayload['section']): string {
        switch (section) {
            case 'models':
                return 'models';
            case 'memory':
                return 'memory';
            case 'mcp':
            case 'skills':
                return 'tools';
            default:
                return 'general';
        }
    }

    private focusItem(itemId?: string, search?: string): void {
        requestAnimationFrame(() => {
            if (itemId) {
                const el = this.view.querySelector(`[data-item-id="${itemId}"]`) as HTMLElement | null;
                if (el) {
                    el.scrollIntoView({ behavior: 'smooth', block: 'center' });
                    el.classList.add('highlight-flash');
                    window.setTimeout(() => el.classList.remove('highlight-flash'), 2000);
                    return;
                }
            }

            if (search) {
                const lowerSearch = search.toLowerCase();
                const candidates = Array.from(this.view.querySelectorAll('[data-item-id]')) as HTMLElement[];
                const matched = candidates.find(el => (el.dataset.itemId || '').toLowerCase().includes(lowerSearch));
                if (matched) {
                    matched.scrollIntoView({ behavior: 'smooth', block: 'center' });
                    matched.classList.add('highlight-flash');
                    window.setTimeout(() => matched.classList.remove('highlight-flash'), 2000);
                }
            }
        });
    }

    private async loadConfig() {
        if (!this.state.gatewayClient) return;
        try {
            const cfg = await this.state.gatewayClient.getServerConfig();
            this.populateUI(cfg);
        } catch (err) {
            console.error('[Settings] Load config failed:', err);
        }
    }

    private populateUI(cfg: any) {
        if (!cfg) return;

        // General
        this.setInputValue('locale-select', cfg.locale || 'zh');
        this.setInputValue('output-path-input', cfg.outputPath || '');
        this.setCheckboxValue('tts-autoplay-toggle', cfg.voice?.ttsAutoplay);
        this.setInputValue('tts-voice-select', cfg.voice?.ttsVoice);
        this.setCheckboxValue('debug-mode-toggle', cfg.debug);

        // Models - Orchestration
        if (cfg.llm) {
            this.setInputValue('server-orch-provider', cfg.llm.orchestration?.provider || 'anthropic');
            this.setInputValue('server-orch-model', cfg.llm.orchestration?.model || '');
        }

        // Models - Execution
        if (cfg.llm?.execution) {
            this.setInputValue('server-exec-provider', cfg.llm.execution.provider || 'openai');
            this.setInputValue('server-exec-model', cfg.llm.execution.model || '');
        }

        // Search
        if (cfg.web?.search) {
            this.setInputValue('server-web-search-provider', cfg.web.search.provider || 'brave');
            this.setInputValue('server-web-search-apikey', cfg.web.search.apiKey || '');
            this.setInputValue('server-web-search-max-results', cfg.web.search.maxResults || 5);
        }

        // Fetch
        if (cfg.web?.fetch) {
            this.setCheckboxValue('server-web-fetch-readability', cfg.web.fetch.readability);
            this.setInputValue('server-web-fetch-max-chars', cfg.web.fetch.maxChars || 50000);
        }
    }

    private setInputValue(id: string, value: any) {
        const el = document.getElementById(id) as HTMLInputElement | HTMLSelectElement;
        if (el) el.value = value || '';
    }

    private setCheckboxValue(id: string, checked: boolean) {
        const el = document.getElementById(id) as HTMLInputElement;
        if (el) el.checked = !!checked;
    }

    private async saveConfig() {
        if (!this.state.gatewayClient) return;
        
        this.saveBtn.disabled = true;
        this.saveHint.textContent = t('settings.saving');
        
        try {
            const updates = this.collectUpdates();
            const result = await this.state.gatewayClient.updateServerConfig(updates);
            if (result.success) {
                this.saveHint.textContent = t('common.save_success');
                this.saveHint.className = 'settings-save-hint success';
            } else {
                this.saveHint.textContent = result.message || t('common.save_failed');
                this.saveHint.className = 'settings-save-hint error';
            }
        } catch (err) {
            this.saveHint.textContent = String(err);
            this.saveHint.className = 'settings-save-hint error';
        } finally {
            this.saveBtn.disabled = false;
        }
    }

    private collectUpdates(): any {
        const updates: any = {
            llm: { 
                orchestration: {},
                execution: {}
            },
            web: {
                search: {},
                fetch: {}
            }
        };

        updates.llm.orchestration.provider = this.getInputValue('server-orch-provider');
        updates.llm.orchestration.model = this.getInputValue('server-orch-model');
        
        updates.llm.execution.provider = this.getInputValue('server-exec-provider');
        updates.llm.execution.model = this.getInputValue('server-exec-model');

        updates.web.search.provider = this.getInputValue('server-web-search-provider');
        updates.web.search.apiKey = this.getInputValue('server-web-search-apikey');
        updates.web.search.maxResults = parseInt(this.getInputValue('server-web-search-max-results')) || 5;

        updates.web.fetch.readability = this.getCheckboxValue('server-web-fetch-readability');
        updates.web.fetch.maxChars = parseInt(this.getInputValue('server-web-fetch-max-chars')) || 50000;

        return updates;
    }

    private getCheckboxValue(id: string): boolean {
        const el = document.getElementById(id) as HTMLInputElement;
        return el ? el.checked : false;
    }

    private getInputValue(id: string): string {
        const el = document.getElementById(id) as HTMLInputElement | HTMLSelectElement;
        return el ? el.value : '';
    }
}
