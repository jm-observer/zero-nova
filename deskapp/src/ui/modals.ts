import { t } from '../i18n/index';
import { AppState } from '../core/state';
import { EventBus } from '../core/event-bus';
import { invoke } from '@tauri-apps/api/core';
import { renderMarkdown } from '../markdown';

export class ModalsView {
    // Confirm Modal
    private confirmModal: HTMLDivElement;
    private confirmMessage: HTMLParagraphElement;
    private confirmYes: HTMLButtonElement;
    private confirmNo: HTMLButtonElement;
    private pendingConfirmation: { taskId: string; resolve: (value: boolean) => void } | null = null;

    // File Preview Modal
    private filePreviewModal: HTMLDivElement;
    private filePreviewIcon: HTMLSpanElement;
    private filePreviewName: HTMLSpanElement;
    private filePreviewSize: HTMLSpanElement;
    private filePreviewBody: HTMLDivElement;
    private filePreviewClose: HTMLButtonElement;
    private filePreviewOpen: HTMLButtonElement;
    private filePreviewReveal: HTMLButtonElement;
    private filePreviewCopy: HTMLButtonElement;

    // Tool Detail Modal
    private toolDetailModal: HTMLDivElement;
    private toolDetailTitle: HTMLHeadingElement;
    private toolDetailContent: HTMLPreElement;
    private toolDetailClose: HTMLButtonElement;
    private toolDetailOk: HTMLButtonElement;
    private toolDetailCopy: HTMLButtonElement;

    constructor(private state: AppState, private bus: EventBus) {
        this.confirmModal = document.getElementById('confirm-modal') as HTMLDivElement;
        this.confirmMessage = document.getElementById('confirm-message') as HTMLParagraphElement;
        this.confirmYes = document.getElementById('confirm-yes') as HTMLButtonElement;
        this.confirmNo = document.getElementById('confirm-no') as HTMLButtonElement;

        this.filePreviewModal = document.getElementById('file-preview-modal') as HTMLDivElement;
        this.filePreviewIcon = document.getElementById('file-preview-icon') as HTMLSpanElement;
        this.filePreviewName = document.getElementById('file-preview-name') as HTMLSpanElement;
        this.filePreviewSize = document.getElementById('file-preview-size') as HTMLSpanElement;
        this.filePreviewBody = document.getElementById('file-preview-body') as HTMLDivElement;
        this.filePreviewClose = document.getElementById('file-preview-close') as HTMLButtonElement;
        this.filePreviewOpen = document.getElementById('file-preview-open') as HTMLButtonElement;
        this.filePreviewReveal = document.getElementById('file-preview-reveal') as HTMLButtonElement;
        this.filePreviewCopy = document.getElementById('file-preview-copy') as HTMLButtonElement;

        this.toolDetailModal = document.getElementById('tool-detail-modal') as HTMLDivElement;
        this.toolDetailTitle = document.getElementById('tool-detail-title') as HTMLHeadingElement;
        this.toolDetailContent = document.getElementById('tool-detail-content') as HTMLPreElement;
        this.toolDetailClose = document.getElementById('tool-detail-close') as HTMLButtonElement;
        this.toolDetailOk = document.getElementById('tool-detail-ok') as HTMLButtonElement;
        this.toolDetailCopy = document.getElementById('tool-detail-copy') as HTMLButtonElement;
    }

    init() {
        this.bindEvents();
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
