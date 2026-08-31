import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { NextRequest } from 'next/server';
import { GET, POST, PUT } from './route';

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

  it('forwards a multipart/form-data upload with its original Content-Type (boundary intact)', async () => {
    const boundary = '----testboundary123';
    const req = makeRequest('/api/Train/1/tickets/pkpass', {
      method: 'POST',
      headers: {
        cookie: 'nr_session=abc123',
        'content-type': `multipart/form-data; boundary=${boundary}`,
      },
      body: `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="t.pkpass"\r\n\r\nfake-bytes\r\n--${boundary}--`,
    });
    await POST(req, { params: Promise.resolve({ path: ['Train', '1', 'tickets', 'pkpass'] }) });
    const [, init] = vi.mocked(fetch).mock.calls[0];
    const forwardedHeaders = (init as { headers: Record<string, string> }).headers;
    expect(forwardedHeaders['Content-Type']).toBe(`multipart/form-data; boundary=${boundary}`);
  });

  it('forwards binary body bytes unchanged (does not lossily decode as UTF-8 text)', async () => {
    // A byte sequence that is invalid UTF-8 on its own (0xff is never a
    // valid standalone UTF-8 byte) -- .text() would have replaced it with
    // U+FFFD before this test could ever observe the original bytes;
    // arrayBuffer() must not.
    const rawBytes = new Uint8Array([0x50, 0x4b, 0x03, 0x04, 0xff, 0x00, 0x89]);
    const req = new NextRequest('http://localhost:3000/api/Train/1/tickets/pkpass', {
      method: 'POST',
      headers: { 'content-type': 'application/octet-stream' },
      body: rawBytes,
    });
    await POST(req, { params: Promise.resolve({ path: ['Train', '1', 'tickets', 'pkpass'] }) });
    const [, init] = vi.mocked(fetch).mock.calls[0];
    const forwardedBody = new Uint8Array((init as { body: ArrayBuffer }).body);
    expect(Array.from(forwardedBody)).toEqual(Array.from(rawBytes));
  });

  it('still forwards a JSON body byte-identically (regression: existing callers unaffected)', async () => {
    const req = makeRequest('/api/preferences/pinned-lines', {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(['wcml']),
    });
    await PUT(req, { params: Promise.resolve({ path: ['preferences', 'pinned-lines'] }) });
    const [, init] = vi.mocked(fetch).mock.calls[0];
    const forwardedHeaders = (init as { headers: Record<string, string> }).headers;
    const forwardedBody = new TextDecoder().decode((init as { body: ArrayBuffer }).body);
    expect(forwardedHeaders['Content-Type']).toBe('application/json');
    expect(forwardedBody).toBe(JSON.stringify(['wcml']));
  });
});
