/**
 * HTML 转义
 */
export function escapeHtml(text: string): string {
    if (typeof text !== 'string') return String(text || '');
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

/**
 * 格式化文件大小
 */
export function formatFileSize(bytes?: number): string {
    if (bytes === undefined || bytes === null) return '';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
/**
 * 格式化时间
 */
export function formatTime(timestamp: number): string {
    const d = new Date(timestamp);
    if (isNaN(d.getTime())) return '';
    const now = new Date();
    const isToday = d.getFullYear() === now.getFullYear() && 
                    d.getMonth() === now.getMonth() && 
                    d.getDate() === now.getDate();
    
    const timeStr = d.toLocaleTimeString('zh-CN', { hour12: false, hour: '2-digit', minute: '2-digit' });
    if (isToday) return timeStr;
    
    return `${d.getMonth() + 1}/${d.getDate()} ${timeStr}`;
}
