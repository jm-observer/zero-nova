/**
 * Phase 3: Error Boundary — 前端错误边界组件
 * 用于捕获和展示 Gateway 连接错误的 React/Error Boundary
 */

export type ErrorBoundaryType = 'gateway' | 'ui' | 'unknown';

export interface ErrorBoundaryEvent {
    type: ErrorBoundaryType;
    message: string;
    stack?: string;
    timestamp: number;
    recoverable: boolean;
}

/**
 * 全局错误边界处理器
 * 捕获未处理的 Promise rejection 和同步错误
 */
export class ErrorBoundary {
    private handlers: ((event: ErrorBoundaryEvent) => void)[] = [];
    private errorCount: Map<string, number> = new Map();
    private readonly MAX_COUNT = 3;

    constructor() {
        // 捕获全局 Promise rejection
        window.addEventListener('unhandledrejection', (event) => {
            this.report({
                type: 'gateway',
                message: event.reason?.message || 'Unhandled Promise rejection',
                stack: event.reason?.stack,
                timestamp: Date.now(),
                recoverable: true,
            });
        });

        // 捕获全局同步错误
        window.addEventListener('error', (event) => {
            this.report({
                type: 'ui',
                message: event.message,
                stack: event.error?.stack,
                timestamp: Date.now(),
                recoverable: true,
            });
        });
    }

    /**
     * 添加错误处理回调
     */
    on(errorHandler: (event: ErrorBoundaryEvent) => void): () => void {
        this.handlers.push(errorHandler);
        return () => {
            const index = this.handlers.indexOf(errorHandler);
            if (index !== -1) {
                this.handlers.splice(index, 1);
            }
        };
    }

    /**
     * 报告错误到所有监听器
     */
    private report(event: ErrorBoundaryEvent) {
        const key = `${event.type}:${event.message}`;
        const count = (this.errorCount.get(key) || 0) + 1;
        this.errorCount.set(key, count);

        //  Error 重复超过阈值时标记为非可恢复
        if (count >= this.MAX_COUNT) {
            event.recoverable = false;
        }

        for (const handler of this.handlers) {
            try {
                handler(event);
            } catch (err) {
                console.error('[ErrorBoundary] Handler error:', err);
            }
        }
    }

    /**
     * 获取当前的错误计数
     */
    getErrorCount(key: string): number {
        return this.errorCount.get(key) || 0;
    }

    /**
     * 重置错误计数
     */
    resetErrorCount(): void {
        this.errorCount.clear();
    }
}

// 全局错误边界实例
export let errorBoundary: ErrorBoundary | null = null;

/**
 * 初始化全局错误边界
 */
export function initErrorBoundary(): ErrorBoundary {
    if (!errorBoundary) {
        errorBoundary = new ErrorBoundary();
    }
    return errorBoundary;
}

/**
 * 获取或初始化全局错误边界
 */
export function getErrorBoundary(): ErrorBoundary | null {
    return errorBoundary;
}
