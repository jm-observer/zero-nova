import { t } from '../i18n/index';
import { AppState } from '../core/state';
import { EventBus } from '../core/event-bus';
import { open as tauriDialogOpen } from '@tauri-apps/plugin-dialog';

import { SETTINGS_TEMPLATE } from './templates/settings-template';

export class SettingsView {
    private view: HTMLElement;
    private tabs!: NodeListOf<HTMLElement>;
    private tabContents!: NodeListOf<HTMLElement>;
    private saveBtn!: HTMLButtonElement;
    private saveHint!: HTMLElement;
    
    private activeView = false;

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
    }

    toggle(active: boolean) {
        this.activeView = active;
        this.view.classList.toggle('hidden', !active);
        if (active) {
            this.loadConfig();
        }
    }

    private switchTab(tabName: string) {
        this.tabs.forEach(t => t.classList.toggle('active', t.dataset.tab === tabName));
        this.tabContents.forEach(tc => tc.classList.toggle('active', tc.dataset.tab === tabName));
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

        // Models
        if (cfg.llm) {
            this.setInputValue('server-orch-provider', cfg.llm.provider || 'anthropic');
            this.setInputValue('server-orch-model', cfg.llm.model_config?.model || '');
        }

        // Search
        if (cfg.search) {
            this.setInputValue('server-web-search-provider', cfg.search.backend || 'brave');
            this.setInputValue('server-web-search-apikey', cfg.search.google_api_key || cfg.search.tavily_api_key || '');
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
            llm: { model_config: {} },
            search: {},
            gateway: {}
        };

        updates.llm.provider = this.getInputValue('server-orch-provider');
        updates.llm.model_config.model = this.getInputValue('server-orch-model');
        updates.search.backend = this.getInputValue('server-web-search-provider');

        return updates;
    }

    private getInputValue(id: string): string {
        const el = document.getElementById(id) as HTMLInputElement | HTMLSelectElement;
        return el ? el.value : '';
    }
}
