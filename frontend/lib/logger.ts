/**
 * Server-side structured logging: one JSON object per line on stdout, the
 * same schema as the Rust services' `common::logging` (and the MCP server):
 *
 * - `timestamp`: RFC 3339, UTC (`2026-10-01T09:30:00.123Z`);
 * - `level`: `DEBUG`, `INFO`, `WARN` or `ERROR`;
 * - `service`: always `frontend`;
 * - `target`: the logging module (`lib/api`, `app/lines/history`, `next`);
 * - `message`;
 * - the caller's fields, flattened to the top level as snake_case keys;
 * - `error` / `stack` for an `Error` (passed as the `error` field, or as any
 *   field holding an `Error`).
 *
 * No spans (the Rust services' optional `spans` key has no equivalent here).
 * `LOG_FORMAT=pretty` (read per call) prints `console.<level>(message,
 * fields)` instead, for `next dev`. In a browser, or anywhere without
 * `process.stdout` (the test suite's jsdom), the logger also falls back to
 * the console, so client-side logging is unchanged.
 *
 * Fields whose name looks like a credential or personal data (password,
 * secret, token, cookie, authorization, API key, email) are written as
 * `[REDACTED]`, at any depth: secrets, tokens and email addresses are never
 * logged.
 */

export type LogLevel = 'DEBUG' | 'INFO' | 'WARN' | 'ERROR';
export type LogFields = Record<string, unknown>;

export const SERVICE = 'frontend';

/** Keys the line owns; a caller field with one of these names is written
 * as `field_<name>`, so a line never has duplicate keys. */
const RESERVED = new Set(['timestamp', 'level', 'service', 'target', 'message', 'stack']);

const SENSITIVE_KEY = /passw(or)?d|passphrase|secret|token|cookie|authori[sz]ation|api_?key|e_?mail/i;

const MAX_DEPTH = 6;

export function toSnakeCase(key: string): string {
  return key
    .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
    .replace(/[^A-Za-z0-9_]+/g, '_')
    .toLowerCase();
}

/** An error's message with its `cause` chain, `: `-joined (like the Rust
 * side's `{:#}`), on one line of JSON. */
export function errorText(err: unknown): string {
  const parts: string[] = [];
  let current: unknown = err;
  for (let i = 0; i < 5 && current !== undefined && current !== null; i++) {
    if (current instanceof Error) {
      parts.push(current.name && current.name !== 'Error' ? `${current.name}: ${current.message}` : current.message);
      current = (current as { cause?: unknown }).cause;
    } else {
      parts.push(typeof current === 'string' ? current : safeString(current));
      break;
    }
  }
  return parts.join(': ');
}

function safeString(value: unknown): string {
  try {
    // Objects go through JSON.stringify; only functions, symbols and
    // primitives reach String(), and none of them prints as [object Object].
    // eslint-disable-next-line @typescript-eslint/no-base-to-string -- see above
    return typeof value === 'object' ? JSON.stringify(value) : String(value);
  } catch {
    return String(value);
  }
}

/** A JSON-safe copy of `value` with sensitive keys redacted. */
function sanitize(value: unknown, depth: number, seen: WeakSet<object>): unknown {
  if (value === null || typeof value !== 'object') {
    if (typeof value === 'bigint') return value.toString();
    if (typeof value === 'function' || typeof value === 'symbol') return String(value);
    if (typeof value === 'number' && !Number.isFinite(value)) return String(value);
    return value;
  }
  if (value instanceof Error) return errorText(value);
  if (value instanceof Date) return Number.isNaN(value.getTime()) ? String(value) : value.toISOString();
  if (value instanceof URL) return value.toString();
  if (seen.has(value) || depth >= MAX_DEPTH) return '[…]';
  seen.add(value);
  if (Array.isArray(value)) return value.map((item) => sanitize(item, depth + 1, seen));
  const out: Record<string, unknown> = {};
  for (const [key, inner] of Object.entries(value)) {
    out[key] = SENSITIVE_KEY.test(key) ? '[REDACTED]' : sanitize(inner, depth + 1, seen);
  }
  return out;
}

/** One log line (without the trailing newline). Exported for the tests and
 * the console bridge. */
export function formatLogLine(
  level: LogLevel,
  target: string,
  message: string,
  fields: LogFields = {},
  now: Date = new Date(),
): string {
  const entry: Record<string, unknown> = {
    timestamp: now.toISOString(),
    level,
    service: SERVICE,
    target,
    message,
  };
  const seen = new WeakSet<object>();
  for (const [rawKey, value] of Object.entries(fields)) {
    if (value === undefined) continue;
    const key = toSnakeCase(rawKey);
    if (SENSITIVE_KEY.test(key)) {
      entry[RESERVED.has(key) ? `field_${key}` : key] = '[REDACTED]';
      continue;
    }
    if (value instanceof Error || key === 'error') {
      entry.error = errorText(value);
      if (value instanceof Error && value.stack) entry.stack = value.stack;
      const digest = value instanceof Error ? (value as { digest?: unknown }).digest : undefined;
      if (typeof digest === 'string') entry.digest = digest;
      continue;
    }
    entry[RESERVED.has(key) ? `field_${key}` : key] = sanitize(value, 0, seen);
  }
  return JSON.stringify(entry);
}

type ConsoleMethod = 'debug' | 'info' | 'warn' | 'error';
const CONSOLE_METHOD: Record<LogLevel, ConsoleMethod> = {
  DEBUG: 'debug',
  INFO: 'info',
  WARN: 'warn',
  ERROR: 'error',
};

function serverStdout(): { write(line: string): unknown } | undefined {
  if (typeof window !== 'undefined') return undefined;
  if (typeof process === 'undefined') return undefined;
  const stdout = (process as { stdout?: { write?: unknown } }).stdout;
  return stdout && typeof stdout.write === 'function' ? (stdout as { write(line: string): unknown }) : undefined;
}

/** Whether log lines go out as JSON (the default) or as plain console
 * calls (`LOG_FORMAT=pretty`, the browser, the jsdom test suite). */
export function jsonLoggingActive(): boolean {
  if (typeof process !== 'undefined') {
    const proc: Partial<Pick<NodeJS.Process, 'env'>> = process;
    if (proc.env?.LOG_FORMAT?.trim().toLowerCase() === 'pretty') return false;
  }
  return serverStdout() !== undefined;
}

/** The console's original methods, so the console bridge (which replaces
 * them) can still print through them in pretty mode without recursing. */
export const originalConsole: Record<ConsoleMethod, (...args: unknown[]) => void> = {
  debug: console.debug.bind(console),
  info: console.info.bind(console),
  warn: console.warn.bind(console),
  error: console.error.bind(console),
};

export function emit(
  level: LogLevel,
  target: string,
  message: string,
  fields?: LogFields,
  viaConsole: Pick<Console, ConsoleMethod> = console,
): void {
  const stdout = jsonLoggingActive() ? serverStdout() : undefined;
  if (stdout) {
    stdout.write(formatLogLine(level, target, message, fields) + '\n');
    return;
  }
  // `console` is read per call (the default argument), so test spies see it.
  const method = CONSOLE_METHOD[level];
  if (fields && Object.keys(fields).length > 0) {
    viaConsole[method](message, fields);
  } else {
    viaConsole[method](message);
  }
}

export interface Logger {
  debug(message: string, fields?: LogFields): void;
  info(message: string, fields?: LogFields): void;
  warn(message: string, fields?: LogFields): void;
  error(message: string, fields?: LogFields): void;
}

/** A logger whose lines carry `target` (conventionally the module path,
 * e.g. `lib/api`). */
export function createLogger(target: string): Logger {
  return {
    debug: (message, fields) => emit('DEBUG', target, message, fields),
    info: (message, fields) => emit('INFO', target, message, fields),
    warn: (message, fields) => emit('WARN', target, message, fields),
    error: (message, fields) => emit('ERROR', target, message, fields),
  };
}
