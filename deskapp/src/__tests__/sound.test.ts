/**
 * 完成提示音模块单元测试
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { playCompletionSound, resetSoundState } from '../utils/sound';

describe('playCompletionSound', () => {
    let originalAudioContext: typeof globalThis.AudioContext;
    let originalDateNow: typeof Date.now;
    let mockOscillator: any;
    let mockGainNode: any;
    let mockCtx: any;
    let dateNowValue: number;

    beforeEach(() => {
        // Save originals
        originalAudioContext = globalThis.AudioContext;
        originalDateNow = Date.now;
        dateNowValue = 0;

        // Create mock objects
        mockOscillator = {
            type: 'sine',
            frequency: { setValueAtTime: vi.fn() },
            start: vi.fn(),
            stop: vi.fn(),
            connect: vi.fn(),
            disconnect: vi.fn(),
        };

        mockGainNode = {
            gain: {
                setValueAtTime: vi.fn(),
                exponentialRampToValueAtTime: vi.fn(),
            },
            connect: vi.fn(),
            disconnect: vi.fn(),
        };

        mockCtx = {
            createOscillator: vi.fn().mockReturnValue(mockOscillator),
            createGain: vi.fn().mockReturnValue(mockGainNode),
            connect: vi.fn(),
            close: vi.fn(),
            state: 'running',
            currentTime: 0,
            destination: {},
        };

        // Mock AudioContext as a class constructor
        globalThis.AudioContext = vi.fn().mockImplementation(function () {
            return mockCtx;
        }) as any;

        // Mock setTimeout
        vi.spyOn(global, 'setTimeout').mockImplementation((cb: any) => {
            cb();
            return 0 as any;
        });

        // Mock Date.now
        vi.spyOn(Date, 'now').mockImplementation(() => dateNowValue);

        // Reset sound state
        resetSoundState();
    });

    afterEach(() => {
        globalThis.AudioContext = originalAudioContext;
        Date.now = originalDateNow;
        vi.restoreAllMocks();
    });

    it('should play sound on first call', () => {
        playCompletionSound();

        // Verify AudioContext was created
        expect(globalThis.AudioContext).toHaveBeenCalled();

        // Verify oscillator was created and configured
        expect(mockCtx.createOscillator).toHaveBeenCalled();
        expect(mockCtx.createGain).toHaveBeenCalled();

        // Verify oscillator was started and stopped
        expect(mockOscillator.start).toHaveBeenCalled();
        expect(mockOscillator.stop).toHaveBeenCalled();
    });

    it('should skip sound within cooldown period', () => {
        // First call
        playCompletionSound();

        // Reset mock call tracking
        mockCtx.createOscillator.mockClear();

        // Second call immediately
        playCompletionSound();

        // Oscillator should NOT be created again (cooldown)
        expect(mockCtx.createOscillator).not.toHaveBeenCalled();
    });

    it('should allow sound after cooldown', () => {
        // First call
        playCompletionSound();

        // Reset mock call tracking
        mockCtx.createOscillator.mockClear();

        // Advance time beyond cooldown (1000ms)
        dateNowValue = 1100;

        // Second call after cooldown
        playCompletionSound();

        // Oscillator should be created again
        expect(mockCtx.createOscillator).toHaveBeenCalled();
    });

    it('should handle AudioContext errors gracefully', () => {
        // Simulate AudioContext creation failure
        globalThis.AudioContext = vi.fn().mockImplementation(function () {
            throw new Error('AudioContext failed');
        }) as any;

        // Should not throw
        expect(() => playCompletionSound()).not.toThrow();
    });

    it('should reset state via resetSoundState', () => {
        playCompletionSound();

        // Skip within cooldown
        playCompletionSound();

        // Reset
        resetSoundState();

        // Reset mock call tracking
        mockCtx.createOscillator.mockClear();

        // Should play again after reset
        playCompletionSound();
        expect(mockCtx.createOscillator).toHaveBeenCalled();
    });
});
