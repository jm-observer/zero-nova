import { t } from '../i18n/index';
import { AppState } from '../core/state';
import { EventBus, Events } from '../core/event-bus';
import { invoke } from '@tauri-apps/api/core';
import { save as tauriDialogSave } from '@tauri-apps/plugin-dialog';


export type { SessionArtifactView } from './types';

export type ArtifactCategory = 'all' | 'document' | 'code' | 'image' | 'data' | 'media' | 'other';

const CATEGORY_EXT_MAP: Record<string, ArtifactCategory> = {
    md: 'document', txt: 'document', pdf: 'document',
    doc: 'document', docx: 'document',
    py: 'code', js: 'code', ts: 'code', rs: 'rs',
    png: 'image', jpg: 'image', jpeg: 'image',
    csv: 'data', xls: 'data', xlsx: 'data',
    mp4: 'media', mp3: 'media',
};

const CATEGORY_ICONS: Record<ArtifactCategory, string> = {
    all: '📁', document: '📝', code: '💻', image: '🖼️', data: '📊', media: '🎬', other: '📋',
};

export class ArtifactsView {
    private panel: HTMLElement;
    private list: HTMLDivElement;
    private filterTabs: HTMLDivElement;
    private activeFilter: ArtifactCategory = 'all';
    private artifacts: SessionArtifactView[] = [];
    private addedPaths = new Set<string>();

    constructor(private state: AppState, private bus: EventBus) {
        this.panel = document.getElementById('artifacts-panel') as HTMLElement;
        this.list = document.getElementById('artifacts-list') as HTMLDivElement;
        this.filterTabs = document.getElementById('artifacts-filter-tabs') as HTMLDivElement;
    }

    init() {
        this.bus.on(Events.SESSION_SELECTED, (payload: { sessionId: string }) => {
            if (payload.sessionId) {
                this.loadArtifacts(payload.sessionId);
            } else {
                this.clear();
            }
        });

        this.bus.on('artifact:added', (payload: { artifact: SessionArtifactView }) => {
            this.addArtifact(payload.artifact);
        });

        this.initResize();
    }

    private initResize() {
        const handle = document.getElementById('artifacts-resize-handle');
        if (!handle) return;

        const ARTIFACTS_MIN = 200, ARTIFACTS_MAX = 600;
        
        handle.addEventListener('mousedown', (e) => {
            if (this.panel.classList.contains('collapsed')) return;
            
            const startX = e.clientX;
            const startWidth = this.panel.getBoundingClientRect().width;
            
            handle.classList.add('active');
            document.body.classList.add('resizing');
            
            const onMove = (ev: MouseEvent) => {
                const diff = ev.clientX - startX;
                const newW = Math.min(ARTIFACTS_MAX, Math.max(ARTIFACTS_MIN, startWidth - diff));
                this.panel.style.width = newW + 'px';
            };
            
            const onUp = () => {
                document.removeEventListener('mousemove', onMove);
                document.removeEventListener('mouseup', onUp);
                handle.classList.remove('active');
                document.body.classList.remove('resizing');
                const w = this.panel.getBoundingClientRect().width;
                localStorage.setItem('artifacts-panel-width', String(Math.round(w)));
            };
            
            document.addEventListener('mousemove', onMove);
            document.addEventListener('mouseup', onUp);
        });
    }

    clear() {
        this.artifacts = [];
        this.list.innerHTML = '';
        this.addedPaths.clear();
        this.activeFilter = 'all';
        this.updateFilterTabs();
    }

    async loadArtifacts(sessionId: string) {
        if (!this.state.gatewayClient) return;
        this.clear();
        try {
            // 使用较短的超时时间，如果后端没实现或没准备好，不阻塞 UI 
            const artifacts = await this.state.gatewayClient.getArtifacts(sessionId).catch(e => {
                console.warn('[Artifacts] Historical artifacts not available or timeout:', e);
                return [] as SessionArtifactView[];
            });
            
            for (const art of artifacts) {
                this.addArtifact(art, false);
            }
        } catch (err) {
            // 彻底捕获，保证不向上传播
            console.error('[Artifacts] Unexpected load error:', err);
        }
    }

    async addArtifact(artifact: SessionArtifactView, persist = true) {
        if (artifact.path && this.addedPaths.has(artifact.path)) return;
        if (artifact.path) this.addedPaths.add(artifact.path);

        this.artifacts.push(artifact);
        // 如果是新添加的（不是从历史加载的），显示面板
        if (persist) {
            this.panel.classList.remove('collapsed');
        }

        const dateKey = this.getDateKey(artifact.timestamp);
        const group = this.ensureDateGroup(dateKey);
        
        const item = this.createItemElement(artifact);
        group.appendChild(item);

        this.updateFilterTabs();
        this.applyFilter();
    }

    private createItemElement(artifact: SessionArtifactView): HTMLDivElement {
        const item = document.createElement('div');
        item.className = 'artifact-item';
        item.dataset.category = this.getCategory(artifact);
        item.dataset.timestamp = String(artifact.timestamp);

        const filename = artifact.filename || artifact.path?.split(/[/\\]/).pop() || 'Unknown';
        const icon = this.getFileIcon(filename);

        item.innerHTML = `
            <div class="artifact-icon">${icon}</div>
            <div class="artifact-info">
                <div class="artifact-name">${this.escapeHtml(filename)}</div>
                <div class="artifact-path">${this.escapeHtml(artifact.path || '')}</div>
            </div>
            <div class="artifact-actions">
                <button class="artifact-action-btn" data-action="open" title="${t('preview.open')}">
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
                </button>
            </div>
        `;

        item.addEventListener('dblclick', () => {
            if (artifact.path) {
                 this.bus.emit('file:preview', { path: artifact.path });
            }
        });

        item.querySelector('[data-action="open"]')?.addEventListener('click', (e) => {
            e.stopPropagation();
            if (artifact.path) invoke('file_open', { filePath: artifact.path });
        });

        return item;
    }

    private getCategory(artifact: SessionArtifactView): ArtifactCategory {
        const fname = artifact.filename || artifact.path?.split(/[/\\]/).pop() || '';
        const ext = fname.split('.').pop()?.toLowerCase() || '';
        return CATEGORY_EXT_MAP[ext] || 'other';
    }

    private getFileIcon(filename: string): string {
        const ext = filename.split('.').pop()?.toLowerCase() || '';
        const icons: Record<string, string> = {
            py: '🐍', js: '📜', ts: '📜',
            html: '🌐', css: '🎨', json: '📋',
            md: '📝', png: '🖼️', jpg: '🖼️',
            pdf: '📕', zip: 'Box',
        };
        return icons[ext] || '📄';
    }

    private getDateKey(ts: number): string {
        const d = new Date(ts);
        return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    }

    private ensureDateGroup(dateKey: string): HTMLDivElement {
        let group = this.list.querySelector(`.artifact-date-group[data-date="${dateKey}"]`) as HTMLDivElement | null;
        if (!group) {
            group = document.createElement('div');
            group.className = 'artifact-date-group';
            group.dataset.date = dateKey;
            group.innerHTML = `<div class="artifact-date-header">${dateKey}</div>`;
            this.list.appendChild(group);
        }
        return group;
    }

    private updateFilterTabs() {
        // Implementation similar to main.ts but scoped to this class
    }

    private applyFilter() {
        const items = this.list.querySelectorAll('.artifact-item') as NodeListOf<HTMLElement>;
        items.forEach(item => {
            if (this.activeFilter === 'all' || item.dataset.category === this.activeFilter) {
                item.style.display = '';
            } else {
                item.style.display = 'none';
            }
        });
    }

    private escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}
