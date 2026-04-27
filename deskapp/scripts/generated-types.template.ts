export interface ValidationHint {
  path: string;
  expected: string;
  got?: unknown;
  message?: string;
}

type EnvelopeHint = 'missing-payload' | 'missing-type' | 'not-an-object';
type EnvelopeMode = 'message-only' | 'payload-required';
type Validator = (payload: unknown, path: string) => ValidationHint[];

const DEFAULT_ERROR_THRESHOLD = 5;
const PROGRESS_TYPES = new Set([
  'iteration',
  'thinking',
  'tool_start',
  'tool_result',
  'token',
  'complete',
  'turn_complete',
  'iteration_limit',
  'tool_log',
  'system_log',
]);

const inboundValidators: Record<string, Validator> = {
  welcome: (payload, path) => validateObject(payload, path, {
    requireAuth: { type: 'boolean', required: true },
    setupRequired: { type: 'boolean', required: false },
  }),
  error: (payload, path) => validateObject(payload, path, {
    message: { type: 'string', required: true },
    code: { type: 'string', required: false },
  }),
  chat: (payload, path) => validateObject(payload, path, {
    input: { type: 'string', required: true },
    sessionId: { type: 'string', required: false },
    agentId: { type: 'string', required: false },
  }),
  'chat.complete': (payload, path) => [
    ...validateObject(payload, path, {
      sessionId: { type: 'string', required: true },
      output: { type: 'string', required: false },
      usage: { type: 'object', required: false },
    }),
    ...validateUsage((payload as Record<string, unknown>)?.usage, `${path}.usage`),
  ],
  'chat.progress': (payload, path) => {
    const hints = validateObject(payload, path, {
      type: { type: 'string', required: true },
      sessionId: { type: 'string', required: false },
      iteration: { type: 'number', required: false },
      toolName: { type: 'string', required: false },
      toolUseId: { type: 'string', required: false },
      log: { type: 'string', required: false },
      stream: { type: 'string', required: false },
    });

    if (isRecord(payload) && typeof payload.type === 'string' && !PROGRESS_TYPES.has(payload.type)) {
      hints.push({ path: `${path}.type`, expected: 'known progress type', got: payload.type });
    }

    return hints;
  },
  'skill.activated': (payload, path) => validateObject(payload, path, {
    sessionId: { type: 'string', required: false },
    skillId: { type: 'string', required: true },
    skillName: { type: 'string', required: true },
    sticky: { type: 'boolean', required: true },
    reason: { type: 'string', required: true },
  }),
  'task.status_changed': (payload, path) => validateObject(payload, path, {
    sessionId: { type: 'string', required: false },
    taskId: { type: 'string', required: true },
    taskSubject: { type: 'string', required: true },
    status: { type: 'string', required: true },
    activeForm: { type: 'string', required: false },
    isMainTask: { type: 'boolean', required: false },
  }),
};

const outboundValidators: Record<string, Validator> = {
  'chat.start': (payload, path) => validateObject(payload, path, {
    sessionId: { type: 'string', required: true },
  }),
  'chat.complete': (payload, path) => validateObject(payload, path, {
    sessionId: { type: 'string', required: true },
  }),
  'sessions.create': (payload, path) => validateObject(payload, path, {
    agentId: { type: 'string', required: true },
  }),
  'sessions.list': () => [],
};

export class ProtocolErrorHandler {
  private consecutiveErrors = 0;

  constructor(private readonly threshold: number = DEFAULT_ERROR_THRESHOLD) {}

  record(): number {
    this.consecutiveErrors += 1;
    return this.consecutiveErrors;
  }

  reset(): void {
    this.consecutiveErrors = 0;
  }

  exceeded(): boolean {
    return this.consecutiveErrors > this.threshold;
  }
}

export function validateEnvelope(message: unknown, modes: EnvelopeMode[] = ['payload-required']): EnvelopeHint[] {
  if (!isRecord(message)) {
    return ['not-an-object'];
  }

  const hints: EnvelopeHint[] = [];
  if (typeof message.type !== 'string' || message.type.length === 0) {
    hints.push('missing-type');
  }

  const payloadOptional = modes.includes('message-only');
  if (!payloadOptional && !('payload' in message)) {
    hints.push('missing-payload');
  }

  return hints;
}

export function validateInboundPayload(type: string, payload: unknown): ValidationHint[] {
  const validator = inboundValidators[type];
  return validator ? validator(payload, type) : [];
}

export function validateOutboundPayload(type: string, payload: unknown): ValidationHint[] {
  const validator = outboundValidators[type];
  return validator ? validator(payload, type) : [];
}

function validateUsage(payload: unknown, path: string): ValidationHint[] {
  if (payload === undefined) {
    return [];
  }

  return validateObject(payload, path, {
    inputTokens: { type: 'number', required: true },
    outputTokens: { type: 'number', required: true },
    cacheCreationInputTokens: { type: 'number', required: false },
    cacheReadInputTokens: { type: 'number', required: false },
  });
}

function validateObject(
  payload: unknown,
  path: string,
  fields: Record<string, { type: 'boolean' | 'number' | 'object' | 'string'; required: boolean }>,
): ValidationHint[] {
  if (!isRecord(payload)) {
    return [{ path, expected: 'object', got: payload }];
  }

  const hints: ValidationHint[] = [];
  for (const [fieldName, rule] of Object.entries(fields)) {
    const fieldPath = `${path}.${fieldName}`;
    if (!(fieldName in payload) || payload[fieldName] === undefined) {
      if (rule.required) {
        hints.push({ path: fieldPath, expected: rule.type, got: undefined, message: 'missing required field' });
      }
      continue;
    }

    const value = payload[fieldName];
    if (rule.type === 'object') {
      if (!isRecord(value)) {
        hints.push({ path: fieldPath, expected: 'object', got: value });
      }
      continue;
    }

    if (typeof value !== rule.type) {
      hints.push({ path: fieldPath, expected: rule.type, got: value });
    }
  }

  return hints;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}
