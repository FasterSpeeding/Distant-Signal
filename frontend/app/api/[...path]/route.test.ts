import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { NextRequest } from 'next/server';
import { GET, POST } from './route';

describe('/api/[...path] proxy', () => {
  beforeEach(() => {
    vi.stubEnv('API_BASE_URL', 'http://test-api:8080');
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(JSON.stringify({ ok: true }), {
            status: 200,
            headers: { 'Content-Type': 'application/json' },
          }),
      ),
    );
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  function makeRequest(pathname: string, init?: RequestInit): NextRequest {
    return new NextRequest(`http://localhost:3000${pathname}`, init);
  }

  it('still forwards an existing /public-scoped route unchanged (regression)', async () => {
    const req = makeRequest('/api/preferences');
    await GET(req, { params: Promise.resolve({ path: ['preferences'] }) });
    const [calledUrl] = vi.mocked(fetch).mock.calls[0];
    expect(calledUrl.toString()).toBe('http://test-api:8080/public/preferences');
  });

  it('forwards a Train/track POST to the bare-root backend path, with cookies attached', async () => {
    const req = makeRequest('/api/Train/track', {
      method: 'POST',
      headers: { cookie: 'nr_session=abc123' },
      body: JSON.stringify({ origin_crs: 'WAT' }),
    });
    await POST(req, { params: Promise.resolve({ path: ['Train', 'track'] }) });
    const [calledUrl, init] = vi.mocked(fetch).mock.calls[0];
    expect(calledUrl.toString()).toBe('http://test-api:8080/Train/track');
    expect((init as RequestInit).method).toBe('POST');
    expect((init as { headers: Record<string, string> }).headers.Cookie).toBe('nr_session=abc123');
  });

  it('a path outside both public/ and Train/ still 400s', async () => {
    // Not reachable through this app's own links today (every catch-all
    // segment this app generates comes from a literal string, never raw
    // user text) -- this is the traversal-safety net Decision 4 said
    // stays "unchanged in kind"; confirm it still rejects a resolved path
    // outside the widened two-prefix allowlist, not just the original
    // single-prefix one.
    const req = makeRequest('/api/../secret');
    const response = await GET(req, { params: Promise.resolve({ path: ['..', 'secret'] }) });
    expect(response.status).toBe(400);
  });
});
