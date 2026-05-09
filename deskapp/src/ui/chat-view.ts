import { invoke } from '@tauri-apps/api/core';
import { t } from '../i18n/index';
import { AppState } from '../core/state';
import { EventBus, Events } from '../core/event-bus';
import { renderMarkdown } from '../markdown';
import { escapeHtml, formatTime } from '../utils/html';
import { playCompletionSound } from '../utils/sound';
import { OrchestrationView } from './orchestration-view';
import type { SessionFileTreeEntryView, SessionRuntimeSnapshot, TokenUsageView, ResourceState } from '../core/types';

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
    private projectMenuReqSeq = 0;
    
    private streamingMessageEl: HTMLElement | null = null;
    private streamingContent = ''; // 仅作向后兼容和备份
    private currentIntentText: string | null = null;
    private layoutObserver: ResizeObserver | null = null;

    // 流式状态机：以会话为粒度的流式输出追踪
    private streamingSessions = new Set<string>();
    private stoppingSessions = new Set<string>();

    // 本次消息的 token 使用量（用于在聊天窗口额外展示）
    private thisTurnTokenUsage: { inputTokens: number; outputTokens: number; totalTokens: number } | null = null;

    // 工具结果缓存：处理到达顺序错乱（结果工具调用到达）
    // 结构：sessionId -> Map<toolUseId, ToolResultEvent>
    private pendingToolResults = new Map<string, Map<string, any>>();
    
    // 上次会话 ID（用于会话切换时的缓存清理）
    private lastSessionId: string | null = null;

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
        new OrchestrationView(this.bus, this.messagesContainer, () => this.state.currentSessionId);
        this.ensureProjectMenu();
        this.bindEvents();
        
        // 检测 :has() 支持，不支持时添加降级类
        if (typeof CSS !== 'undefined' && !CSS.supports('selector(:has(.foo))')) {
            document.documentElement.classList.add('no-has-support');
        }
        
        // 监听消息容器大小变化，更新右侧的 Minimap 导航条
        if (window.ResizeObserver) {
            this.layoutObserver = new ResizeObserver(() => {
                this.updateMinimap();
            });
            this.layoutObserver.observe(this.messagesContainer);
        }
        
        this.bus.on(Events.SESSION_SELECTED, () => {
            this.clearRuntimeStatusText();
            this.updateHeaderTitle();
            this.hideProjectPicker();
            this.hideProjectMenu();
            void this.refreshProjectMenuState();
            this.updateSendButton();
        });

        // 监听会话标题更新事件 (Plan 2)
        this.bus.on(Events.SESSION_SUMMARY_UPDATED, (payload: { sessionId: string; title: string }) => {
            console.log('[ChatView] Session summary updated, updating header title:', payload.sessionId, payload.title);
            // 如果当前会话标题被更新，同步更新 header
            if (this.state.currentSessionId === payload.sessionId) {
                this.updateHeaderTitle();
            }
        });

        const gatewayClient = this.state.gatewayClient;
        if (gatewayClient && typeof gatewayClient.onSessionRuntimeUpdated === 'function') {
            gatewayClient.onSessionRuntimeUpdated((payload) => {
                this.handleSessionRuntimeUpdated(payload);
            });
        }
        
        this.bus.on(Events.SESSION_CHANGED, (payload: any) => {
             console.log('[ChatView] Session changed:', payload.previousSessionId, '->', payload.sessionId);
             if (payload.previousSessionId) {
                 this.invalidateSessionFileTreeCache(payload.previousSessionId);
             }
             // 清理离开前会话的缓存结果，避免误清理新会话缓存
             const cacheKey = payload.fromSessionId || this.lastSessionId;
             if (cacheKey) {
                 this.clearPendingResultsForSession(cacheKey);
             }
             this.lastSessionId = payload.toSessionId || this.state.currentSessionId;

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
                 // 兜底：首条消息建会话时 sendMessage 里 sessionId 为 null，此处补充追踪
                 if (!this.streamingSessions.has(payload.sessionId)) {
                     this.streamingSessions.add(payload.sessionId);
                     this.updateSendButton();
                     // 首次收到 token 时，记录本次消息开始前的 session 累计 token
                     const sessionUsage = this.state.getSessionResourceState(payload.sessionId, 'tokenUsage');
                     const prevData = (sessionUsage as ResourceState<TokenUsageView> | undefined)?.data;
                     if (prevData) {
                         this.thisTurnTokenUsage = {
                             inputTokens: 0,
                             outputTokens: 0,
                             totalTokens: 0,
                         };
                         // 存储快照用于后续计算增量
                         (this as any)._thisTurnSnapshot = {
                             inputTokens: prevData.inputTokens ?? 0,
                             outputTokens: prevData.outputTokens ?? 0,
                         };
                     }
                 }
                 this.appendToken(payload.token);
             }
        });

        this.bus.on('chat:complete', (payload: any) => {
             // 无论当前显示哪个会话，都清理该会话的流式状态，避免切换回来后按钮卡住
             this.streamingSessions.delete(payload.sessionId);
             this.stoppingSessions.delete(payload.sessionId);
             if (payload.sessionId === this.state.currentSessionId) {
                 console.log('[ChatView] Chat complete, resetting streaming state');
                 // 播放完成提示音
                 playCompletionSound();
                 this.streamingMessageEl = null;
                 this.streamingContent = '';
                 this.updateSendButton();
                 this.invalidateSessionFileTreeCache(payload.sessionId);
                 this.clearRuntimeStatusText();
                 // 重置本次请求的 token 使用量
                 this.thisTurnTokenUsage = null;
                // 清理工具结果缓存
                 this.pendingToolResults.delete(payload.sessionId);
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
            // 无论当前显示哪个会话，都清理该会话的流式状态
            this.streamingSessions.delete(payload.sessionId);
            this.stoppingSessions.delete(payload.sessionId);
            if (payload.sessionId === this.state.currentSessionId) {
                this.updateSendButton();
                this.clearRuntimeStatusText();
                this.handleChatError(payload);
            }
        });

        // 停止响应：退出 STOPPING 状态
        this.bus.on('chat:stop-response', (payload: any) => {
            // 无论当前显示哪个会话，都清理 stopping 标记
            this.stoppingSessions.delete(payload.sessionId);
            if (payload.sessionId === this.state.currentSessionId) {
                console.log('[ChatView] Chat stop response received');
                this.updateSendButton();
            }
        });

        // 连接断开容错：清空所有 streaming/stopping 状态
        if (gatewayClient && typeof gatewayClient.onConnectionChange === 'function') {
            gatewayClient.onConnectionChange((status) => {
                if (status === 'disconnected') {
                    console.log('[ChatView] Gateway disconnected, clearing streaming state');
                    this.streamingSessions.clear();
                    this.stoppingSessions.clear();
                    this.updateSendButton();
                }
            });
        }

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
        // 统一按钮点击处理：发送消息或停止生成
        this.sendBtn.addEventListener('click', () => {
            const sid = this.state.currentSessionId;
            const isStreaming = sid ? this.streamingSessions.has(sid) : false;
            if (isStreaming) {
                this.handleStopClick();
            } else {
                this.sendMessage();
            }
        });
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
            
            // 允许结果卡内部的 <details>/<summary>、链接、按钮等交互
            // 只阻止对折叠按钮的点击冒泡
            const isResultCardHeader = target.closest('.tool-result-header');
            const isResultCardContentInteractive = target.closest('a, button, details, summary');
            
            if (isResultCardHeader && !isResultCardContentInteractive) {
                // 点击结果卡头部，阻止冒泡到父卡片
                e.stopPropagation();
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

        const menuAnchor = this.inputRow !== this.inputContainer ? this.inputRow : this.inputContainer.firstChild;
        if (menuAnchor) {
            this.inputContainer.insertBefore(this.projectMenuBtn, menuAnchor);
        } else {
            this.inputContainer.appendChild(this.projectMenuBtn);
        }
        this.inputContainer.appendChild(this.projectMenuEl);
        this.renderProjectMenu();
    }

    private async refreshProjectMenuState() {
        const sessionId = this.state.currentSessionId;
        if (!sessionId) {
            this.renderProjectMenu();
            return;
        }

        const reqId = ++this.projectMenuReqSeq;
        if (!this.state.gatewayClient) {
            const runtimeState = this.state.getSessionResourceState(sessionId, 'runtime') as { data?: SessionRuntimeSnapshot } | undefined;
            if (runtimeState?.data) {
                this.applySessionProjectDir(sessionId, runtimeState.data.projectDir ?? null);
            }
            this.renderProjectMenu();
            return;
        }

        try {
            const runtime = await this.state.gatewayClient.getSessionRuntime(sessionId);
            if (reqId !== this.projectMenuReqSeq) {
                return;
            }
            this.state.updateSessionResourceState(sessionId, 'runtime', this.state.setLoadedResource(runtime));
            this.applySessionProjectDir(sessionId, runtime.projectDir ?? null);
        } catch (error) {
            console.warn('[ChatView] Failed to refresh session runtime for project menu:', error);
        }

        if (reqId === this.projectMenuReqSeq) {
            this.renderProjectMenu();
        }
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

        // 滚动选中项到可视区域
        if (this.pickerActiveIndex >= 0) {
            const activeEl = this.projectPickerEl.querySelector(`.project-picker-item[data-index="${this.pickerActiveIndex}"]`);
            if (activeEl) {
                const item = activeEl as HTMLElement & { scrollIntoView?: (options?: ScrollIntoViewOptions) => void };
                item.scrollIntoView?.({ block: 'nearest' });
            }
        }

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
        console.info('[ChatView] session.runtime.updated received:', {
            sessionId,
            currentSessionId: this.state.currentSessionId,
            projectDir,
        });
        const changed = this.updateProjectDirRuntimeState(sessionId, projectDir, payload);
        if (sessionId === this.state.currentSessionId) {
            if (changed && this.pickerVisible) {
                void this.loadProjectEntries(this.pickerCurrentPath);
            }
            this.renderProjectMenu();
            console.info('[ChatView] Project menu rendered from runtime update:', {
                sessionId,
                projectDir,
                buttonTextAfterRender: this.projectMenuBtn?.textContent,
            });
        }
    }

    private updateProjectDirRuntimeState(
        sessionId: string,
        projectDir: string | null | undefined,
        payload?: Record<string, unknown>
    ): boolean {
        const changed = this.applySessionProjectDir(sessionId, projectDir);
        const runtimeState = this.state.getSessionResourceState(sessionId, 'runtime') as { data?: SessionRuntimeSnapshot } | undefined;
        const runtime = {
            ...(runtimeState?.data ?? {}),
            ...((payload ?? {}) as Partial<SessionRuntimeSnapshot>),
            sessionId,
            projectDir: projectDir ?? runtimeState?.data?.projectDir ?? null,
        } as SessionRuntimeSnapshot;
        this.state.updateSessionResourceState(sessionId, 'runtime', this.state.setLoadedResource(runtime));
        return changed;
    }

    private applySessionProjectDir(sessionId: string, projectDir: string | null | undefined): boolean {
        if (projectDir === undefined) {
            return false;
        }
        const previousProjectDir = this.sessionProjectDirState.get(sessionId);
        const changed = projectDir !== previousProjectDir;
        if (projectDir !== previousProjectDir) {
            this.invalidateSessionFileTreeCache(sessionId);
        }
        this.sessionProjectDirState.set(sessionId, projectDir);
        return changed;
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

    private selectProjectPickerFolder(entry: ProjectDirEntry) {
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
                if (e.shiftKey && target.isDir) {
                    this.selectProjectPickerFolder(target);
                } else {
                    void this.selectProjectPickerEntry(target);
                }
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

        // 如果当前会话正在流式输出，不允许发送新消息（Enter 键路径也经过此处）
        const sessionId = this.state.currentSessionId;
        if (sessionId && this.streamingSessions.has(sessionId)) return;

        // 发送新消息前，确保重置流式状态，避免内容追加到旧的气泡中
        this.streamingMessageEl = null;
        this.streamingContent = '';

        // 标记当前会话为 streaming 状态
        if (sessionId) {
            this.streamingSessions.add(sessionId);
            this.updateSendButton();
        }
        
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

    private resolveToolUseId(block: any): string {
        return block.id || block.toolUseId || block.tool_use_id || '';
    }

    private buildToolHtml(message: any): string {
        if (!Array.isArray(message.content)) return '';
        
        // Phase 1: Collect tool_use and tool_result, build mappings
        const toolUseMap = new Map<string, any>();
        const resultMap = new Map<string, any>();
        const toolUseOrder: string[] = [];
        
        for (const block of message.content) {
            if (block.type === 'tool_use' || block.type === 'tool_call') {
                const id = this.resolveToolUseId(block);
                if (id) {
                    toolUseMap.set(id, block);
                    if (!toolUseOrder.includes(id)) {
                        toolUseOrder.push(id);
                    }
                }
            } else if (block.type === 'tool_result') {
                const id = this.resolveToolUseId(block);
                if (id) {
                    resultMap.set(id, block);
                }
            }
        }
        
        // Phase 2: Output tool_use in order with corresponding tool_result
        let htmlParts: string[] = [];
        const processedResultIds = new Set<string>();
        
        for (const toolUseId of toolUseOrder) {
            const toolUseBlock = toolUseMap.get(toolUseId);
            const resultBlock = resultMap.get(toolUseId);
            
            const name = toolUseBlock?.name || toolUseBlock?.toolName || '';
            const args = toolUseBlock?.args || toolUseBlock?.input || {};
            
            let resultHtml = '';
            if (resultBlock) {
                processedResultIds.add(toolUseId);
                resultHtml = this.buildToolResultInline(resultBlock);
            }
            
            // Nested structure: tool_use contains tool_result
            const containerClass = resultHtml ? 'tool-result-container has-result' : 'tool-result-container';
            const html = `
                <div class="tool-use-card collapsible collapsed" data-tool-use-id="${toolUseId}">
                    <div class="tool-name">🛠️ ${name} <span class="collapse-icon">⌄</span></div>
                    <pre class="tool-args">${JSON.stringify(args || {}, null, 2)}</pre>
                    <div class="tool-log-streamer hidden"></div>
                    <div class="${containerClass}" data-rel-id="${toolUseId}">
                        ${resultHtml}
                    </div>
                </div>
            `;
            htmlParts.push(html);
        }
        
        // Phase 3: Output orphaned tool_result (no matching tool_use)
        for (const block of message.content) {
            if (block.type === 'tool_result') {
                const id = this.resolveToolUseId(block);
                if (id && !processedResultIds.has(id)) {
                    const originalContent = block.content || block.result || block.output || '';
                    let displayContent = '';
                    let isErrorCode = this.hasExitCodeError(originalContent, block.isError);
                    
                    try {
                        const parsed = typeof originalContent === 'string' ? JSON.parse(originalContent) : originalContent;
                        if (parsed && typeof parsed === 'object') {
                            if (parsed.output_summary) {
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
                                displayContent = `<pre class="json-result"><code>${escapeHtml(JSON.stringify(parsed, null, 2))}</code></pre>`;
                            }
                        } else {
                            displayContent = escapeHtml(String(originalContent));
                        }
                    } catch (e) {
                        displayContent = escapeHtml(String(originalContent));
                    }

                    const html = `
                        <div class="tool-result-card collapsible ${isErrorCode ? 'error' : ''}">
                            <div class="tool-result-header">🔍 ${t('chat.tool_result')} <span class="collapse-icon">⌄</span></div>
                            <div class="tool-result-content">${displayContent}</div>
                        </div>
                    `;
                    htmlParts.push(html);
                }
            }
        }
        
        return htmlParts.join('');
    }

    private buildToolResultInline(block: any): string {
        const originalContent = block.content || block.result || block.output || '';
        let displayContent = '';
        let isErrorCode = this.hasExitCodeError(originalContent, block.isError);
        
        try {
            const parsed = typeof originalContent === 'string' ? JSON.parse(originalContent) : originalContent;
            if (parsed && typeof parsed === 'object') {
                if (parsed.output_summary) {
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
                    displayContent = `<pre class="json-result"><code>${escapeHtml(JSON.stringify(parsed, null, 2))}</code></pre>`;
                }
            } else {
                displayContent = escapeHtml(String(originalContent));
            }
        } catch (e) {
            displayContent = escapeHtml(String(originalContent));
        }

        return `<div class="tool-result-card collapsible ${isErrorCode ? 'error' : ''}">
            <div class="tool-result-header">🔍 ${t('chat.tool_result')} <span class="collapse-icon">⌄</span></div>
            <div class="tool-result-content">${displayContent}</div>
        </div>`;
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
            // Check if content contains tool blocks that should use nested structure
            const hasToolBlocks = content.some((block: any) => 
                block.type === 'tool_use' || block.type === 'tool_call' || block.type === 'tool_result'
            );
            
            if (hasToolBlocks) {
                contentHtml = this.buildToolHtml(message);
            } else {
                // For non-tool content, keep the original mapping approach
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
                    }
                    return '';
                }).join('');
            }
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
        const tokenUsage = message.tokenUsage;
        // 获取 session 级别的 token 累计值
        const sessionTokenUsage = this.state.getSessionResourceState(this.state.currentSessionId || '', 'tokenUsage');
        const sessionUsageData = (sessionTokenUsage as ResourceState<TokenUsageView> | undefined)?.data;
        const sessionInputTokens = sessionUsageData?.inputTokens ?? 0;
        const sessionOutputTokens = sessionUsageData?.outputTokens ?? 0;
        const sessionTotalTokens = sessionInputTokens + sessionOutputTokens;
        
        // 计算本次请求的 token 增量
        const thisTurnSnapshot = (this as any)._thisTurnSnapshot as { inputTokens?: number; outputTokens?: number } | undefined;
        const thisTurnInputDelta = thisTurnSnapshot?.inputTokens != null ? sessionInputTokens - thisTurnSnapshot.inputTokens : 0;
        const thisTurnOutputDelta = thisTurnSnapshot?.outputTokens != null ? sessionOutputTokens - thisTurnSnapshot.outputTokens : 0;
        const thisTurnTotalDelta = thisTurnInputDelta + thisTurnOutputDelta;

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
        const tokenUsageHtml = isAssistant && tokenUsage
            ? `<div class="message-token-usage">
                Tokens: prompt ${this.formatTokenCountInK(tokenUsage.inputTokens)} · completion ${this.formatTokenCountInK(tokenUsage.outputTokens)} · total ${this.formatTokenCountInK(tokenUsage.totalTokens)}
                ${thisTurnTotalDelta > 0 ? `<span class="this-turn-token-usage"> (this turn: prompt ${this.formatTokenCountInK(thisTurnInputDelta)} · completion ${this.formatTokenCountInK(thisTurnOutputDelta)} · total ${this.formatTokenCountInK(thisTurnTotalDelta)})</span>` : ''}
                ${sessionUsageData ? `<span class="session-token-usage"> (session: prompt ${this.formatTokenCountInK(sessionInputTokens)} · completion ${this.formatTokenCountInK(sessionOutputTokens)} · total ${this.formatTokenCountInK(sessionTotalTokens)})</span>` : ''}
            </div>`
            : '';

        return `
            <div class="message ${roleClass} ${voiceTranscriptState ? `voice-transcript ${voiceTranscriptState}` : ''}" data-index="${index}">
                <div class="message-bubble">
                    ${voiceBadgeHtml}
                    ${intentHtml}
                    <div class="markdown-body">${contentHtml}</div>
                    ${traceActionsHtml}
                    ${tokenUsageHtml}
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

        // 日志出现时，确保卡片是展开的，并移除 hidden
        streamer.classList.remove('hidden');
        card.classList.remove('collapsed');

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
        const cachedToolUseId = this.resolveToolUseId(event);
        const { toolName, args, toolUseId } = event;
        // 使用统一解析的 ID
        const finalToolUseId = cachedToolUseId || toolUseId;
        
        if (!this.streamingMessageEl) {
            this.streamingMessageEl = this.createStreamingMessage();
            this.messagesContainer.appendChild(this.streamingMessageEl);
        }

        const markdownBody = this.streamingMessageEl.querySelector('.markdown-body');
        if (markdownBody) {
            const html = `
                <div class="tool-use-card collapsible" data-tool-use-id="${finalToolUseId}">
                    <div class="tool-name">🛠️ ${toolName} <span class="collapse-icon">⌄</span></div>
                    <pre class="tool-args">${JSON.stringify(args || {}, null, 2)}</pre>
                    <div class="tool-log-streamer hidden"></div>
                    <div class="tool-result-container" data-rel-id="${finalToolUseId}"></div>
                </div>
            `;
            markdownBody.insertAdjacentHTML('beforeend', html);
            this.scrollToBottom();
            
            // 检查并立即渲染缓存的结果
            const sessionId = event.sessionId || this.state.currentSessionId;
            const sessionCache = this.pendingToolResults.get(sessionId);
            if (sessionCache && sessionCache.has(finalToolUseId)) {
                const cachedResult = sessionCache.get(finalToolUseId);
                this.renderCachedResult(markdownBody, finalToolUseId, cachedResult);
                sessionCache.delete(finalToolUseId);
            }
        }
    }

    private handleToolResult(event: any) {
        const { toolUseId, result, isError } = event;
        this.handleProjectManagerResult(event);
        
        // 当 streamingMessageEl 不存在时，缓存结果以便后续渲染
        if (!this.streamingMessageEl) {
            const sessionId = event.sessionId || this.state.currentSessionId;
            if (!this.pendingToolResults.has(sessionId)) {
                this.pendingToolResults.set(sessionId, new Map());
            }
            const sessionCache = this.pendingToolResults.get(sessionId);
            if (sessionCache) {
                sessionCache.set(toolUseId, { result, isError, sessionId });
            }
            return;
        }

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

            // 将 has-result 类添加到最近的 tool-result-container
            const resultContainer = markdownBody.querySelector(`.tool-result-container[data-rel-id="${toolUseId}"]`);
            if (resultContainer) {
                resultContainer.classList.add('has-result');
            }

            // 15s 后自动折叠工具调用和结果，给用户更多阅读时间
            setTimeout(() => {
                // 查找嵌套结构中的 tool-use-card（通过 closest 向上查找）
                const toolCard = markdownBody.querySelector(`.tool-result-container[data-rel-id="${toolUseId}"]`)
                    ?.closest('.tool-use-card');
                const resultCard = markdownBody.querySelector(`.tool-result-card[data-rel-id="${toolUseId}"]`);
                if (toolCard) toolCard.classList.add('collapsed');
                if (resultCard) resultCard.classList.add('collapsed');
                this.updateMinimap();
            }, 15000);
        }
    }

    private handleProjectManagerResult(event: any) {
        const toolName = typeof event.toolName === 'string' ? event.toolName : event.tool;
        if (toolName !== 'ProjectManager' || event.isError) {
            return;
        }

        const projectDir = this.readProjectDirFromToolResult(event.result);
        console.info('[ChatView] ProjectManager result received:', {
            sessionId: event.sessionId,
            currentSessionId: this.state.currentSessionId,
            projectDir,
            hasGatewayClient: Boolean(this.state.gatewayClient),
        });

        if (event.sessionId !== this.state.currentSessionId) {
            return;
        }

        if (projectDir) {
            this.updateProjectDirFromExternalResult(event.sessionId, projectDir);
            return;
        }

        void this.refreshProjectMenuState();
    }

    private readProjectDirFromToolResult(result: unknown): string | null {
        if (typeof result !== 'string') {
            return null;
        }
        const match = result.match(/Project directory changed to:\s*(.+)/i);
        return match?.[1]?.trim() || null;
    }

    private updateProjectDirFromExternalResult(sessionId: string, projectDir: string) {
        const changed = this.updateProjectDirRuntimeState(sessionId, projectDir);
        console.info('[ChatView] Project menu state applied:', {
            sessionId,
            projectDir,
            changed,
            buttonTextBeforeRender: this.projectMenuBtn?.textContent,
        });
        if (changed && this.pickerVisible) {
            void this.loadProjectEntries(this.pickerCurrentPath);
        }
        this.renderProjectMenu();
        console.info('[ChatView] Project menu rendered:', {
            sessionId,
            projectDir,
            buttonTextAfterRender: this.projectMenuBtn?.textContent,
        });
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
        const streamingMessage = this.streamingMessageEl?.parentElement === this.messagesContainer
            ? this.streamingMessageEl
            : null;
        if (streamingMessage) {
            streamingMessage.insertAdjacentHTML('beforebegin', html);
        } else {
            this.messagesContainer.insertAdjacentHTML('beforeend', html);
        }
        this.scrollToBottom();
    }

    /**
     * 处理 AI 迭代进度，更新状态栏
     */
    private handleIteration(event: any) {
        const { iteration } = event;
        // 构建友好的状态文本
        const statusText = `Agent Running (${iteration}/30)`;
        // 发布运行文案事件，不再写入连接态事件，避免与 TitleBar 连接态冲突
        console.debug('[ChatView] Iteration progress:', statusText);
        this.bus.emit(Events.RUNTIME_STATUS_TEXT, { text: statusText });
        
        // 可选：如果 5s 内没有任何新进度，Titlebar 会保持这个状态直到任务完成（chat:complete 回调会重置状态）
    }

    private clearRuntimeStatusText() {
        this.bus.emit(Events.RUNTIME_STATUS_TEXT_CLEAR);
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
                    const scrollable = htmlMsg as HTMLElement & {
                        scrollIntoView?: (options?: ScrollIntoViewOptions) => void;
                    };
                    scrollable.scrollIntoView?.({ behavior: 'smooth', block: 'center' });
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

    private formatTokenCountInK(value: unknown): string {
        const count = typeof value === 'number' ? value : Number(value ?? 0);
        if (!Number.isFinite(count) || count < 0) {
            return '0k';
        }
        const inK = count / 1000;
        const formatted = inK >= 10 ? inK.toFixed(1) : inK.toFixed(2);
        return `${formatted.replace(/\.?0+$/, '')}k`;
    }

    // ========================
    // 停止按钮逻辑
    // ========================

    /**
     * 处理停止按钮点击
     */
    private handleStopClick() {
        const sid = this.state.currentSessionId;
        if (!sid) return;
        if (this.stoppingSessions.has(sid)) return; // 防重复

        this.stoppingSessions.add(sid);
        this.updateSendButton(); // 立即禁用按钮

        if (this.state.gatewayClient) {
            this.state.gatewayClient.stopTask(sid);
        }
    }

    /**
     * 更新发送按钮状态
     */
    private updateSendButton() {
        const sid = this.state.currentSessionId;
        const isStopping = sid ? this.stoppingSessions.has(sid) : false;
        const isStreaming = sid ? this.streamingSessions.has(sid) : false;

        if (isStreaming) {
            // 切换为停止图标
            this.sendBtn.classList.add('is-stop');
            this.sendBtn.disabled = isStopping;
            this.sendBtn.setAttribute('aria-label', isStopping ? '停止中...' : '停止生成');
        } else {
            // 恢复发送图标
            this.sendBtn.classList.remove('is-stop');
            this.sendBtn.disabled = false;
            this.sendBtn.setAttribute('aria-label', '发送');
        }
    }

    // ========================
    // 工具结果缓存管理
    // ========================

    /**
     * 清理指定会话的工具结果缓存
     */
    private clearPendingResultsForSession(sessionId: string) {
        this.pendingToolResults.delete(sessionId);
    }

    /**
     * 渲染缓存的工具结果到 streaming message 中
     */
    private renderCachedResult(markdownBody: Element, toolUseId: string, cachedResult: any) {
        if (!markdownBody) return;
        
        const originalContent = cachedResult.result || '';
        let displayContent = '';
        let isErrorCode = this.hasExitCodeError(originalContent, cachedResult.isError);
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
        
        const resultContainer = markdownBody.querySelector(`.tool-result-container[data-rel-id="${toolUseId}"]`);
        if (resultContainer) {
            resultContainer.classList.add('has-result');
        }
    }
}

