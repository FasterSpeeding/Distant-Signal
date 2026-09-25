import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { NextRequest } from 'next/server';

// `getSiteOrigin()` (lib/siteOrigin.ts) -- now used by both this route's
// Origin/Referer CSRF check and its logged-out login redirect -- reads
// `next/headers` when `NEXT_PUBLIC_SITE_URL` isn't set. There is no Next
// request context in a unit test (same stub shape
// `app/groups/[id]/page.test.tsx`'s own `next/headers` mock uses). Left
// empty by default rather than pre-populated: with no `host` header,
// `getSiteOrigin()` falls back to `http://localhost:3000` -- exactly the
// origin every request below is already built against, so most existing
// tests need no changes at all; only the tests that care about a
// *different* real origin below set `host` explicitly.
const incomingHeaders = new Map<string, string>();
vi.mock('next/headers', () => ({
  headers: async () => ({
    get: (name: string) => incomingHeaders.get(name) ?? null,
  }),
}));

import { GET, POST } from './route';

// Typed off NextRequest's own constructor rather than the DOM lib's
// `RequestInit` -- Next's `RequestInit` (next/server, not re-exported
// publicly) narrows `signal` to `AbortSignal | undefined` (no `null`),
// which the DOM lib type allows, so `RequestInit` here didn't structurally
// match what `new NextRequest(...)` actually accepts.
function makeRequest(
  pathname: string,
  init?: ConstructorParameters<typeof NextRequest>[1] & { cookie?: string },
): NextRequest {
  const { cookie, ...rest } = init ?? {};
  const headers = new Headers(rest.headers);
  if (cookie) {
    headers.set('cookie', cookie);
  }
  return new NextRequest(`http://localhost:3000${pathname}`, { ...rest, headers });
}

describe('GET /connect-claude/authorize', () => {
  beforeEach(() => {
    vi.stubEnv('RAILMCP_BASE_URL', 'http://railmcp.internal:3000');
    vi.stubEnv('RAILMCP_INTERNAL_COMPLETE_TOKEN', 'internal-complete-token-for-tests');
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
    incomingHeaders.clear();
  });

  it('400s when mcp_request_id is missing', async () => {
    const req = makeRequest('/connect-claude/authorize');
    const res = await GET(req);
    expect(res.status).toBe(400);
  });

  // Regression for Finding 4 of the 2026-09-24 security review:
  // `mcp_request_id` used to reach the `railMcp`
  // `/internal/pending-authorization/{id}` URL (carrying
  // `X-Internal-Complete-Token`) completely unvalidated -- a crafted value
  // containing `../` could redirect that internal fetch to an
  // attacker-chosen path on the internal host.
  it('400s when mcp_request_id contains characters outside its allowed shape', async () => {
    const req = makeRequest('/connect-claude/authorize?mcp_request_id=' + encodeURIComponent('../evil'));
    const res = await GET(req);
    expect(res.status).toBe(400);
  });

  it('redirects to /api/auth/login with a correctly-encoded return_to when not logged in', async () => {
    const req = makeRequest('/connect-claude/authorize?mcp_request_id=req1');
    const res = await GET(req);
    expect(res.status).toBe(307);
    const location = res.headers.get('location')!;
    expect(location).toContain('/api/auth/login?return_to=');
    const returnTo = decodeURIComponent(new URL(location).searchParams.get('return_to')!);
    expect(returnTo).toBe('/connect-claude/authorize?mcp_request_id=req1');
  });

  // Regression for Finding 1/2 of the 2026-09-24 security review: this
  // redirect used to be built from `req.url`, which resolves against
  // `req.nextUrl.origin` -- always this app's bare bound host under plain
  // `next start` (effectively `http://localhost:3000`), never the real
  // public origin a reverse-proxied deployment is actually served from.
  // Every logged-out visitor's browser was sent to
  // `https://localhost:3000/api/auth/login?...` instead of the real host.
  it("redirects to the app's real public origin (from the Host header), not req.url's bare bound host", async () => {
    incomingHeaders.set('host', 'ds.cursed.solutions');
    incomingHeaders.set('x-forwarded-proto', 'https');
    const req = makeRequest('/connect-claude/authorize?mcp_request_id=req1');
    const res = await GET(req);
    expect(res.status).toBe(307);
    const location = res.headers.get('location')!;
    expect(location.startsWith('https://ds.cursed.solutions/api/auth/login?')).toBe(true);
  });

  it('renders a consent screen naming the requesting client when a session cookie is present', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ clientName: 'Claude' }), { status: 200 })),
    );
    const req = makeRequest('/connect-claude/authorize?mcp_request_id=req1', {
      cookie: 'distant_signal_session=raw-token-value',
    });
    const res = await GET(req);
    expect(res.status).toBe(200);
    const body = await res.text();
    expect(body).toContain('Claude');
    expect(body).toContain('req1');

    const [calledUrl, init] = vi.mocked(fetch).mock.calls[0];
    expect(calledUrl.toString()).toBe('http://railmcp.internal:3000/internal/pending-authorization/req1');
    expect((init as RequestInit & { headers: Record<string, string> }).headers['X-Internal-Complete-Token']).toBe(
      'internal-complete-token-for-tests',
    );
  });

  it('renders a consent screen with a generic label when the client registered no client_name', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({}), { status: 200 })));
    const req = makeRequest('/connect-claude/authorize?mcp_request_id=req1', {
      cookie: 'distant_signal_session=raw-token-value',
    });
    const res = await GET(req);
    expect(res.status).toBe(200);
    const body = await res.text();
    expect(body).toContain('An application');
  });

  it('returns 410 when the pending authorization has expired', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(null, { status: 404 })));
    const req = makeRequest('/connect-claude/authorize?mcp_request_id=req1', {
      cookie: 'distant_signal_session=raw-token-value',
    });
    const res = await GET(req);
    expect(res.status).toBe(410);
  });

  it('still renders the consent screen (without a client name) when the pending-authorization lookup itself fails', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new Error('network down');
      }),
    );
    const req = makeRequest('/connect-claude/authorize?mcp_request_id=req1', {
      cookie: 'distant_signal_session=raw-token-value',
    });
    const res = await GET(req);
    expect(res.status).toBe(200);
    const body = await res.text();
    expect(body).toContain('An application');
  });

  it('escapes an untrusted, self-reported client_name before interpolating it into HTML', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ clientName: '<script>alert(1)</script>' }), { status: 200 })),
    );
    const req = makeRequest('/connect-claude/authorize?mcp_request_id=req1', {
      cookie: 'distant_signal_session=raw-token-value',
    });
    const res = await GET(req);
    const body = await res.text();
    expect(body).not.toContain('<script>alert(1)</script>');
    expect(body).toContain('&lt;script&gt;');
  });
});

describe('POST /connect-claude/authorize', () => {
  beforeEach(() => {
    vi.stubEnv('RAILMCP_BASE_URL', 'http://railmcp.internal:3000');
    vi.stubEnv('RAILMCP_INTERNAL_COMPLETE_TOKEN', 'internal-complete-token-for-tests');
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
    incomingHeaders.clear();
  });

  function postRequest(
    mcpRequestId: string,
    decision: 'approve' | 'deny',
    cookie?: string,
    originHeaders: Record<string, string> = { origin: 'http://localhost:3000' },
  ): NextRequest {
    const form = new URLSearchParams({ decision });
    const headers = new Headers({ 'content-type': 'application/x-www-form-urlencoded', ...originHeaders });
    if (cookie) headers.set('cookie', cookie);
    return new NextRequest(`http://localhost:3000/connect-claude/authorize?mcp_request_id=${mcpRequestId}`, {
      method: 'POST',
      headers,
      body: form.toString(),
    });
  }

  it('400s without a session cookie', async () => {
    const req = postRequest('req1', 'approve');
    const res = await POST(req);
    expect(res.status).toBe(400);
  });

  it('403s a cross-site POST with a mismatched Origin, even with a valid session cookie', async () => {
    const req = postRequest('req1', 'approve', 'distant_signal_session=raw-token-value', {
      origin: 'https://evil.example.com',
    });
    const res = await POST(req);
    expect(res.status).toBe(403);
  });

  it('403s a POST with no Origin and a Referer on a different origin', async () => {
    const req = postRequest('req1', 'approve', 'distant_signal_session=raw-token-value', {
      referer: 'https://evil.example.com/attack',
    });
    const res = await POST(req);
    expect(res.status).toBe(403);
  });

  it('403s a POST with neither an Origin nor a Referer header', async () => {
    const req = postRequest('req1', 'approve', 'distant_signal_session=raw-token-value', {});
    const res = await POST(req);
    expect(res.status).toBe(403);
  });

  it('accepts a same-origin POST that carries Referer but no Origin', async () => {
    const fetchSpy = vi
      .fn()
      .mockResolvedValue(new Response(JSON.stringify({ redirectUrl: 'https://claude.ai/cb?code=abc&state=xyz' }), { status: 200 }));
    vi.stubGlobal('fetch', fetchSpy);

    const req = postRequest('req1', 'approve', 'distant_signal_session=raw-token-value', {
      referer: 'http://localhost:3000/connect-claude/authorize?mcp_request_id=req1',
    });
    const res = await POST(req);
    expect(res.status).toBe(307);
  });

  // Regression for Finding 1 of the 2026-09-24 security review, and the
  // actual production bug: this check used to compare `Origin` against
  // `req.nextUrl.origin`, which under plain `next start` (no explicit
  // host/`trustHostHeader` config) is always this app's bare bound host --
  // NOT the real public origin a reverse-proxied deployment is served
  // from. A real browser's `Origin: https://ds.cursed.solutions` never
  // matched `req.nextUrl.origin` (effectively `http://localhost:3000`),
  // so every genuine Approve/Deny POST was rejected with a 403. Here,
  // `req`'s own base URL stays `http://localhost:3000` (`postRequest`
  // above builds every request against it, same as the rest of this
  // suite) while the `Host`/`X-Forwarded-Proto` headers `getSiteOrigin()`
  // reads say the real origin is `https://ds.cursed.solutions` -- exactly
  // the shape a reverse-proxied production request actually has. A POST
  // whose Origin matches that REAL origin must be accepted, not rejected.
  it('accepts a same-origin POST whose real public origin (Host header) differs from req.nextUrl.origin', async () => {
    incomingHeaders.set('host', 'ds.cursed.solutions');
    incomingHeaders.set('x-forwarded-proto', 'https');
    const fetchSpy = vi
      .fn()
      .mockResolvedValue(new Response(JSON.stringify({ redirectUrl: 'https://claude.ai/cb?code=abc&state=xyz' }), { status: 200 }));
    vi.stubGlobal('fetch', fetchSpy);

    const req = postRequest('req1', 'approve', 'distant_signal_session=raw-token-value', {
      origin: 'https://ds.cursed.solutions',
    });
    const res = await POST(req);
    expect(res.status).toBe(307);
  });

  it('403s a POST whose Origin matches req.nextUrl.origin but not the real public origin', async () => {
    incomingHeaders.set('host', 'ds.cursed.solutions');
    incomingHeaders.set('x-forwarded-proto', 'https');
    const req = postRequest('req1', 'approve', 'distant_signal_session=raw-token-value', {
      origin: 'http://localhost:3000',
    });
    const res = await POST(req);
    expect(res.status).toBe(403);
  });

  // Regression for Finding 4: same shape check GET already carries,
  // applied to POST's own read of the same query param.
  it('400s when mcp_request_id contains characters outside its allowed shape', async () => {
    const req = new NextRequest(
      `http://localhost:3000/connect-claude/authorize?mcp_request_id=${encodeURIComponent('../evil')}`,
      {
        method: 'POST',
        headers: new Headers({
          'content-type': 'application/x-www-form-urlencoded',
          origin: 'http://localhost:3000',
          cookie: 'distant_signal_session=raw-token-value',
        }),
        body: new URLSearchParams({ decision: 'approve' }).toString(),
      },
    );
    const res = await POST(req);
    expect(res.status).toBe(400);
  });

  it('on approval, forwards the RAW session cookie value to /internal/complete-authorization and redirects to the returned URL', async () => {
    const fetchSpy = vi
      .fn()
      .mockResolvedValue(new Response(JSON.stringify({ redirectUrl: 'https://claude.ai/cb?code=abc&state=xyz' }), { status: 200 }));
    vi.stubGlobal('fetch', fetchSpy);

    const req = postRequest('req1', 'approve', 'distant_signal_session=raw-token-value');
    const res = await POST(req);
    expect(res.status).toBe(307);
    expect(res.headers.get('location')).toBe('https://claude.ai/cb?code=abc&state=xyz');

    const [calledUrl, init] = fetchSpy.mock.calls[0];
    expect(calledUrl.toString()).toBe('http://railmcp.internal:3000/internal/complete-authorization');
    expect(JSON.parse((init as RequestInit).body as string)).toEqual({
      mcp_request_id: 'req1',
      ds_session_cookie_value: 'raw-token-value',
    });
  });

  it('on denial, calls deny-authorization instead of complete-authorization, and never sends the session cookie value', async () => {
    const fetchSpy = vi
      .fn()
      .mockResolvedValue(new Response(JSON.stringify({ redirectUrl: 'https://claude.ai/cb?error=access_denied&state=xyz' }), { status: 200 }));
    vi.stubGlobal('fetch', fetchSpy);

    const req = postRequest('req2', 'deny', 'distant_signal_session=raw-token-value');
    const res = await POST(req);
    expect(res.status).toBe(307);

    const [calledUrl, init] = fetchSpy.mock.calls[0];
    expect(calledUrl.toString()).toContain('/internal/deny-authorization');
    expect(JSON.parse((init as RequestInit).body as string)).toEqual({ mcp_request_id: 'req2' });
  });

  it('502s when the adapter fails to complete the exchange', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 500 })));
    const req = postRequest('req1', 'approve', 'distant_signal_session=raw-token-value');
    const res = await POST(req);
    expect(res.status).toBe(502);
  });
});
