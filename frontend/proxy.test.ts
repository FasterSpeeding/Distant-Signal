import { afterEach, describe, it, expect, vi } from 'vitest';
import { NextRequest } from 'next/server';
import { proxy, config } from './proxy';

function nonceOf(csp: string | null): string {
  const m = csp?.match(/'nonce-([^']+)'/);
  if (!m) throw new Error(`no nonce in ${csp}`);
  return m[1];
}

describe('proxy', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('sets a nonce-based CSP on the response and hands the same nonce to rendering', () => {
    const res = proxy(new NextRequest('http://localhost:3000/lines'));
    const csp = res.headers.get('Content-Security-Policy');
    const nonce = nonceOf(csp);
    const scriptSrc = csp?.split('; ').find((d) => d.startsWith('script-src '));
    expect(scriptSrc).not.toContain('unsafe-inline');
    expect(csp).toMatch(/script-src 'self' 'nonce-[^']+' 'strict-dynamic'(;| 'unsafe-eval';)/);
    // NextResponse.next({ request: { headers } }) encodes the overridden
    // request headers as x-middleware-request-* on the response.
    expect(res.headers.get('x-middleware-request-x-nonce')).toBe(nonce);
    expect(res.headers.get('x-middleware-request-content-security-policy')).toBe(csp);
  });

  it('mints a different nonce per request', () => {
    const a = nonceOf(proxy(new NextRequest('http://localhost:3000/')).headers.get('Content-Security-Policy'));
    const b = nonceOf(proxy(new NextRequest('http://localhost:3000/')).headers.get('Content-Security-Policy'));
    expect(a).not.toBe(b);
  });

  it('overwrites a client-supplied x-nonce', () => {
    const req = new NextRequest('http://localhost:3000/', { headers: { 'x-nonce': 'attacker' } });
    const res = proxy(req);
    expect(res.headers.get('x-middleware-request-x-nonce')).not.toBe('attacker');
  });

  it('reads the railMcp origin from the environment at request time (FE-2)', () => {
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', 'https://railmcp.example.com/mcp');
    const csp = proxy(new NextRequest('http://localhost:3000/chat')).headers.get('Content-Security-Policy');
    expect(csp).toContain("connect-src 'self' https://api.anthropic.com https://railmcp.example.com;");
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', '');
    const without = proxy(new NextRequest('http://localhost:3000/chat')).headers.get('Content-Security-Policy');
    expect(without).toContain("connect-src 'self' https://api.anthropic.com;");
  });
});

describe('proxy matcher', () => {
  const source = config.matcher[0].source;
  // Next compiles the matcher with path-to-regexp; the source is a single
  // regex group, so an anchored RegExp over it is the same test.
  const re = new RegExp(`^${source}$`);

  it.each(['/', '/lines', '/lines/abc', '/chat', '/chat/callback', '/connect-claude/authorize', '/apiary', '/icon.svg.bak', '/attribution', '/privacy', '/terms', '/account', '/healthzz'])(
    'runs for page %s',
    (path) => {
      expect(re.test(path)).toBe(true);
    },
  );

  it.each([
    '/api',
    '/api/lines',
    '/api/auth/callback',
    '/_next/static/chunks/x.js',
    '/_next/image',
    '/healthz',
    '/robots.txt',
    '/sw.js',
    '/sw-cache-rules.js',
    '/offline.html',
    '/manifest.webmanifest',
    '/favicon.ico',
    '/icon.svg',
    '/apple-icon.png',
    '/icon-192.png',
    '/icon-512.png',
  ])('skips %s', (path) => {
    expect(re.test(path)).toBe(false);
  });

  it('skips next/link prefetches', () => {
    expect(config.matcher[0].missing).toEqual([
      { type: 'header', key: 'next-router-prefetch' },
      { type: 'header', key: 'purpose', value: 'prefetch' },
    ]);
  });
});
