import { afterEach, describe, it, expect, vi } from 'vitest';
import { NextRequest } from 'next/server';
import { proxy, config } from './proxy';

function nonceOf(csp: string | null): string {
  const m = csp?.match(/'nonce-([^']+)'/);
  if (!m) throw new Error(`no nonce in ${csp}`);
  return m[1]!;
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

describe('proxy on /trains', () => {
  it('permanently redirects an old-shape search to the same search in its new names', () => {
    const res = proxy(
      new NextRequest('http://localhost:3000/trains?station=MAN&stops_at=WAT&from=09:00&to_day_offset=1&ticketId=7'),
    );
    expect(res.status).toBe(308);
    const location = new URL(res.headers.get('location')!);
    expect(location.pathname).toBe('/trains');
    expect(Object.fromEntries(location.searchParams)).toEqual({
      from: 'MAN',
      to: 'WAT',
      time: '09:00',
      until_day_offset: '1',
      ticketId: '7',
    });
  });

  it('sends a UID straight to its train page', () => {
    const res = proxy(new NextRequest('http://localhost:3000/trains?uid=w12345&date=2026-10-31'));
    expect(res.status).toBe(307);
    expect(new URL(res.headers.get('location')!).pathname).toBe('/train/W12345/2026-10-31');
  });

  it('renders a current search, with its CSP', () => {
    for (const query of ['', '?station=RDG&time=09:00', '?from=RDG&to=PAD', '?headcode=1S00']) {
      const res = proxy(new NextRequest(`http://localhost:3000/trains${query}`));
      expect(res.headers.get('location'), query).toBeNull();
      expect(res.headers.get('Content-Security-Policy'), query).toContain('nonce-');
    }
  });

  it('leaves other pages with a from= alone', () => {
    const res = proxy(new NextRequest('http://localhost:3000/plan?from=09:00'));
    expect(res.headers.get('location')).toBeNull();
  });
});

describe('proxy matcher', () => {
  const source = config.matcher[0]!.source;
  // Next compiles the matcher with path-to-regexp; the source is a single
  // regex group, so an anchored RegExp over it is the same test.
  const re = new RegExp(`^${source}$`);

  it.each([
    '/',
    '/lines',
    '/lines/abc',
    '/chat',
    '/chat/callback',
    '/connect-claude/authorize',
    '/apiary',
    '/icon.svg.bak',
    '/attribution',
    '/privacy',
    '/terms',
    '/account',
    '/healthzz',
  ])('runs for page %s', (path) => {
    expect(re.test(path)).toBe(true);
  });

  it.each([
    '/api',
    '/api/lines',
    '/api/auth/callback',
    '/_next/static/chunks/x.js',
    '/_next/image',
    '/healthz',
    '/robots.txt',
    '/.well-known/security.txt',
    '/sw.js',
    '/sw-cache-rules.js',
    '/offline.html',
    '/manifest.webmanifest',
    '/favicon.ico',
    '/icon.svg',
    '/apple-icon.png',
    '/icon-192.png',
    '/icon-512.png',
    '/icon-maskable-512.png',
    '/opengraph-image.png',
  ])('skips %s', (path) => {
    expect(re.test(path)).toBe(false);
  });

  // Next's CSP guide skips prefetches (`missing: next-router-prefetch /
  // purpose: prefetch`), which left an HTML page served to such a request
  // with no CSP at all. Every page request must reach the proxy.
  it('does not skip prefetches', () => {
    expect(config.matcher[0]).not.toHaveProperty('missing');
    expect(config.matcher[0]).not.toHaveProperty('has');
  });
});

describe('proxy on prefetch requests', () => {
  it.each<Record<string, string>>([
    { 'next-router-prefetch': '1' },
    { 'next-router-prefetch': '1', rsc: '1' },
    { purpose: 'prefetch' },
    { 'sec-purpose': 'prefetch' },
  ])('sets the nonce CSP on a request with %o', (headers) => {
    const res = proxy(new NextRequest('http://localhost:3000/', { headers }));
    const csp = res.headers.get('Content-Security-Policy');
    const nonce = nonceOf(csp);
    expect(csp).toMatch(/script-src 'self' 'nonce-[^']+' 'strict-dynamic'/);
    expect(res.headers.get('x-middleware-request-x-nonce')).toBe(nonce);
    expect(res.headers.get('x-middleware-request-content-security-policy')).toBe(csp);
  });
});
