import { t } from '../i18n/index';
import { AppState } from '../core/state';
import { EventBus, Events } from '../core/event-bus';
import { formatTime, escapeHtml } from '../utils/html';

export class SidebarView {
    private sessionList: HTMLElement;
    private newSessionBtn: HTMLElement;
    private sidebarToggle: HTMLElement;
    private sidebar: HTMLElement;

    constructor(private state: AppState, private bus: EventBus) {
        this.sessionList = document.getElementById('session-list') as HTMLElement;
        this.newSessionBtn = document.getElementById('new-session-btn') as HTMLElement;
        this.sidebarToggle = document.getElementById('sidebar-toggle') as HTMLElement;
        this.sidebar = document.getElementById('sidebar') as HTMLElement;
    }

    init() {
        console.log('[SidebarView] Initializing...');
        this.newSessionBtn.addEventListener('click', () => this.bus.emit('session:create'));
        
        this.sidebarToggle.addEventListener('click', () => {
            this.sidebar.classList.toggle('collapsed');
            if (!this.sidebar.classList.contains('collapsed')) {
                const saved = localStorage.getItem('sidebar-width');
                if (saved) this.sidebar.style.width = saved + 'px';
            } else {
                this.sidebar.style.width = '';
            }
        });

        this.bus.on(Events.SESSION_UPDATED, () => {
            console.log('[SidebarView] Sessions updated, rendering...');
            this.renderSessions();
        });

        this.initResize();
    }

    private initResize() {
        const handle = document.getElementById('sidebar-resize-handle')!;
        const SIDEBAR_MIN = 180, SIDEBAR_MAX = 480;

        handle.addEventListener('mousedown', (e) => {
            if (this.sidebar.classList.contains('collapsed')) return;
            
            const startX = e.clientX;
            const startWidth = this.sidebar.getBoundingClientRect().width;
            
            handle.classList.add('active');
            document.body.classList.add('resizing');
            
            const onMove = (ev: MouseEvent) => {
                const diff = ev.clientX - startX;
                const newW = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, startWidth + diff));
                this.sidebar.style.width = newW + 'px';
            };
            
            const onUp = () => {
                document.removeEventListener('mousemove', onMove);
                document.removeEventListener('mouseup', onUp);
                handle.classList.remove('active');
                document.body.classList.remove('resizing');
                const w = this.sidebar.getBoundingClientRect().width;
                localStorage.setItem('sidebar-width', String(Math.round(w)));
            };
            
            document.addEventListener('mousemove', onMove);
            document.addEventListener('mouseup', onUp);
        });
    }

    async renderSessions() {
        const sessions = this.state.sessions;
        if (sessions.length === 0) {
            this.sessionList.innerHTML = `<div class="empty-state">${t('misc.no_sessions')}</div>`;
            return;
        }

        this.sessionList.innerHTML = sessions
            .sort((a, b) => b.createdAt - a.createdAt)
            .map(session => this.renderSessionItem(session))
            .join('');

        this.bindItemEvents();
    }

    private renderSessionItem(session: any) {
        const active = session.id === this.state.currentSessionId ? ' active' : '';
        return `
            <div class="session-item${active}" data-session-id="${session.id}">
                <div class="session-item-content">
                    <div class="session-title">${escapeHtml(session.title || t('app.new_session'))}</div>
                    <div class="session-time">${formatTime(session.createdAt)}</div>
                </div>
            </div>
        `;
    }

    private bindItemEvents() {
        this.sessionList.querySelectorAll('.session-item').forEach(item => {
            item.addEventListener('click', () => {
                const id = (item as HTMLElement).dataset.sessionId;
                if (id) this.bus.emit('session:select', { id });
            });
        });
    }
}
