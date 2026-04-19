import { t } from '../i18n/index';
import { AppState } from '../core/state';
import { EventBus } from '../core/event-bus';
import { escapeHtml } from '../utils/html';

export class SchedulerView {
    private schedulerView: HTMLElement;
    private schedulerBtn: HTMLElement;
    private schedulerRefreshBtn: HTMLElement;
    private schedulerListView: HTMLElement;
    private schedulerTasks: HTMLDivElement;
    private schedulerInlineDetail: HTMLElement;
    private schedulerInlineActions: HTMLElement;
    private schedulerInlineRuns: HTMLDivElement;
    private schedulerTasksWrapper: HTMLElement;
    
    private viewActive = false;
    private selectedTaskId: string | null = null;
    private countdownTimerId: any = null;
    private cachedTasks: any[] = [];

    constructor(private state: AppState, private bus: EventBus) {
        this.schedulerView = document.getElementById('scheduler-view') as HTMLElement;
        this.schedulerBtn = document.getElementById('scheduler-btn') as HTMLElement;
        this.schedulerRefreshBtn = document.getElementById('scheduler-refresh-btn') as HTMLElement;
        this.schedulerListView = document.getElementById('scheduler-list-view') as HTMLElement;
        this.schedulerTasks = document.getElementById('scheduler-tasks') as HTMLDivElement;
        this.schedulerInlineDetail = document.getElementById('scheduler-inline-detail') as HTMLElement;
        this.schedulerInlineActions = document.getElementById('scheduler-inline-actions') as HTMLElement;
        this.schedulerInlineRuns = document.getElementById('scheduler-inline-runs') as HTMLDivElement;
        this.schedulerTasksWrapper = document.getElementById('scheduler-tasks-wrapper') as HTMLElement;
    }

    init() {
        this.schedulerBtn.addEventListener('click', () => this.toggleView());
        this.schedulerRefreshBtn.addEventListener('click', () => this.loadData());
        
        this.bus.on('scheduler:refresh', () => {
            if (this.viewActive) {
                this.loadData();
                if (this.selectedTaskId) {
                    this.renderInlineDetail(this.selectedTaskId);
                    this.loadTaskRuns(this.selectedTaskId);
                }
            }
        });
    }

    toggleView() {
        this.viewActive = !this.viewActive;
        this.bus.emit('view:toggle', { name: 'scheduler', active: this.viewActive });
        
        if (this.viewActive) {
            this.showList();
            this.loadData();
            this.startCountdown();
        } else {
            this.stopCountdown();
            this.selectedTaskId = null;
        }
    }

    showList() {
        this.selectedTaskId = null;
        this.schedulerTasks.querySelectorAll('.scheduler-task-card').forEach(card => {
            (card as HTMLElement).classList.remove('hidden');
        });
        this.schedulerInlineDetail.classList.add('hidden');
        this.schedulerTasksWrapper.classList.remove('detail-mode');
        this.schedulerRefreshBtn.classList.remove('hidden');
        document.getElementById('scheduler-header-back-btn')?.remove();
    }

    async loadData() {
        if (!this.state.gatewayClient) return;
        try {
            this.cachedTasks = await this.state.gatewayClient.getSchedulerTasks();
            this.renderTasks(this.cachedTasks);
        } catch (error) {
            console.error('[Scheduler] Load data failed:', error);
        }
    }

    private renderTasks(tasks: any[]) {
        if (!tasks || tasks.length === 0) {
            this.schedulerTasks.innerHTML = `<div class="scheduler-empty"><p>${t('scheduler.empty')}</p></div>`;
            return;
        }

        const now = Date.now();
        this.schedulerTasks.innerHTML = tasks.map(task => this.renderTaskCard(task, now)).join('');
        
        this.schedulerTasks.querySelectorAll('.scheduler-task-card').forEach(card => {
            card.addEventListener('click', () => {
                const id = (card as HTMLElement).dataset.taskId;
                if (id) this.showDetail(id);
            });
        });
    }

    private renderTaskCard(task: any, now: number): string {
        return `
            <div class="scheduler-task-card" data-task-id="${task.id}">
                <div class="scheduler-task-card-left">
                    <div class="scheduler-task-card-name">${escapeHtml(task.name)}</div>
                    <div class="scheduler-task-card-meta">
                        <span>${task.runCount} 次</span>
                    </div>
                </div>
                <span class="scheduler-task-status-badge ${task.status}">${task.status}</span>
            </div>
        `;
    }

    showDetail(taskId: string) {
        this.selectedTaskId = taskId;
        this.schedulerTasks.querySelectorAll('.scheduler-task-card').forEach(card => {
            const el = card as HTMLElement;
            el.classList.toggle('hidden', el.dataset.taskId !== taskId);
        });

        this.schedulerTasksWrapper.classList.add('detail-mode');
        this.schedulerInlineDetail.classList.remove('hidden');
        this.renderInlineDetail(taskId);
        this.loadTaskRuns(taskId);

        this.schedulerRefreshBtn.classList.add('hidden');
        this.addBackButton();
    }

    private addBackButton() {
        if (document.getElementById('scheduler-header-back-btn')) return;
        const btn = document.createElement('button');
        btn.id = 'scheduler-header-back-btn';
        btn.innerHTML = '←';
        btn.onclick = () => this.showList();
        this.schedulerListView.querySelector('.scheduler-view-header')?.prepend(btn);
    }

    private renderInlineDetail(taskId: string) {
        // Implementation for buttons (pause, resume, etc.)
    }

    private async loadTaskRuns(taskId: string) {
        if (!this.state.gatewayClient) return;
        const runs = await this.state.gatewayClient.getSchedulerRuns(taskId, 50);
        this.renderRuns(runs);
    }

    private renderRuns(runs: any[]) {
        // Implementation for run history rows
    }

    private startCountdown() {
        this.stopCountdown();
        this.countdownTimerId = setInterval(() => this.updateCountdowns(), 1000);
    }

    private stopCountdown() {
        if (this.countdownTimerId) clearInterval(this.countdownTimerId);
    }

    private updateCountdowns() {
        // Update countdown text on UI elements
    }
}
