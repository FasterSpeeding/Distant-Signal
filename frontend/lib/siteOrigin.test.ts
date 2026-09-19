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

import { getSiteOrigin } from './siteOrigin';

describe('getSiteOrigin', () => {
  afterEach(() => {
    incomingHeaders.clear();
    delete process.env.NEXT_PUBLIC_SITE_URL;
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
});
