import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { NextRequest } from 'next/server';
import { GET, POST } from './route';

function makeRequest(pathname: string, init?: RequestInit & { cookie?: string }): NextRequest {
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
  });

  it('400s when mcp_request_id is missing', async () => {
    const req = makeRequest('/connect-claude/authorize');
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
