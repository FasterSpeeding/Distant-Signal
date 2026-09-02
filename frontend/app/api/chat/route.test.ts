import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { NextRequest } from 'next/server';
import { POST } from './route';

describe('/api/chat proxy', () => {
  beforeEach(() => {
    vi.stubEnv('ORCHESTRATOR_BASE_URL', 'http://test-orchestrator:3001');
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  function makeRequest(init?: ConstructorParameters<typeof NextRequest>[1]): NextRequest {
    return new NextRequest('http://localhost:3000/api/chat', init);
  }

  it('forwards the Cookie header and JSON body to POST {ORCHESTRATOR_BASE_URL}/chat', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(null, { status: 200, headers: { 'Content-Type': 'text/event-stream' } })),
    );
    const req = makeRequest({
      method: 'POST',
      headers: { cookie: 'distant_signal_session=abc123', 'content-type': 'application/json' },
      body: JSON.stringify({ message: 'when is the next train from KGX?' }),
    });

    await POST(req);

    const [calledUrl, init] = vi.mocked(fetch).mock.calls[0];
    expect(calledUrl.toString()).toBe('http://test-orchestrator:3001/chat');
    const forwardedInit = init as RequestInit & { headers: Record<string, string> };
    expect(forwardedInit.method).toBe('POST');
    expect(forwardedInit.headers.Cookie).toBe('distant_signal_session=abc123');
    expect(forwardedInit.headers['Content-Type']).toBe('application/json');
    expect(forwardedInit.body).toBe(JSON.stringify({ message: 'when is the next train from KGX?' }));
  });

  it('forwards a missing Cookie header as an empty string, not a crash', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(null, { status: 401 })));
    const req = makeRequest({ method: 'POST', body: JSON.stringify({ message: 'hi' }) });

    await POST(req);

    const [, init] = vi.mocked(fetch).mock.calls[0];
    const forwardedInit = init as RequestInit & { headers: Record<string, string> };
    expect(forwardedInit.headers.Cookie).toBe('');
  });

  it('passes a 401 from the orchestrator through with its status and body intact', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(JSON.stringify({ error: 'unauthenticated' }), {
            status: 401,
            headers: { 'Content-Type': 'application/json' },
          }),
      ),
    );
    const req = makeRequest({ method: 'POST', body: JSON.stringify({ message: 'hi' }) });

    const response = await POST(req);

    expect(response.status).toBe(401);
    expect(await response.json()).toEqual({ error: 'unauthenticated' });
  });

  it('passes a 403 from the orchestrator through with its status and body intact', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(JSON.stringify({ error: 'chatbot_not_available' }), {
            status: 403,
            headers: { 'Content-Type': 'application/json' },
          }),
      ),
    );
    const req = makeRequest({ method: 'POST', body: JSON.stringify({ message: 'hi' }) });

    const response = await POST(req);

    expect(response.status).toBe(403);
    expect(await response.json()).toEqual({ error: 'chatbot_not_available' });
  });

  it('a 200 SSE response streams the same body object through unmodified, not consumed/re-read', async () => {
    const upstreamBody = new ReadableStream({
      start(controller) {
        controller.enqueue(new TextEncoder().encode('data: {"type":"text-delta","text":"hi"}\n\n'));
        controller.close();
      },
    });
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(upstreamBody, { status: 200, headers: { 'Content-Type': 'text/event-stream' } }),
      ),
    );
    const req = makeRequest({ method: 'POST', body: JSON.stringify({ message: 'hi' }) });

    const response = await POST(req);

    expect(response.status).toBe(200);
    expect(response.headers.get('Content-Type')).toBe('text/event-stream');
    expect(response.body).toBe(upstreamBody);
  });

  it('throws when ORCHESTRATOR_BASE_URL is not set', async () => {
    vi.unstubAllEnvs();
    const req = makeRequest({ method: 'POST', body: JSON.stringify({ message: 'hi' }) });
    await expect(POST(req)).rejects.toThrow(/ORCHESTRATOR_BASE_URL/);
  });
});
