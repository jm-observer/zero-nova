import { t } from '../i18n/index';
import { AppState } from '../core/state';
import { EventBus, Events } from '../core/event-bus';
import { escapeHtml } from '../utils/html';

export class SidebarView {
    private sessionList: HTMLElement;
    private agentList: HTMLElement;
    private newSessionBtn: HTMLElement;
    private sidebarToggle: HTMLElement;
    private sidebar: HTMLElement;

    constructor(private state: AppState, private bus: EventBus) {
        this.sessionList = document.getElementById('session-list') as HTMLElement;
        this.agentList = document.getElementById('agent-list') as HTMLElement;
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

        this.bus.on('agents:updated', () => {
            console.log('[SidebarView] Agents updated, rendering...');
            this.renderAgents();
        });

        this.bus.on(Events.AGENT_SWITCHED, (payload: { agentId: string }) => {
            this.updateActiveAgent(payload.agentId);
            this.renderSessions(); // Re-filter sessions for new agent
        });

        this.bus.on(Events.SESSION_SELECTED, (payload: { sessionId: string }) => {
            console.log('[SidebarView] Session selected:', payload.sessionId);
            this.updateActiveItem(payload.sessionId);
        });

        this.initResize();
    }

    private updateActiveItem(sessionId: string | null) {
        this.sessionList.querySelectorAll('.session-item').forEach(item => {
            const active = (item as HTMLElement).dataset.sessionId === sessionId;
            item.classList.toggle('active', active);
        });
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

    private updateActiveAgent(agentId: string | null) {
        this.agentList.querySelectorAll('.agent-item').forEach(item => {
            const active = (item as HTMLElement).dataset.agentId === agentId;
            item.classList.toggle('active', active);
        });
    }

    renderAgents() {
        const agents = this.state.agentsList;
        if (agents.length === 0) {
            this.agentList.innerHTML = '';
            return;
        }

        this.agentList.innerHTML = agents.map(agent => `
            <div class="agent-item${agent.id === this.state.currentAgentId ? ' active' : ''}" 
                 data-agent-id="${agent.id}" title="${escapeHtml(agent.name)}">
                <div class="agent-avatar" style="background-color: ${agent.color || 'var(--color-primary)'}">
                    ${agent.icon ? agent.icon : agent.name.charAt(0)}
                </div>
            </div>
        `).join('');

        this.agentList.querySelectorAll('.agent-item').forEach(item => {
            item.addEventListener('click', () => {
                const id = (item as HTMLElement).dataset.agentId;
                if (id) this.state.setCurrentAgent(id);
            });
        });
    }

    async renderSessions() {
        const sessions = this.state.sessions.filter(s => s.agentId === this.state.currentAgentId);
        
        if (sessions.length === 0) {
            this.sessionList.innerHTML = `<div class="empty-state">${t('misc.no_sessions')}</div>`;
            return;
        }

        // Group by time
        const groups: Record<string, any[]> = {
            today: [],
            yesterday: [],
            earlier: []
        };

        const now = new Date();
        const today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
        const yesterday = today - 86400000;

        sessions.sort((a, b) => b.createdAt - a.createdAt).forEach(s => {
            if (s.createdAt >= today) groups.today.push(s);
            else if (s.createdAt >= yesterday) groups.yesterday.push(s);
            else groups.earlier.push(s);
        });

        let html = '';
        if (groups.today.length > 0) {
            html += `<div class="session-group-label">${t('misc.today')}</div>`;
            html += groups.today.map(s => this.renderSessionItem(s)).join('');
        }
        if (groups.yesterday.length > 0) {
            html += `<div class="session-group-label">${t('misc.yesterday')}</div>`;
            html += groups.yesterday.map(s => this.renderSessionItem(s)).join('');
        }
        if (groups.earlier.length > 0) {
            html += `<div class="session-group-label">${t('misc.earlier')}</div>`;
            html += groups.earlier.map(s => this.renderSessionItem(s)).join('');
        }

        this.sessionList.innerHTML = html;
        this.bindItemEvents();
    }

    private renderSessionItem(session: any) {
        const active = session.id === this.state.currentSessionId ? ' active' : '';
        return `
            <div class="session-item${active}" data-session-id="${session.id}">
                <div class="session-item-content">
                    <div class="session-title">${escapeHtml(session.title || t('app.new_session'))}</div>
                </div>
                <button class="session-delete-btn" title="${t('misc.delete_session')}">&times;</button>
            </div>
        `;
    }

    private bindItemEvents() {
        this.sessionList.querySelectorAll('.session-item').forEach(item => {
            item.addEventListener('click', (e) => {
                const target = e.target as HTMLElement;
                const id = (item as HTMLElement).dataset.sessionId;
                if (!id) return;

                if (target.classList.contains('session-delete-btn')) {
                    e.stopPropagation();
                    if (confirm(t('app.confirm_delete_session'))) {
                        this.bus.emit(Events.SESSION_DELETE, { id });
                    }
                } else {
                    this.state.setCurrentSession(id);
                }
            });
        });
    }
}
