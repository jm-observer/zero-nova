/**
 * Phase 3: 类型守卫函数（Type Guards）
 * 用于运行时类型检查和验证
 */

import type { ProgressEvent, GatewayMessage } from '../gateway-client';

// ============================================================
// 类型守卫函数
// ============================================================

/**
 * 检查 ProgressEvent 类型
 * 支持动态验证 event 的 type 字段值
 */
export function isProgressEvent(value: unknown): value is ProgressEvent {
  return (
    typeof value === 'object' &&
    value !== null &&
    'type' in value &&
    ['iteration', 'thinking', 'tool_start', 'tool_result', 'token', 'complete', 'turn_complete', 'iteration_limit', 'tool_log'].includes(
      (value as ProgressEvent).type
    )
  );
}

/**
 * 检查 GatewayMessage 类型
 */
export function isGatewayMessage(value: unknown): value is GatewayMessage {
  return (
    typeof value === 'object' &&
    value !== null &&
    'type' in value &&
    typeof (value as GatewayMessage).type === 'string'
  );
}

/**
 * 检查是否是工具开始事件
 */
export function isToolStartEvent(event: ProgressEvent): boolean {
  return event.type === 'tool_start' && typeof event.tool === 'string';
}

/**
 * 检查是否是 token 流事件
 */
export function isTokenStreamEvent(event: ProgressEvent): boolean {
  return event.type === 'token' && typeof event.token === 'string';
}

/**
 * 检查是否是思考内容事件
 */
export function isThinkingEvent(event: ProgressEvent): boolean {
  return event.type === 'thinking' && typeof event.thinking === 'string';
}

/**
 * 检查是否是完成事件
 */
export function isCompleteEvent(event: ProgressEvent): boolean {
  return event.type === 'complete';
}

/**
 * 检查是否是错误相关事件（isError 或 type 包含 'error'）
 */
export function isErrorEvent(event: ProgressEvent): boolean {
  return event.isError === true || event.type.endsWith('.error') || event.type === 'error';
}

// ============================================================
// 数据转换辅助函数
// ============================================================

/**
 * 规范化 ProgressEvent — 统一 tool/toolName/toolUseId 字段
 */
export function normalizeProgressEvent(event: ProgressEvent): ProgressEvent {
  const normalized: ProgressEvent = { ...event };

  // toolName -> tool 兼容
  if (normalized.toolName && !normalized.tool) {
    normalized.tool = normalized.toolName;
  }
  // tool -> toolName 兼容
  if (!normalized.toolName && normalized.tool) {
    normalized.toolName = normalized.tool;
  }
  // toolUseId 标准化
  if (normalized.toolUseId && !('tool_use_id' in (normalized as Record<string, unknown>))) {
    (normalized as Record<string, unknown>).tool_use_id = normalized.toolUseId;
  }

  return normalized;
}

/**
 * 安全的数字转换 — 处理可能的字符串数字
 */
export function toNumber(value: unknown, fallback: number = 0): number {
  if (typeof value === 'number') return value;
  if (typeof value === 'string') {
    const parsed = Number(value);
    return isNaN(parsed) ? fallback : parsed;
  }
  return fallback;
}

/**
 * 安全的布尔转换
 */
export function toBoolean(value: unknown, fallback: boolean = false): boolean {
  if (typeof value === 'boolean') return value;
  if (typeof value === 'string') {
    return value.toLowerCase() === 'true';
  }
  return !!value;
}
