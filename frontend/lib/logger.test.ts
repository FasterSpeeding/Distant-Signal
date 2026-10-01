// @vitest-environment node
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { consoleCallToLine } from './consoleBridge';
import { createLogger, errorText, formatLogLine, jsonLoggingActive, toSnakeCase } from './logger';

const NOW = new Date('2026-10-01T09:30:00.123Z');

function parse(line: string): Record<string, unknown> {
  expect(line).not.toContain('\n');
  expect(line).not.toMatch(/\u001b\[/);
  const value = JSON.parse(line) as unknown;
  expect(typeof value).toBe('object');
  return value as Record<string, unknown>;
}

describe('formatLogLine', () => {
  it('writes the shared schema as one JSON line', () => {
    const line = parse(
      formatLogLine('INFO', 'lib/api', 'fetched\nlines', { lineId: 'tfl-victoria', count: 3, ok: true }, NOW),
    );
    expect(line).toEqual({
      timestamp: '2026-10-01T09:30:00.123Z',
      level: 'INFO',
      service: 'frontend',
      target: 'lib/api',
      message: 'fetched\nlines',
      line_id: 'tfl-victoria',
      count: 3,
      ok: true,
    });
    expect(Object.keys(line).slice(0, 5)).toEqual(['timestamp', 'level', 'service', 'target', 'message']);
  });

  it('turns an Error into error and stack fields, with its cause chain', () => {
    const err = new Error('upstream unavailable', { cause: new TypeError('fetch failed') });
    const line = parse(formatLogLine('ERROR', 'app/api/proxy', 'proxy failed', { error: err }, NOW));
    expect(line.level).toBe('ERROR');
    expect(line.error).toBe('upstream unavailable: TypeError: fetch failed');
    expect(String(line.stack)).toContain('upstream unavailable');
    expect(String(line.stack)).toContain('logger.test.ts');
  });

  it('records a non-Error error value as text', () => {
    const line = parse(formatLogLine('WARN', 't', 'm', { error: { status: 502 } }, NOW));
    expect(line.error).toBe('{"status":502}');
    expect(line.stack).toBeUndefined();
  });

  it('redacts secrets, tokens and emails at any depth', () => {
    const line = formatLogLine(
      'INFO',
      't',
      'm',
      {
        accessToken: 'tok-123',
        email: 'someone@example.com',
        request: { headers: { cookie: 'session=abc', authorization: 'Bearer xyz' }, path: '/x' },
        client_secret: 's3cr3t',
        passengers: 4,
      },
      NOW,
    );
    for (const leaked of ['tok-123', 'someone@example.com', 'session=abc', 'Bearer xyz', 's3cr3t']) {
      expect(line).not.toContain(leaked);
    }
    const parsed = parse(line);
    expect(parsed.access_token).toBe('[REDACTED]');
    expect((parsed.request as { path: string }).path).toBe('/x');
    expect(parsed.passengers).toBe(4);
  });

  it('never duplicates a schema key', () => {
    const line = parse(formatLogLine('INFO', 't', 'm', { service: 'api', level: 2 }, NOW));
    expect(line.service).toBe('frontend');
    expect(line.level).toBe('INFO');
    expect(line.field_service).toBe('api');
    expect(line.field_level).toBe(2);
  });

  it('survives circular and non-JSON values', () => {
    const circular: Record<string, unknown> = { a: 1 };
    circular.self = circular;
    const line = parse(formatLogLine('INFO', 't', 'm', { circular, big: BigInt(5), nan: NaN }, NOW));
    expect(line.big).toBe('5');
    expect(line.nan).toBe('NaN');
    expect((line.circular as { self: string }).self).toBe('[…]');
  });
});

describe('createLogger on the server', () => {
  let written: string[];
  beforeEach(() => {
    written = [];
    vi.spyOn(process.stdout, 'write').mockImplementation((chunk: string | Uint8Array) => {
      written.push(String(chunk));
      return true;
    });
  });
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllEnvs();
  });

  it('writes JSON lines to stdout by default', () => {
    expect(jsonLoggingActive()).toBe(true);
    createLogger('lib/legal').warn('placeholders unfilled', { missing: ['CONTACT_EMAIL'] });
    expect(written).toHaveLength(1);
    expect(written[0].endsWith('\n')).toBe(true);
    const line = parse(written[0].trimEnd());
    expect(line).toMatchObject({ level: 'WARN', service: 'frontend', target: 'lib/legal' });
    expect(Number.isNaN(Date.parse(String(line.timestamp)))).toBe(false);
    expect(String(line.timestamp).endsWith('Z')).toBe(true);
  });

  it('prints through the console instead under LOG_FORMAT=pretty', () => {
    vi.stubEnv('LOG_FORMAT', 'pretty');
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    createLogger('t').warn('hello', { a: 1 });
    expect(written).toHaveLength(0);
    expect(warn).toHaveBeenCalledWith('hello', { a: 1 });
  });
});

describe('console bridge', () => {
  it('formats console arguments into message and error', () => {
    const err = new Error('boom');
    expect(consoleCallToLine(['\u001b[31m⨯\u001b[39m', err])).toEqual({ message: 'boom', fields: { error: err } });
    expect(consoleCallToLine(['\u001b[33mrender failed\u001b[39m', err])).toEqual({
      message: 'render failed',
      fields: { error: err },
    });
    expect(consoleCallToLine(['%s took %dms', 'render', 12])).toEqual({ message: 'render took 12ms', fields: {} });
    expect(consoleCallToLine([err])).toEqual({ message: 'boom', fields: { error: err } });
  });
});

describe('helpers', () => {
  it('snake_cases field names', () => {
    expect(toSnakeCase('routePath')).toBe('route_path');
    expect(toSnakeCase('line_id')).toBe('line_id');
    expect(toSnakeCase('x-forwarded-for')).toBe('x_forwarded_for');
  });

  it('renders error text for odd values', () => {
    expect(errorText('plain')).toBe('plain');
    expect(errorText(new RangeError('bad'))).toBe('RangeError: bad');
  });
});
