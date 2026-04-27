import { t } from '../i18n/index';
import { EventBus } from '../core/event-bus';
import { invoke } from '@tauri-apps/api/core';

export class ModalsView {
    // Confirm Modal
    private confirmModal: HTMLDivElement;
    private confirmMessage!: HTMLParagraphElement;
    private confirmYes!: HTMLButtonElement;
    private confirmNo!: HTMLButtonElement;
    private pendingConfirmation: { taskId: string; resolve: (value: boolean) => void } | null = null;

    // File Preview Modal
    private filePreviewModal: HTMLDivElement;
    private filePreviewName!: HTMLSpanElement;
    private filePreviewSize!: HTMLSpanElement;
    private filePreviewBody!: HTMLDivElement;
    private filePreviewClose!: HTMLButtonElement;
    private filePreviewOpen!: HTMLButtonElement;
    private filePreviewReveal!: HTMLButtonElement;
    private filePreviewCopy!: HTMLButtonElement;

    // Tool Detail Modal
    private toolDetailModal: HTMLDivElement;
    private toolDetailTitle!: HTMLHeadingElement;
    private toolDetailContent!: HTMLPreElement;
    private toolDetailClose!: HTMLButtonElement;
    private toolDetailOk!: HTMLButtonElement;
    private toolDetailCopy!: HTMLButtonElement;

    constructor(private bus: EventBus) {
        this.confirmModal = this.requireElement('confirm-modal');
        this.filePreviewModal = this.requireElement('file-preview-modal');
        this.toolDetailModal = this.requireElement('tool-detail-modal');
    }

    init() {
        this.ensureModalMarkup();
        this.cacheInnerElements();
        this.bindEvents();
    }

    private ensureModalMarkup() {
        if (!this.confirmModal.querySelector('#confirm-yes')) {
            this.confirmModal.innerHTML = `
                <div class="modal-content confirm-modal-content">
                    <h3>${t('common.confirm')}</h3>
                    <p id="confirm-message"></p>
                    <div class="modal-actions">
                        <button id="confirm-no" class="secondary-btn">${t('common.cancel')}</button>
                        <button id="confirm-yes" class="primary-btn">${t('common.confirm')}</button>
                    </div>
                </div>
            `;
        }

        if (!this.filePreviewModal.querySelector('#file-preview-close')) {
            this.filePreviewModal.innerHTML = `
                <div class="modal-content file-preview-modal-content">
                    <div class="modal-header">
                        <div>
                            <span id="file-preview-name" class="file-preview-name"></span>
                            <span id="file-preview-size" class="file-preview-size"></span>
                        </div>
                        <button id="file-preview-close" class="icon-btn" type="button">×</button>
                    </div>
                    <div id="file-preview-body" class="tool-detail-modal-body"></div>
                    <div class="modal-actions">
                        <button id="file-preview-copy" class="secondary-btn">${t('common.copy')}</button>
                        <button id="file-preview-reveal" class="secondary-btn">${t('console.action_reveal')}</button>
                        <button id="file-preview-open" class="primary-btn">${t('preview.open')}</button>
                    </div>
                </div>
            `;
        }

        if (!this.toolDetailModal.querySelector('#tool-detail-close')) {
            this.toolDetailModal.innerHTML = `
                <div class="modal-content tool-detail-modal-content">
                    <div class="modal-header">
                        <h3 id="tool-detail-title"></h3>
                        <button id="tool-detail-close" class="icon-btn" type="button">×</button>
                    </div>
                    <pre id="tool-detail-content" class="tool-detail-modal-body"></pre>
                    <div class="modal-actions">
                        <button id="tool-detail-copy" class="secondary-btn">${t('common.copy')}</button>
                        <button id="tool-detail-ok" class="primary-btn">${t('common.confirm')}</button>
                    </div>
                </div>
            `;
        }
    }

    private cacheInnerElements() {
        this.confirmMessage = this.requireElement('confirm-message');
        this.confirmYes = this.requireElement('confirm-yes');
        this.confirmNo = this.requireElement('confirm-no');

        this.filePreviewName = this.requireElement('file-preview-name');
        this.filePreviewSize = this.requireElement('file-preview-size');
        this.filePreviewBody = this.requireElement('file-preview-body');
        this.filePreviewClose = this.requireElement('file-preview-close');
        this.filePreviewOpen = this.requireElement('file-preview-open');
        this.filePreviewReveal = this.requireElement('file-preview-reveal');
        this.filePreviewCopy = this.requireElement('file-preview-copy');

        this.toolDetailTitle = this.requireElement('tool-detail-title');
        this.toolDetailContent = this.requireElement('tool-detail-content');
        this.toolDetailClose = this.requireElement('tool-detail-close');
        this.toolDetailOk = this.requireElement('tool-detail-ok');
        this.toolDetailCopy = this.requireElement('tool-detail-copy');
    }

    private requireElement<T extends HTMLElement>(id: string): T {
        const element = document.getElementById(id);
        if (!(element instanceof HTMLElement)) {
            throw new Error(`Missing required modal element: ${id}`);
        }

        return element as T;
    }

    private bindEvents() {
        // Confirm Modal
        this.confirmYes.onclick = () => this.handleConfirm(true);
        this.confirmNo.onclick = () => this.handleConfirm(false);

        // Tool Detail Modal
        this.toolDetailClose.onclick = () => this.toolDetailModal.classList.add('hidden');
        this.toolDetailOk.onclick = () => this.toolDetailModal.classList.add('hidden');
        this.toolDetailCopy.onclick = () => {
            const text = this.toolDetailContent.textContent || '';
            if (text) {
                navigator.clipboard.writeText(text);
                this.bus.emit('toast', { message: '✓ ' + t('common.copied') });
            }
        };
        this.toolDetailModal.onclick = (e) => {
            if (e.target === this.toolDetailModal) this.toolDetailModal.classList.add('hidden');
        };

        // File Preview Modal
        this.filePreviewClose.onclick = () => this.filePreviewModal.classList.add('hidden');
        this.filePreviewModal.onclick = (e) => {
            if (e.target === this.filePreviewModal) this.filePreviewModal.classList.add('hidden');
        };
    }

    showConfirmation(taskId: string, message: string): Promise<boolean> {
        return new Promise((resolve) => {
            this.pendingConfirmation = { taskId, resolve };
            this.confirmMessage.textContent = message;
            this.confirmModal.classList.remove('hidden');
        });
    }

    private handleConfirm(approved: boolean) {
        if (!this.pendingConfirmation) return;
        const { resolve } = this.pendingConfirmation;
        resolve(approved);
        this.pendingConfirmation = null;
        this.confirmModal.classList.add('hidden');
    }

    showToolDetail(title: string, content: string) {
        this.toolDetailTitle.textContent = title;
        this.toolDetailContent.textContent = content;
        this.toolDetailModal.classList.remove('hidden');
    }

    async openFilePreview(filePath: string) {
        this.filePreviewModal.classList.remove('hidden');
        this.filePreviewName.textContent = filePath.split(/[\\/]/).pop() || filePath;
        this.filePreviewBody.innerHTML = '<div class="loading-spinner"></div>';
        
        try {
            const info = await invoke<any>('file_stat', { filePath });
            this.filePreviewSize.textContent = this.formatSize(info.size);
            
            // 根据文件类型渲染预览内容
            const ext = filePath.slice(filePath.lastIndexOf('.')).toLowerCase();
            if (['.png', '.jpg', '.jpeg', '.gif', '.webp'].includes(ext)) {
                const res = await invoke<any>('file_read', { filePath });
                this.filePreviewBody.innerHTML = `<img src="${res.dataUrl}" style="max-width:100%; max-height:400px; display:block; margin:0 auto;" />`;
            } else if (['.txt', '.md', '.json', '.js', '.ts', '.rs', '.py', '.toml', '.yaml', '.yml'].includes(ext)) {
                const res = await invoke<any>('file_read_text', { filePath });
                this.filePreviewBody.innerHTML = `<pre class="file-content-preview">${this.escapeHtml(res.content)}</pre>`;
            } else {
                this.filePreviewBody.innerHTML = `<div class="unsupported-preview">${t('preview.not_supported')}</div>`;
            }
            
            this.filePreviewOpen.onclick = () => invoke('file_open', { filePath });
            this.filePreviewReveal.onclick = () => invoke('file_reveal', { filePath });
            this.filePreviewCopy.onclick = () => {
                invoke('file_read_text', { filePath }).then((res: any) => {
                    navigator.clipboard.writeText(res.content);
                    this.bus.emit('toast', { message: '✓ ' + t('common.copied') });
                });
            };
        } catch (err) {
            this.filePreviewBody.innerHTML = `<div class="error-preview">${t('preview.load_failed')}: ${err}</div>`;
        }
    }

    private formatSize(bytes: number): string {
        if (bytes < 1024) return bytes + ' B';
        if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
        return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
    }

    private escapeHtml(text: string): string {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}
