/**
 * 完成提示音模块
 *
 * 使用 Web Audio API 合成一个简短的"叮"声，
 * 在聊天完成时通知用户。
 *
 * 设计要点：
 * - 无需外部音频文件
 * - 模块级单例 AudioContext（浏览器要求用户交互后才能创建）
 * - 1 秒冷却时间去重，防止快速连续 done 重复播放
 * - 静默失败，不阻塞主流程
 */

// 声音参数常量
const SOUND_CONFIG = {
    frequency: 800,      // Hz — 提示音频率
    duration: 0.2,       // 秒 — 提示音时长
    volume: 0.5,         // 0-1 — 音量
    cooldown: 1000,      // 毫秒 — 播放冷却时间
} as const;

// 模块级状态
let lastPlayTime = 0;
let audioContext: AudioContext | null = null;

/**
 * 获取或创建 AudioContext。
 * 浏览器要求用户交互后才能创建 AudioContext，
 * 因此延迟到首次播放时创建。
 */
function getAudioContext(): AudioContext {
    if (!audioContext || audioContext.state === 'closed') {
        audioContext = new AudioContext();
    }
    return audioContext;
}

/**
 * 播放完成提示音。
 *
 * 使用正弦波 + 指数衰减生成自然的"叮"声效果。
 * 同一轮对话的连续 done 事件会通过冷却时间去重。
 */
export function playCompletionSound(): void {
    const now = Date.now();

    // 冷却时间去重
    if (now - lastPlayTime < SOUND_CONFIG.cooldown) {
        return;
    }

    lastPlayTime = now;

    try {
        const ctx = getAudioContext();

        // 创建振荡器（正弦波）
        const oscillator = ctx.createOscillator();
        oscillator.type = 'sine';
        oscillator.frequency.setValueAtTime(SOUND_CONFIG.frequency, ctx.currentTime);

        // 创建增益节点（音量控制）
        const gainNode = ctx.createGain();
        gainNode.gain.setValueAtTime(SOUND_CONFIG.volume, ctx.currentTime);

        // 指数衰减到 0（自然的"叮"声效果）
        gainNode.gain.exponentialRampToValueAtTime(
            0.001,
            ctx.currentTime + SOUND_CONFIG.duration,
        );

        // 连接：振荡器 → 增益 → 输出
        oscillator.connect(gainNode);
        gainNode.connect(ctx.destination);

        // 播放
        oscillator.start(ctx.currentTime);
        oscillator.stop(ctx.currentTime + SOUND_CONFIG.duration);

        // 清理引用（避免内存泄漏）
        setTimeout(() => {
            oscillator.disconnect();
            gainNode.disconnect();
        }, SOUND_CONFIG.duration * 1000 + 100);

    } catch (error) {
        // 静默失败（如 AudioContext 被阻止或浏览器不支持）
        console.debug('[Sound] Failed to play completion sound:', error);
    }
}

/**
 * 重置声音状态。
 * 主要用于测试或手动触发场景。
 */
export function resetSoundState(): void {
    lastPlayTime = 0;
}
