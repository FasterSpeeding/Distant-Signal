import { describe, it, expect, vi, afterEach } from 'vitest';

// Same shape as liveDataCache.test.ts's own `next/headers` stub -- there is
// no Next request context in a unit test. `incomingHeaders` is what each
// test dials in.
const incomingHeaders = new Map<string, string>();
vi.mock('next/headers', () => ({
  headers: async () => ({
    get: (name: string) => incomingHeaders.get(name) ?? null,
  }),
}));

import { getSiteOrigin, __resetSiteOriginWarningForTests } from './siteOrigin';

describe('getSiteOrigin', () => {
  afterEach(() => {
    incomingHeaders.clear();
    delete process.env.NEXT_PUBLIC_SITE_URL;
    __resetSiteOriginWarningForTests();
    vi.restoreAllMocks();
  });

  it('prefers NEXT_PUBLIC_SITE_URL when set, trimming a trailing slash', async () => {
    process.env.NEXT_PUBLIC_SITE_URL = 'https://distant-signal.example/';
    expect(await getSiteOrigin()).toBe('https://distant-signal.example');
  });

  it('falls back to the request Host header with https when NEXT_PUBLIC_SITE_URL is unset', async () => {
    incomingHeaders.set('host', 'distant-signal.example');
    expect(await getSiteOrigin()).toBe('https://distant-signal.example');
  });

  it('honours x-forwarded-proto over the non-localhost default', async () => {
    incomingHeaders.set('host', 'distant-signal.example');
    incomingHeaders.set('x-forwarded-proto', 'http');
    expect(await getSiteOrigin()).toBe('http://distant-signal.example');
  });

  it('defaults localhost to http, since dev has no TLS-terminating proxy in front of it', async () => {
    incomingHeaders.set('host', 'localhost:3000');
    expect(await getSiteOrigin()).toBe('http://localhost:3000');
  });

  it('falls back to localhost:3000 when there is no Host header at all', async () => {
    expect(await getSiteOrigin()).toBe('http://localhost:3000');
  });

  // Signal Box Audit, flib Low finding: production never actually sets
  // NEXT_PUBLIC_SITE_URL, so the request-header fallback below silently
  // becomes the norm rather than a rare dev-only path. This warning is the
  // code-level guard against that going unnoticed.
  it('warns once when falling back to the request Host header because NEXT_PUBLIC_SITE_URL is unset', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    incomingHeaders.set('host', 'distant-signal.example');
    await getSiteOrigin();
    await getSiteOrigin();
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn.mock.calls[0][0]).toContain('NEXT_PUBLIC_SITE_URL');
  });

  it('does not warn when NEXT_PUBLIC_SITE_URL is configured', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    process.env.NEXT_PUBLIC_SITE_URL = 'https://distant-signal.example/';
    await getSiteOrigin();
    expect(warn).not.toHaveBeenCalled();
  });

  // A malformed or hostile Host header (here, one carrying an embedded
  // path/credential-like segment) must not be trusted verbatim -- it ends up
  // in share/invite links and in the same-origin checks
  // app/connect-claude/authorize/route.ts and app/api/[...path]/route.ts
  // build from this value.
  it('falls back to localhost:3000 when the Host header is not a well-formed hostname[:port]', async () => {
    incomingHeaders.set('host', 'evil.example/@attacker.example');
    expect(await getSiteOrigin()).toBe('http://localhost:3000');
  });

  it('rejects a Host header carrying embedded whitespace/CRLF', async () => {
    incomingHeaders.set('host', 'distant-signal.example\r\nX-Injected: 1');
    expect(await getSiteOrigin()).toBe('http://localhost:3000');
  });
});
