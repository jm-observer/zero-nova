import { invoke } from '@tauri-apps/api/core';
import { t } from '../i18n/index';
import { AppState } from '../core/state';
import { EventBus, Events } from '../core/event-bus';
import { renderMarkdown } from '../markdown';
import { escapeHtml, formatTime } from '../utils/html';
import type { SessionFileTreeEntryView, SessionRuntimeSnapshot } from '../core/types';

type ProjectDirEntry = SessionFileTreeEntryView;

export class ChatView {
    private messagesContainer: HTMLElement;
    private messageInput: HTMLTextAreaElement;
    private sendBtn: HTMLButtonElement;
    private inspectBtn: HTMLButtonElement;
    private inputContainer: HTMLElement;
    private inputRow: HTMLElement;
    private projectMenuBtn: HTMLButtonElement | null = null;
    private projectMenuEl: HTMLElement | null = null;
    private projectMenuVisible = false;
    private projectPickerEl: HTMLElement | null = null;
    private pickerVisible = false;
    private pickerLoading = false;
    private pickerTokenStart = -1;
    private pickerCurrentPath = '';
    private pickerEntries: ProjectDirEntry[] = [];
    private pickerFilteredEntries: ProjectDirEntry[] = [];
    private pickerFilterKeyword = '';
    private pickerActiveIndex = 0;
    private pickerReqSeq = 0;
    private pickerComposing = false;
    private pickerErrorMessage: string | null = null;
    private sessionFileTreeCache = new Map<string, Map<string, ProjectDirEntry[]>>();
    private sessionProjectDirState = new Map<string, string | null>();
    
    private streamingMessageEl: HTMLElement | null = null;
    private streamingContent = ''; // 仅作向后兼容和备份
    private currentIntentText: string | null = null;
    private layoutObserver: ResizeObserver | null = null;

    constructor(private state: AppState, private bus: EventBus) {
        this.messagesContainer = document.getElementById('messages') as HTMLElement;
        this.messageInput = document.getElementById('message-input') as HTMLTextAreaElement;
        this.sendBtn = document.getElementById('send-btn') as HTMLButtonElement;
        this.inspectBtn = document.getElementById('inspect-btn') as HTMLButtonElement;
        this.inputContainer = document.querySelector('.input-container') as HTMLElement;
        this.inputRow = (this.inputContainer.querySelector('.input-row') as HTMLElement) ?? this.inputContainer;
    }

    init() {
        console.log('[ChatView] Initializing...');
        this.ensureProjectMenu();
        this.bindEvents();
        
        // 监听消息容器大小变化，更新右侧的 Minimap 导航条
        if (window.ResizeObserver) {
            this.layoutObserver = new ResizeObserver(() => {
                this.updateMinimap();
            });
            this.layoutObserver.observe(this.messagesContainer);
        }
        
        this.bus.on(Events.SESSION_SELECTED, () => {
            this.updateHeaderTitle();
            this.hideProjectPicker();
            this.hideProjectMenu();
            void this.refreshProjectMenuState();
        });

        this.state.gatewayClient?.onSessionRuntimeUpdated((payload) => {
            this.handleSessionRuntimeUpdated(payload);
        });
        
        this.bus.on(Events.SESSION_CHANGED, (payload: any) => {
             console.log('[ChatView] Session changed:', payload.previousSessionId, '->', payload.sessionId);
             if (payload.previousSessionId) {
                 this.invalidateSessionFileTreeCache(payload.previousSessionId);
             }
             // 如果是从初始状态 (null) 切换到第一个会话，说明正在通过首条消息建立会话，
             // 此时应保留当前显示的乐观消息（用户刚发出的那一条），不执行清空。
             if (payload.previousSessionId === null && payload.sessionId !== null) {
                 return;
             }
             this.clear();
        });

        this.bus.on(Events.MESSAGES_UPDATED, (payload: any) => {
             console.log('[ChatView] Messages updated, rendering...', payload.messages.length);
             this.renderMessages(payload.messages);
        });

        this.bus.on(Events.MESSAGE_ADDED, (payload: any) => {
             console.log('[ChatView] New message added:', payload.message.id);
             this.addMessage(payload.message);
        });

        this.bus.on('token', (payload: { sessionId: string, token: string }) => {
             if (payload.sessionId === this.state.currentSessionId) {
                 this.appendToken(payload.token);
             }
        });

        this.bus.on('chat:complete', (payload: any) => {
             if (payload.sessionId === this.state.currentSessionId) {
                 console.log('[ChatView] Chat complete, resetting streaming state');
                 this.streamingMessageEl = null;
                 this.streamingContent = '';
                 this.invalidateSessionFileTreeCache(payload.sessionId);
             }
        });

        this.bus.on(Events.CHAT_INTENT, (payload: any) => {
            this.handleIntent(payload);
        });
        
        this.bus.on('tool:log', (event: any) => {
            this.handleToolLog(event);
        });

        this.bus.on('tool:start', (event: any) => {
            if (event.sessionId === this.state.currentSessionId) {
                this.handleToolStart(event);
            }
        });

        this.bus.on('tool:result', (event: any) => {
            if (event.sessionId === this.state.currentSessionId) {
                this.handleToolResult(event);
            }
        });
        
        this.bus.on('chat:error', (payload: any) => {
            if (payload.sessionId === this.state.currentSessionId) {
                this.handleChatError(payload);
            }
        });

        this.bus.on('system:log', (event: any) => {
            if (event.sessionId === this.state.currentSessionId) {
                this.handleSystemLog(event);
            }
        });

        this.bus.on('chat:iteration', (event: any) => {
            if (event.sessionId === this.state.currentSessionId) {
                this.handleIteration(event);
            }
        });
    }

    private bindEvents() {
        this.sendBtn.addEventListener('click', () => this.sendMessage());
        this.inspectBtn?.addEventListener('click', () => {
            if (!this.state.consoleVisible) {
                this.state.setConsoleTab('overview');
            }
            this.state.setConsoleVisible(!this.state.consoleVisible);
        });
        this.messageInput.addEventListener('keydown', (e) => {
            if (this.handleProjectPickerKeydown(e)) {
                return;
            }
            if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                this.sendMessage();
            }
        });
        this.messageInput.addEventListener('input', () => {
            void this.syncProjectPicker();
        });
        this.messageInput.addEventListener('blur', () => {
            requestAnimationFrame(() => {
                const active = document.activeElement;
                if (active && this.projectPickerEl?.contains(active)) {
                    return;
                }
                this.hideProjectPicker();
            });
        });
        this.messageInput.addEventListener('compositionstart', () => {
            this.pickerComposing = true;
        });
        this.messageInput.addEventListener('compositionend', () => {
            this.pickerComposing = false;
            void this.syncProjectPicker();
        });

        this.messagesContainer.addEventListener('contextmenu', (e) => this.handleContextMenu(e));
        document.addEventListener('click', () => {
            this.hideContextMenu();
            this.hideProjectMenu();
        });

        // 工具卡片折叠监听
        this.messagesContainer.addEventListener('click', (e) => {
            const target = e.target as HTMLElement;
            const traceCopyBtn = target.closest('.message-trace-copy-btn') as HTMLButtonElement | null;
            if (traceCopyBtn) {
                void this.handleTraceCopyClick(traceCopyBtn);
                return;
            }
            // 允许点击整个 Header 或 Header 内部的任何元素
            const header = target.closest('.tool-name, .tool-result-header');
            if (header) {
                const card = header.closest('.tool-use-card, .tool-result-card');
                if (card) {
                    card.classList.toggle('collapsed');
                    // 触发布局更刷新，确保导航条位置正确
                    this.updateMinimap();
                }
            } else {
                // 如果直接点击了已折叠卡片的空白处，也执行展开
                const collapsedCard = target.closest('.collapsible.collapsed');
                if (collapsedCard) {
                    collapsedCard.classList.remove('collapsed');
                    this.updateMinimap();
                }
            }
        });
    }

    private ensureProjectMenu() {
        if (this.projectMenuBtn && this.projectMenuEl) {
            this.renderProjectMenu();
            return;
        }

        this.projectMenuBtn = document.createElement('button');
        this.projectMenuBtn.type = 'button';
        this.projectMenuBtn.className = 'project-menu-trigger';
        this.projectMenuBtn.addEventListener('click', (event) => {
            event.stopPropagation();
            this.projectMenuVisible = !this.projectMenuVisible;
            this.renderProjectMenu();
        });

        this.projectMenuEl = document.createElement('div');
        this.projectMenuEl.className = 'project-menu';
        this.projectMenuEl.addEventListener('click', (event) => {
            event.stopPropagation();
        });

        this.inputContainer.insertBefore(this.projectMenuBtn, this.inputRow);
        this.inputContainer.appendChild(this.projectMenuEl);
        this.renderProjectMenu();
    }

    private async refreshProjectMenuState() {
        const sessionId = this.state.currentSessionId;
        if (!sessionId) {
            this.renderProjectMenu();
            return;
        }

        const runtimeState = this.state.getSessionResourceState(sessionId, 'runtime') as { data?: SessionRuntimeSnapshot } | undefined;
        if (runtimeState?.data) {
            this.sessionProjectDirState.set(sessionId, runtimeState.data.projectDir ?? null);
            this.renderProjectMenu();
            return;
        }

        if (!this.state.gatewayClient) {
            this.renderProjectMenu();
            return;
        }

        try {
            const runtime = await this.state.gatewayClient.getSessionRuntime(sessionId);
            this.state.updateSessionResourceState(sessionId, 'runtime', this.state.setLoadedResource(runtime));
            this.sessionProjectDirState.set(sessionId, runtime.projectDir ?? null);
        } catch (error) {
            console.warn('[ChatView] Failed to refresh session runtime for project menu:', error);
        }

        this.renderProjectMenu();
    }

    private renderProjectMenu() {
        if (!this.projectMenuBtn || !this.projectMenuEl) {
            return;
        }

        const sessionId = this.state.currentSessionId;
        const projectDir = sessionId ? this.sessionProjectDirState.get(sessionId) : null;
        const basename = projectDir ? projectDir.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || projectDir : t('chat.project_not_set');
        this.projectMenuBtn.textContent = `${t('chat.project_label')}: ${basename}`;
        this.projectMenuBtn.disabled = !sessionId;

        if (!this.projectMenuVisible) {
            this.projectMenuEl.classList.remove('visible');
            this.projectMenuEl.innerHTML = '';
            return;
        }

        this.projectMenuEl.classList.add('visible');
        const disabledAttr = projectDir ? '' : 'disabled';
        const pathHtml = projectDir ? escapeHtml(projectDir) : escapeHtml(t('chat.project_not_set_long'));
        this.projectMenuEl.innerHTML = `
            <div class="project-menu-path">${pathHtml}</div>
            <div class="project-menu-hint">${escapeHtml(t('chat.project_hint'))}</div>
            <div class="project-menu-actions">
                <button type="button" class="project-menu-action" data-action="copy" ${disabledAttr}>${escapeHtml(t('chat.project_copy_path'))}</button>
                <button type="button" class="project-menu-action" data-action="open" ${disabledAttr}>${escapeHtml(t('chat.project_open_dir'))}</button>
                <button type="button" class="project-menu-action" data-action="refresh">${escapeHtml(t('chat.project_refresh'))}</button>
            </div>
        `;

        this.projectMenuEl.querySelectorAll('.project-menu-action').forEach((element) => {
            element.addEventListener('click', () => {
                const action = (element as HTMLElement).dataset.action;
                if (action === 'copy') {
                    void this.copyProjectPath();
                } else if (action === 'open') {
                    void this.openProjectDir();
                } else if (action === 'refresh') {
                    void this.refreshProjectMenuState();
                }
            });
        });
    }

    private hideProjectMenu() {
        this.projectMenuVisible = false;
        if (this.projectMenuEl) {
            this.projectMenuEl.classList.remove('visible');
            this.projectMenuEl.innerHTML = '';
        }
    }

    private getCurrentProjectDir(): string | null {
        const sessionId = this.state.currentSessionId;
        if (!sessionId) {
            return null;
        }

        const runtimeState = this.state.getSessionResourceState(sessionId, 'runtime') as { data?: SessionRuntimeSnapshot } | undefined;
        if (runtimeState?.data?.projectDir !== undefined) {
            return runtimeState.data.projectDir ?? null;
        }

        return this.sessionProjectDirState.get(sessionId) ?? null;
    }

    private async copyProjectPath() {
        const projectDir = this.getCurrentProjectDir();
        if (!projectDir) {
            return;
        }
        await navigator.clipboard.writeText(projectDir);
        this.hideProjectMenu();
    }

    private async openProjectDir() {
        const projectDir = this.getCurrentProjectDir();
        if (!projectDir) {
            return;
        }
        await invoke('file_open', { filePath: projectDir });
        this.hideProjectMenu();
    }

    private async handleTraceCopyClick(button: HTMLButtonElement) {
        if (button.disabled) {
            return;
        }
        const bodyType = button.dataset.bodyType;
        const messageId = button.dataset.messageId;
        if (!bodyType || !messageId) {
            this.bus.emit('toast', { message: t('chat.copy_body_data_invalid') });
            return;
        }
        const message = this.state.messages.find((item) => item.id === messageId);
        const trace = message?.metadata?.providerHttpTrace;
        if (!trace || trace.boundMessageId !== messageId) {
            this.bus.emit('toast', { message: t('chat.copy_body_data_invalid') });
            return;
        }
        const value = bodyType === 'request' ? trace.requestBody : trace.responseBody;
        if (value === undefined || value === null) {
            this.bus.emit('toast', { message: t('chat.copy_body_unavailable') });
            return;
        }

        // 短暂禁用，避免快速重复复制
        button.disabled = true;
        setTimeout(() => {
            button.disabled = false;
        }, 300);

        try {
            await navigator.clipboard.writeText(JSON.stringify(value, null, 2));
            this.bus.emit('toast', {
                message: bodyType === 'request' ? t('chat.copy_request_body_success') : t('chat.copy_response_body_success'),
            });
        } catch (error) {
            console.error('[ChatView] Failed to copy provider body:', error);
            this.bus.emit('toast', { message: t('chat.copy_body_failed') });
        }
    }

    private async syncProjectPicker() {
        const ctx = this.getAtTokenContext();
        if (!ctx) {
            this.hideProjectPicker();
            return;
        }

        this.pickerTokenStart = ctx.start;
        const normalized = ctx.tokenValue.replace(/\\/g, '/');
        const slash = normalized.lastIndexOf('/');
        const targetPath = slash >= 0 ? normalized.slice(0, slash) : '';
        const keyword = slash >= 0 ? normalized.slice(slash + 1) : normalized;

        if (!this.pickerVisible || this.pickerCurrentPath !== targetPath) {
            await this.loadProjectEntries(targetPath);
        } else {
            this.pickerFilterKeyword = keyword;
            this.applyProjectPickerFilter();
            this.renderProjectPicker();
        }
    }

    private getAtTokenContext(): { start: number; tokenValue: string } | null {
        const cursor = this.messageInput.selectionStart ?? this.messageInput.value.length;
        const before = this.messageInput.value.slice(0, cursor);
        const atIdx = before.lastIndexOf('@');
        if (atIdx < 0) {
            return null;
        }
        const prefix = atIdx > 0 ? before[atIdx - 1] : ' ';
        if (!/\s/.test(prefix)) {
            return null;
        }
        const token = before.slice(atIdx + 1);
        if (/\s/.test(token)) {
            return null;
        }
        return { start: atIdx, tokenValue: token };
    }

    private async loadProjectEntries(relativePath: string) {
        this.showProjectPicker();
        this.pickerLoading = true;
        this.pickerErrorMessage = null;
        this.pickerCurrentPath = relativePath;
        this.renderProjectPicker();

        const reqId = ++this.pickerReqSeq;
        try {
            const sessionId = this.state.currentSessionId;
            if (!sessionId) {
                throw new Error('NO_SESSION');
            }

            const runtimeState = this.state.getSessionResourceState(sessionId, 'runtime');
            if (!runtimeState?.data) {
                this.pickerErrorMessage = '当前会话未设置项目目录';
                this.pickerEntries = [];
                this.pickerFilteredEntries = [];
                return;
            }

            const cache = this.getSessionFileTreeCache(sessionId);
            const cacheKey = relativePath || '';
            let entries = cache.get(cacheKey);
            if (!entries) {
                entries = await this.state.gatewayClient?.listSessionFileTree(sessionId, relativePath || undefined);
                if (!entries) {
                    throw new Error('LOAD_FAILED');
                }
                cache.set(cacheKey, entries);
            }
            if (reqId !== this.pickerReqSeq) {
                return;
            }
            this.pickerEntries = entries;
            this.pickerFilterKeyword = this.getCurrentFilterKeyword();
            this.applyProjectPickerFilter();
        } catch (error) {
            if (reqId !== this.pickerReqSeq) {
                return;
            }
            this.pickerEntries = [];
            this.pickerFilteredEntries = [];
            this.pickerErrorMessage = this.getPickerErrorMessage(error);
            console.error('[ChatView] session.file_tree.list failed:', error);
        } finally {
            if (reqId === this.pickerReqSeq) {
                this.pickerLoading = false;
                this.renderProjectPicker();
            }
        }
    }

    private getCurrentFilterKeyword(): string {
        const ctx = this.getAtTokenContext();
        if (!ctx) {
            return '';
        }
        const normalized = ctx.tokenValue.replace(/\\/g, '/');
        const slash = normalized.lastIndexOf('/');
        return slash >= 0 ? normalized.slice(slash + 1) : normalized;
    }

    private applyProjectPickerFilter() {
        const keyword = this.pickerFilterKeyword.toLowerCase();
        this.pickerFilteredEntries = this.pickerEntries.filter((entry) => {
            if (!keyword) {
                return true;
            }
            return entry.name.toLowerCase().includes(keyword) || entry.relativePath.toLowerCase().includes(keyword);
        });
        this.pickerActiveIndex = this.pickerFilteredEntries.length === 0 ? -1 : 0;
    }

    private showProjectPicker() {
        if (!this.projectPickerEl) {
            this.projectPickerEl = document.createElement('div');
            this.projectPickerEl.className = 'project-picker';
            this.projectPickerEl.addEventListener('mousedown', (event) => {
                event.preventDefault();
            });
            this.inputContainer.appendChild(this.projectPickerEl);
        }
        this.pickerVisible = true;
        this.projectPickerEl.classList.add('visible');
    }

    private hideProjectPicker() {
        this.pickerVisible = false;
        this.pickerCurrentPath = '';
        this.pickerEntries = [];
        this.pickerFilteredEntries = [];
        this.pickerFilterKeyword = '';
        this.pickerErrorMessage = null;
        this.pickerActiveIndex = 0;
        if (this.projectPickerEl) {
            this.projectPickerEl.classList.remove('visible');
            this.projectPickerEl.innerHTML = '';
        }
    }

    private renderProjectPicker() {
        if (!this.projectPickerEl || !this.pickerVisible) {
            return;
        }
        const breadcrumb = this.pickerCurrentPath ? this.pickerCurrentPath : '.';
        const filterTip = this.pickerFilterKeyword ? `筛选: ${escapeHtml(this.pickerFilterKeyword)}` : '';
        const items: string[] = [];
        if (this.pickerCurrentPath) {
            items.push(`<button class="project-picker-item up" data-up="1">../</button>`);
        }
        for (let i = 0; i < this.pickerFilteredEntries.length; i += 1) {
            const entry = this.pickerFilteredEntries[i];
            const activeClass = i === this.pickerActiveIndex ? 'active' : '';
            const typeClass = entry.isDir ? 'dir' : 'file';
            items.push(
                `<button class="project-picker-item ${typeClass} ${activeClass}" data-index="${i}">${escapeHtml(entry.name)}</button>`
            );
        }
        const empty = !this.pickerLoading && items.length === 0
            ? `<div class="project-picker-empty">${escapeHtml(this.pickerErrorMessage || '无匹配项')}</div>`
            : '';
        this.projectPickerEl.innerHTML = `
            <div class="project-picker-header">
                <span class="project-picker-path">@${escapeHtml(breadcrumb)}</span>
                <span class="project-picker-filter">${filterTip}</span>
            </div>
            <div class="project-picker-list">
                ${this.pickerLoading ? '<div class="project-picker-empty">加载中...</div>' : items.join('') || empty}
            </div>
        `;

        this.projectPickerEl.querySelectorAll('.project-picker-item[data-index]').forEach((el) => {
            el.addEventListener('click', () => {
                const idx = Number((el as HTMLElement).getAttribute('data-index') ?? '-1');
                if (idx < 0 || idx >= this.pickerFilteredEntries.length) {
                    return;
                }
                void this.selectProjectPickerEntry(this.pickerFilteredEntries[idx]);
            });
        });
        const upButton = this.projectPickerEl.querySelector('.project-picker-item.up');
        if (upButton) {
            upButton.addEventListener('click', () => {
                void this.navigateProjectPickerUp();
            });
        }
    }

    private async navigateProjectPickerUp() {
        if (!this.pickerCurrentPath) {
            return;
        }
        const parts = this.pickerCurrentPath.split('/').filter(Boolean);
        parts.pop();
        await this.loadProjectEntries(parts.join('/'));
    }

    private getSessionFileTreeCache(sessionId: string): Map<string, ProjectDirEntry[]> {
        const existing = this.sessionFileTreeCache.get(sessionId);
        if (existing) {
            return existing;
        }
        const created = new Map<string, ProjectDirEntry[]>();
        this.sessionFileTreeCache.set(sessionId, created);
        return created;
    }

    private invalidateSessionFileTreeCache(sessionId?: string) {
        if (!sessionId) {
            return;
        }
        this.sessionFileTreeCache.delete(sessionId);
    }

    private handleSessionRuntimeUpdated(payload: Record<string, unknown>) {
        const sessionId = typeof payload.sessionId === 'string'
            ? payload.sessionId
            : (typeof payload.session_id === 'string' ? payload.session_id : null);
        if (!sessionId) {
            return;
        }
        const projectDir = this.readProjectDirFromRuntimePayload(payload);
        const previousProjectDir = this.sessionProjectDirState.get(sessionId);
        if (projectDir !== previousProjectDir) {
            this.invalidateSessionFileTreeCache(sessionId);
            if (projectDir !== undefined) {
                this.sessionProjectDirState.set(sessionId, projectDir);
            }
        }
        if (sessionId === this.state.currentSessionId) {
            this.renderProjectMenu();
        }
    }

    private readProjectDirFromRuntimePayload(payload: Record<string, unknown>): string | null | undefined {
        if (typeof payload.projectDir === 'string') {
            return payload.projectDir;
        }
        if (payload.projectDir === null) {
            return null;
        }
        if (typeof payload.project_dir === 'string') {
            return payload.project_dir;
        }
        if (payload.project_dir === null) {
            return null;
        }
        return undefined;
    }

    private getPickerErrorMessage(error: unknown): string {
        if (error instanceof Error) {
            if (error.message.includes('project directory') || error.message.includes('项目目录')) {
                return '当前会话未设置项目目录';
            }
            if (error.message.includes('not found') || error.message.includes('不存在')) {
                return '目录不存在或无权限访问';
            }
        }
        return '无法加载会话文件树';
    }

    private async selectProjectPickerEntry(entry: ProjectDirEntry) {
        if (entry.isDir) {
            await this.loadProjectEntries(entry.relativePath);
            return;
        }
        const cursor = this.messageInput.selectionStart ?? this.messageInput.value.length;
        const before = this.messageInput.value.slice(0, this.pickerTokenStart);
        const after = this.messageInput.value.slice(cursor);
        this.messageInput.value = `${before}@${entry.relativePath} ${after}`;
        const nextCursor = (before + `@${entry.relativePath} `).length;
        this.messageInput.setSelectionRange(nextCursor, nextCursor);
        this.hideProjectPicker();
        this.messageInput.focus();
    }

    private handleProjectPickerKeydown(e: KeyboardEvent): boolean {
        if (!this.pickerVisible || this.pickerComposing) {
            return false;
        }
        if (e.key === 'Escape') {
            e.preventDefault();
            this.hideProjectPicker();
            return true;
        }
        if (e.key === 'ArrowDown') {
            e.preventDefault();
            if (this.pickerFilteredEntries.length > 0) {
                this.pickerActiveIndex = (this.pickerActiveIndex + 1) % this.pickerFilteredEntries.length;
                this.renderProjectPicker();
            }
            return true;
        }
        if (e.key === 'ArrowUp') {
            e.preventDefault();
            if (this.pickerFilteredEntries.length > 0) {
                this.pickerActiveIndex =
                    (this.pickerActiveIndex - 1 + this.pickerFilteredEntries.length) % this.pickerFilteredEntries.length;
                this.renderProjectPicker();
            }
            return true;
        }
        if ((e.key === 'Enter' || e.key === 'Tab') && this.pickerActiveIndex >= 0) {
            e.preventDefault();
            const target = this.pickerFilteredEntries[this.pickerActiveIndex];
            if (target) {
                void this.selectProjectPickerEntry(target);
            }
            return true;
        }
        return false;
    }

    private hideContextMenu() {
        const menu = document.getElementById('chat-context-menu');
        if (menu) menu.remove();
    }

    private handleContextMenu(e: MouseEvent) {
        const messageEl = (e.target as HTMLElement).closest('.message');
        if (!messageEl) return;

        e.preventDefault();
        this.hideContextMenu();

        const index = parseInt(messageEl.getAttribute('data-index') || '-1');
        if (index === -1) return;

        const menu = document.createElement('div');
        menu.id = 'chat-context-menu';
        menu.className = 'context-menu';
        menu.style.position = 'fixed';
        menu.style.left = `${e.clientX}px`;
        menu.style.top = `${e.clientY}px`;
        menu.style.zIndex = '1000';

        const item = document.createElement('div');
        item.className = 'context-menu-item';
        item.innerHTML = `<span class="icon">content_copy</span> ${t('chat.clone_session')}`;
        item.onclick = () => {
             this.bus.emit(Events.SESSION_COPY, { id: this.state.currentSessionId, index });
             this.hideContextMenu();
        };

        menu.appendChild(item);
        document.body.appendChild(menu);
    }

    private sendMessage() {
        const text = this.messageInput.value.trim();
        if (!text) return;
        
        // 发送新消息前，确保重置流式状态，避免内容追加到旧的气泡中
        this.streamingMessageEl = null;
        this.streamingContent = '';
        
        this.currentIntentText = null;
        this.bus.emit('message:send', { text });
        this.messageInput.value = '';
    }

    clear() {
        this.messagesContainer.innerHTML = '';
        this.streamingMessageEl = null;
        this.streamingContent = '';
        this.updateMinimap();
    }

    renderMessages(messages: any[]) {
        // 保存当前的流式状态，避免在渲染历史消息时冲掉正在产生的回复
        const prevStreamingEl = this.streamingMessageEl;
        const isStreaming = !!prevStreamingEl;

        this.streamingMessageEl = null;
        this.streamingContent = '';
        
        // 过滤掉 system 角色的消息，避免工作区显得杂乱
        // 同时保留原始索引，以便后续操作（如克隆会话）能对应上正确的后端索引
        const displayMessages = messages
            .map((m, i) => ({ ...m, originalIndex: i }))
            .filter(m => m.role !== 'system');
        
        if (displayMessages.length === 0 && !isStreaming) {
            this.showWelcome();
            return;
        }
        
        this.messagesContainer.innerHTML = displayMessages.map((m) => this.renderMessage(m, m.originalIndex)).join('');
        
        // 如果之前正在流式输出，将其重新追加到容器末尾
        if (isStreaming) {
            this.streamingMessageEl = prevStreamingEl;
            this.messagesContainer.appendChild(this.streamingMessageEl);
        }

        this.scrollToBottom();
        // 因为可能有大量 DOM 发生改变，使用 setTimeout 等待渲染结束再抓取位置
        setTimeout(() => this.updateMinimap(), 50);
    }

    private showWelcome() {
        this.messagesContainer.innerHTML = `
            <div class="welcome-message">
                <h3>${t('chat.welcome_title')}</h3>
                <p>${t('chat.welcome_desc')}</p>
            </div>
        `;
    }

    private hasExitCodeError(content: any, backendIsError: boolean = false): boolean {
        if (backendIsError) return true;
        if (!content) return false;
        
        let c = content;
        if (typeof content === 'string') {
            try {
                c = JSON.parse(content);
            } catch(e) {}
        }
        
        if (typeof c === 'object' && c !== null && c.exit_code !== undefined) {
            return c.exit_code !== 0;
        }
        if (typeof content === 'string') {
            if (content.includes('"exit_code":')) {
                return !content.includes('"exit_code": 0') && !content.includes('"exit_code":0');
            } else if (content.includes('exit_code:')) {
                return !content.match(/exit_code:\s*0\b/);
            }
        }
        return false;
    }

    private addMessage(message: any) {
        if (message.role === 'system') return;
        // 使用 state 中的消息总数减 1 作为原始索引
        const index = this.state.messages.length - 1;
        const html = this.renderMessage(message, index);
        this.messagesContainer.insertAdjacentHTML('beforeend', html);
        this.scrollToBottom();
        this.updateMinimap();
    }

    private renderMessage(message: any, index: number): string {
        const isAssistant = message.role === 'assistant';
        const content = message.content;
        const voiceTranscriptState = message.metadata?.voiceTranscriptState as 'pending' | 'final' | undefined;
        let contentHtml = '';
        if (!content) {
            contentHtml = '<span class="empty-content">...</span>';
        } else if (Array.isArray(content)) {
            // 处理 Phase 4 的内容块数组
            contentHtml = content.map((block: any) => {
                const type = block.type;
                if (type === 'text') {
                    const text = typeof block.text === 'string' ? block.text : (block.content || '');
                    return renderMarkdown(text);
                } else if (type === 'thinking') {
                    return `<div class="thinking-block">
                        <div class="thinking-header">${t('chat.thinking')}</div>
                        <div class="thinking-content">${renderMarkdown(block.thinking || '')}</div>
                    </div>`;
                } else if (type === 'tool_use' || type === 'tool_call') {
                    const name = block.name || block.toolName;
                    const input = block.input || block.args;
                    const toolUseId = block.id || block.toolUseId;
                    return `<div class="tool-use-card collapsible collapsed" data-tool-use-id="${toolUseId}">
                        <div class="tool-name">🛠️ ${name} <span class="collapse-icon">⌄</span></div>
                        <pre class="tool-args">${JSON.stringify(input, null, 2)}</pre>
                        <div class="tool-log-streamer hidden"></div>
                    </div>`;
                } else if (type === 'tool_result') {
                    const originalContent = block.content || block.result || block.output || '';
                    let displayContent = '';
                    let isErrorCode = this.hasExitCodeError(originalContent, block.isError);
                    
                    try {
                        const parsed = typeof originalContent === 'string' ? JSON.parse(originalContent) : originalContent;
                        
                        if (parsed && typeof parsed === 'object') {
                            if (parsed.output_summary) {
                                // Subagent summary rendering
                                displayContent = renderMarkdown(parsed.output_summary);
                                
                                if (parsed.logs && Array.isArray(parsed.logs) && parsed.logs.length > 0) {
                                    displayContent += `
                                        <details class="subagent-logs-detail" style="margin-top: 12px; border: 1px solid var(--border-color); border-radius: 6px; overflow: hidden;">
                                            <summary style="padding: 8px 12px; background: var(--bg-secondary); cursor: pointer; font-size: 0.85em; font-weight: 500; display: flex; align-items: center; gap: 8px;">
                                                <span class="icon">📜</span> ${t('chat.subagent_logs')}
                                            </summary>
                                            <div style="padding: 0; background: #000; color: #fff; font-family: var(--font-code); font-size: 0.8em; max-height: 300px; overflow-y: auto;">
                                                <pre style="margin: 0; padding: 12px; white-space: pre-wrap; line-height: 1.4;">${escapeHtml(parsed.logs.join(''))}</pre>
                                            </div>
                                        </details>`;
                                }

                                if (parsed.workspace_files && Array.isArray(parsed.workspace_files) && parsed.workspace_files.length > 0) {
                                    displayContent += `<div class="tool-result-files" style="margin-top: 10px; font-size: 0.9em; color: var(--text-secondary);">
                                        📁 ${t('chat.files_created', parsed.workspace_files.length)}: ${parsed.workspace_files.join(', ')}
                                    </div>`;
                                }
                            } else {
                                // Other JSON tools
                                displayContent = `<pre class="json-result"><code>${escapeHtml(JSON.stringify(parsed, null, 2))}</code></pre>`;
                            }
                        } else {
                            displayContent = escapeHtml(String(originalContent));
                        }
                    } catch (e) {
                        displayContent = escapeHtml(String(originalContent));
                    }

                    return `<div class="tool-result-card collapsible collapsed ${isErrorCode ? 'error' : ''}">
                        <div class="tool-result-header">🔍 ${t('chat.tool_result')} <span class="collapse-icon">⌄</span></div>
                        <div class="tool-result-content">${displayContent}</div>
                    </div>`;
                }
                return '';
            }).join('');
        } else {
            // 兼容旧的字符串格式
            const text = typeof content === 'string' ? content : JSON.stringify(content);
            contentHtml = isAssistant ? renderMarkdown(text) : escapeHtml(text);
        }
        
        const isTool = Array.isArray(message.content) && message.content.some((b: any) => b.type === 'tool_result');
        let hasToolError = false;
        if (isTool) {
             hasToolError = message.content.some((b: any) => {
                  if (b.type !== 'tool_result') return false;
                  return this.hasExitCodeError(b.content || b.result || b.output, b.isError);
             });
        }
        const roleClass = isTool ? `tool ${hasToolError ? 'tool-error-msg' : ''}` : message.role;
        
        const intentText = message.metadata?.intentText || (isAssistant && this.currentIntentText ? this.currentIntentText : '');
        const intentHtml = intentText ? `<div class="message-intent">${escapeHtml(intentText)}</div>` : '';
        const voiceBadgeHtml = voiceTranscriptState
            ? `<div class="message-intent">${voiceTranscriptState === 'pending' ? t('voice.recognizing') : t('voice.title')}</div>`
            : '';
        const trace = message.metadata?.providerHttpTrace;
        const traceBound = trace?.boundMessageId === message.id;
        const hasRequestBody = !!trace && traceBound && trace.requestBody !== undefined && trace.requestBody !== null;
        const hasResponseBody = !!trace && traceBound && trace.responseBody !== undefined && trace.responseBody !== null;
        const traceActionsHtml = isAssistant
            ? `<div class="message-trace-actions">
                <button
                    class="message-trace-copy-btn"
                    data-body-type="request"
                    data-message-id="${escapeHtml(message.id || '')}"
                    ${hasRequestBody ? '' : 'disabled'}
                >${t('chat.copy_request_body')}</button>
                <button
                    class="message-trace-copy-btn"
                    data-body-type="response"
                    data-message-id="${escapeHtml(message.id || '')}"
                    ${hasResponseBody ? '' : 'disabled'}
                >${t('chat.copy_response_body')}</button>
            </div>`
            : '';

        return `
            <div class="message ${roleClass} ${voiceTranscriptState ? `voice-transcript ${voiceTranscriptState}` : ''}" data-index="${index}">
                <div class="message-bubble">
                    ${voiceBadgeHtml}
                    ${intentHtml}
                    <div class="markdown-body">${contentHtml}</div>
                    ${traceActionsHtml}
                </div>
                <div class="message-time">${formatTime(message.timestamp || message.createdAt)}</div>
            </div>
        `;
    }

    private appendToken(token: string) {
        if (!this.streamingMessageEl) {
            this.streamingMessageEl = this.createStreamingMessage();
            this.messagesContainer.appendChild(this.streamingMessageEl);
        }
        
        const markdownBody = this.streamingMessageEl.querySelector('.markdown-body');
        if (!markdownBody) return;

        // 查找或创建当前的文本块容器
        // 如果最后一个子元素不是文本块（可能是工具卡片），则新建一个
        let textBlock = markdownBody.lastElementChild as HTMLElement;
        if (!textBlock || !textBlock.classList.contains('streaming-text-block')) {
            textBlock = document.createElement('div');
            textBlock.className = 'streaming-text-block';
            (textBlock as any)._rawContent = ''; // 用于增量 Markdown 渲染
            markdownBody.appendChild(textBlock);
        }

        (textBlock as any)._rawContent += token;
        textBlock.innerHTML = renderMarkdown((textBlock as any)._rawContent);
        
        this.scrollToBottom();
    }

    private createStreamingMessage(): HTMLElement {
        const div = document.createElement('div');
        div.className = 'message assistant streaming';
        
        const intentHtml = this.currentIntentText 
            ? `<div class="message-intent">${escapeHtml(this.currentIntentText)}</div>` 
            : '';

        div.innerHTML = `
            <div class="message-bubble">
                ${intentHtml}
                <div class="markdown-body"></div>
            </div>
        `;
        return div;
    }

    private handleIntent(payload: any) {
        console.log('[ChatView] Handling intent:', payload);
        let text = '';
        switch(payload.intent) {
            case 'chat': text = t('chat.intent_chat'); break;
            case 'resolve': text = t('chat.intent_resolve'); break;
            case 'continue_workflow': text = t('chat.intent_continue_workflow'); break;
            case 'address_agent': 
                const agent = this.state.agentsList.find(a => a.id === payload.agentId);
                const name = agent ? agent.name : (payload.agentId || 'Unknown');
                text = t('chat.intent_address_agent').replace('{0}', name);
                break;
        }
        
        if (text) {
            this.currentIntentText = text;
            // 如果已经正在流式输出，动态更新当前的 header
            if (this.streamingMessageEl) {
                const intentEl = this.streamingMessageEl.querySelector('.message-intent');
                if (intentEl) {
                    intentEl.textContent = text;
                } else {
                    const bubble = this.streamingMessageEl.querySelector('.message-bubble');
                    bubble?.insertAdjacentHTML('afterbegin', `<div class="message-intent">${escapeHtml(text)}</div>`);
                }
            }
        }
    }

    private handleToolLog(event: any) {
        const { toolUseId, log, stream, sessionId } = event;
        // 隔离非当前会话的工具日志
        if (sessionId && sessionId !== this.state.currentSessionId) return;

        // 查找对应的 tool-use-card
        const card = this.messagesContainer.querySelector(`.tool-use-card[data-tool-use-id="${toolUseId}"]`);
        if (!card) return;

        const streamer = card.querySelector('.tool-log-streamer');
        if (!streamer) return;

        streamer.classList.remove('hidden');
        const line = document.createElement('div');
        line.className = `log-line ${stream || 'stdout'}`;
        line.textContent = log;
        streamer.appendChild(line);

        // 自动滚动日志区域
        streamer.scrollTop = streamer.scrollHeight;
        
        // 同时滚动整个消息区域
        this.scrollToBottom();
    }

    private handleChatError(payload: any) {
        let text = '';
        if (payload.type === 'iteration_limit') {
            text = t('chat.error_iteration_limit').replace('{0}', String(payload.iteration || 10));
        } else {
            text = payload.message || t('common.unknown_error');
        }

        const html = `
            <div class="message system error">
                <div class="message-bubble">
                    <div class="error-header">⚠️ ${t('common.error')}</div>
                    <div class="error-content">${escapeHtml(text)}</div>
                </div>
            </div>
        `;
        this.messagesContainer.insertAdjacentHTML('beforeend', html);
        this.scrollToBottom();
    }

    private handleToolStart(event: any) {
        const { toolName, args, toolUseId } = event;
        if (!this.streamingMessageEl) {
            this.streamingMessageEl = this.createStreamingMessage();
            this.messagesContainer.appendChild(this.streamingMessageEl);
        }

        const markdownBody = this.streamingMessageEl.querySelector('.markdown-body');
        if (markdownBody) {
            const html = `
                <div class="tool-use-card collapsible" data-tool-use-id="${toolUseId}">
                    <div class="tool-name">🛠️ ${toolName} <span class="collapse-icon">⌄</span></div>
                    <pre class="tool-args">${JSON.stringify(args || {}, null, 2)}</pre>
                    <div class="tool-log-streamer hidden"></div>
                </div>
            `;
            markdownBody.insertAdjacentHTML('beforeend', html);
            this.scrollToBottom();
        }
    }

    private handleToolResult(event: any) {
        const { toolUseId, result, isError } = event;
        
        // 我们需要找到对应的 tool-use-card 并在其后插入结果，或者直接在 streaming message 中寻找
        if (!this.streamingMessageEl) return;

        const markdownBody = this.streamingMessageEl.querySelector('.markdown-body');
        if (markdownBody) {
            const originalContent = result || '';
            let displayContent = '';
            let isErrorCode = this.hasExitCodeError(originalContent, isError);
            try {
                const parsed = typeof originalContent === 'string' ? JSON.parse(originalContent) : originalContent;
                if (parsed && typeof parsed === 'object' && parsed.output_summary) {
                    displayContent = renderMarkdown(parsed.output_summary);
                } else {
                    displayContent = `<pre class="json-result"><code>${escapeHtml(JSON.stringify(parsed, null, 2))}</code></pre>`;
                }
            } catch (e) {
                displayContent = escapeHtml(String(originalContent));
            }

            const html = `
                <div class="tool-result-card collapsible ${isErrorCode ? 'error' : ''}" data-rel-id="${toolUseId}">
                    <div class="tool-result-header">🔍 ${t('chat.tool_result')} <span class="collapse-icon">⌄</span></div>
                    <div class="tool-result-content">${displayContent}</div>
                </div>
            `;
            markdownBody.insertAdjacentHTML('beforeend', html);
            this.scrollToBottom();
            this.updateMinimap();

            // 5s 后自动折叠工具调用和结果
            setTimeout(() => {
                const toolCard = markdownBody.querySelector(`.tool-use-card[data-tool-use-id="${toolUseId}"]`);
                const resultCard = markdownBody.querySelector(`.tool-result-card[data-rel-id="${toolUseId}"]`);
                if (toolCard) toolCard.classList.add('collapsed');
                if (resultCard) resultCard.classList.add('collapsed');
            }, 5000);
        }
    }

    private handleSystemLog(event: any) {
        const { log } = event;
        // 过滤常见的 Agent 迭代反馈，避免干扰用户视线
        if (log.includes('Agent iteration')) return;

        const isError = log.toLowerCase().includes('failed') || log.toLowerCase().includes('error');
        const roleClass = isError ? 'system log error' : 'system log';

        const html = `
            <div class="message ${roleClass}">
                <div class="message-bubble">
                    <div class="markdown-body">${escapeHtml(log)}</div>
                </div>
            </div>
        `;
        
        this.messagesContainer.insertAdjacentHTML('beforeend', html);
        this.scrollToBottom();
    }

    /**
     * 处理 AI 迭代进度，更新状态栏
     */
    private handleIteration(event: any) {
        const { iteration } = event;
        // 构建友好的状态文本
        const statusText = `Agent Running (${iteration}/30)`;
        // 发布全局状态更新，TitleBar 会捕获并更新顶部的红绿灯/文字
        this.bus.emit(Events.GATEWAY_STATUS, { status: 'running', text: statusText });
        
        // 可选：如果 5s 内没有任何新进度，Titlebar 会保持这个状态直到任务完成（chat:complete 回调会重置状态）
    }

    private scrollToBottom() {
        this.messagesContainer.scrollTop = this.messagesContainer.scrollHeight;
    }

    private updateMinimap() {
        const minimap = document.getElementById('chat-minimap');
        const minimapMarkers = document.getElementById('chat-minimap-markers');
        if (!minimap || !minimapMarkers) return;

        const messagesContainer = this.messagesContainer;
        const scrollHeight = messagesContainer.scrollHeight;
        // 如果内容不需要滚动，可以隐藏或仍保留。按比例的话，不超出一满屏会显得比较稀疏。
        // if (scrollHeight <= clientHeight && messagesContainer.children.length < 2) {
        //    minimap.style.display = 'none';
        //    return;
        // } else {
        //    minimap.style.display = 'block';
        // }

        let hasAnyError = false;
        minimapMarkers.innerHTML = '';

        const messages = messagesContainer.querySelectorAll('.message');
        if (messages.length === 0) return;

        messages.forEach((msg) => {
            const htmlMsg = msg as HTMLElement;
            const isUser = htmlMsg.classList.contains('user');
            const isToolError = htmlMsg.classList.contains('tool-error-msg');
            
            if (isToolError) hasAnyError = true;

            if (isUser || isToolError) {
                // 计算相对位置 (百分比)
                // 以消息元素的中间位置为准，由于 scrollHeight 包含所有的 content，
                // 计算比例能大致映射到 minimap 的竖线上。
                const offsetTop = htmlMsg.offsetTop;
                // 添加一个小偏移，使得中心点更准
                const topPercent = ((offsetTop + (htmlMsg.clientHeight / 2)) / scrollHeight) * 100;
                
                const marker = document.createElement('div');
                marker.className = `chat-minimap-marker ${isUser ? 'user' : 'tool-error'}`;
                // 限制最高值为 98% 避免掉出底端
                marker.style.top = `${Math.min(98, Math.max(2, topPercent))}%`;
                marker.title = isUser ? 'User Message' : 'Tool Error (exit_code != 0)';
                
                // 用户点击跳转
                marker.addEventListener('click', () => {
                    htmlMsg.scrollIntoView({ behavior: 'smooth', block: 'center' });
                    // 如果有高亮需求可以在这里补充
                    htmlMsg.style.transition = 'background-color 0.5s ease';
                    const origBg = htmlMsg.style.backgroundColor;
                    // Flash effect
                    htmlMsg.style.backgroundColor = 'rgba(99, 102, 241, 0.15)';
                    setTimeout(() => { htmlMsg.style.backgroundColor = origBg; }, 1000);
                });
                
                minimapMarkers.appendChild(marker);
            }
        });

        // 根据是否有 tool error ，让外面的主线变成红色
        if (hasAnyError) {
            minimap.classList.add('has-error');
        } else {
            minimap.classList.remove('has-error');
        }
    }

    private updateHeaderTitle() {
        const titleEl = document.getElementById('chat-header-title');
        if (titleEl) {
            const session = this.state.sessions.find(s => s.id === this.state.currentSessionId);
            const agent = this.state.agentsList.find(a => a.id === this.state.currentAgentId);
            titleEl.textContent = session?.title || agent?.name || 'Chat';
        }
    }
}
