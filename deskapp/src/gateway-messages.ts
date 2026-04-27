import {
  ProtocolErrorHandler,
  type ValidationHint,
  validateEnvelope,
  validateGatewayMessage,
  validateInboundPayload,
  validateOutboundPayload,
} from './generated/generated-types';

export interface ParsedMessage {
  id?: string;
  type: string;
  payload?: unknown;
}

export function validateOutboundMessage(type: string, payload: unknown): ValidationHint[] {
  return validateOutboundPayload(type, payload);
}

export function parseInboundMessage(
  raw: string,
  errorHandler: ProtocolErrorHandler,
): { parsed: ParsedMessage | null; errors: ValidationHint[] } {
  let decoded: unknown;

  try {
    decoded = JSON.parse(raw);
  } catch (error) {
    errorHandler.record();
    return {
      parsed: null,
      errors: [{ path: '$', expected: 'valid JSON', got: raw, message: error instanceof Error ? error.message : 'invalid json' }],
    };
  }

  if (!isRecord(decoded)) {
    errorHandler.record();
    return { parsed: null, errors: [{ path: '$', expected: 'object', got: decoded }] };
  }

  const errors: ValidationHint[] = validateGatewayMessage(decoded);

  if (errors.length === 0) {
    const envelopeHints = validateEnvelope(decoded, ['payload-required']);
    errors.push(...envelopeHints.map((hint) => ({
      path: hint === 'missing-type' ? 'type' : 'payload',
      expected: hint === 'missing-type' ? 'string' : 'present',
      got: hint,
    })));
  }

  const parsed: ParsedMessage = {
    id: typeof decoded.id === 'string' ? decoded.id : undefined,
    type: typeof decoded.type === 'string' ? decoded.type : '',
    payload: decoded.payload,
  };

  if (parsed.type && errors.length === 0) {
    errors.push(...validateInboundPayload(parsed.type, parsed.payload));
  }

  if (errors.length > 0) {
    errorHandler.record();
  } else {
    errorHandler.reset();
  }

  return { parsed, errors };
}

export function serializeMessage(message: ParsedMessage, strict = false): string {
  if (strict) {
    const envelopeHints = validateEnvelope(message, ['payload-required']);
    if (envelopeHints.length > 0) {
      console.warn('serializeMessage received a non-standard gateway envelope', envelopeHints);
    }
  }

  return JSON.stringify(message);
}

export function normalizeProgressEvent(event: unknown): Record<string, unknown> {
  if (!isRecord(event)) {
    return { type: 'unknown' };
  }

  const normalized: Record<string, unknown> = { ...event };
  if (typeof normalized.toolName === 'string' && typeof normalized.tool !== 'string') {
    normalized.tool = normalized.toolName;
  }
  if (typeof normalized.tool === 'string' && typeof normalized.toolName !== 'string') {
    normalized.toolName = normalized.tool;
  }
  if (typeof normalized.tool_use_id === 'string' && typeof normalized.toolUseId !== 'string') {
    normalized.toolUseId = normalized.tool_use_id;
  }
  if (typeof normalized.toolUseId === 'string' && typeof normalized.tool_use_id !== 'string') {
    normalized.tool_use_id = normalized.toolUseId;
  }

  return normalized;
}

export function createValidatedHandler<T>(handler: (payload: T) => void, validator: (payload: unknown) => payload is T) {
  return (message: ParsedMessage) => {
    if (validator(message.payload)) {
      handler(message.payload);
    }
  };
}

export function trackConsecError(errorHandler: ProtocolErrorHandler): boolean {
  errorHandler.record();
  return errorHandler.exceeded();
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}
