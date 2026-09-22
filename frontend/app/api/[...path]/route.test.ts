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

  // Typed off NextRequest's own constructor rather than the DOM lib's
  // `RequestInit` -- Next's `RequestInit` (next/server, not re-exported
  // publicly) narrows `signal` to `AbortSignal | undefined` (no `null`),
  // which the DOM lib type allows, so `RequestInit` here didn't structurally
  // match what `new NextRequest(...)` actually accepts.
  function makeRequest(pathname: string, init?: ConstructorParameters<typeof NextRequest>[1]): NextRequest {
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

  it('a path outside public/, Train/, and Journeys/ still 400s', async () => {
    // Not reachable through this app's own links today (every catch-all
    // segment this app generates comes from a literal string, never raw
    // user text) -- this is the traversal-safety net Decision 4 said
    // stays "unchanged in kind"; confirm it still rejects a resolved path
    // outside the widened three-prefix allowlist, not just the original
    // single-prefix one.
    const req = makeRequest('/api/../secret');
    const response = await GET(req, { params: Promise.resolve({ path: ['..', 'secret'] }) });
    expect(response.status).toBe(400);
  });

  it('forwards a bare POST /api/Journeys (no trailing path) to the bare-root backend /Journeys path', async () => {
    // Regression for Finding C1: `resolveTargetPath` used to only
    // special-case `path[0] === 'Train'`, so this resolved to the
    // non-existent `/public/Journeys` on the backend -- every
    // `POST /Journeys` call this app's frontend makes (creating a
    // journey/leg) would 404. `routes::journeys::router()` is `.merge`d
    // onto the backend's root router exactly like `routes::train::router()`
    // (`crates/api/src/main.rs`), not nested under `/public`. Unlike every
    // `/Train/...` call this proxy forwards, this resolves to the BARE
    // `/Journeys` path with no trailing segment at all, so this also
    // exercises the guard's "no trailing slash required" case.
    const req = makeRequest('/api/Journeys', {
      method: 'POST',
      headers: { cookie: 'nr_session=abc123' },
      body: JSON.stringify({ leg: { mode: 'pin' } }),
    });
    await POST(req, { params: Promise.resolve({ path: ['Journeys'] }) });
    const [calledUrl, init] = vi.mocked(fetch).mock.calls[0];
    expect(calledUrl.toString()).toBe('http://test-api:8080/Journeys');
    expect((init as RequestInit).method).toBe('POST');
  });

  it('forwards a GET /api/Journeys/mine to the bare-root backend path', async () => {
    const req = makeRequest('/api/Journeys/mine');
    await GET(req, { params: Promise.resolve({ path: ['Journeys', 'mine'] }) });
    const [calledUrl] = vi.mocked(fetch).mock.calls[0];
    expect(calledUrl.toString()).toBe('http://test-api:8080/Journeys/mine');
  });

  it('forwards a GET /api/Journeys/1/legs/2/candidates to the bare-root backend path', async () => {
    const req = makeRequest('/api/Journeys/1/legs/2/candidates');
    await GET(req, { params: Promise.resolve({ path: ['Journeys', '1', 'legs', '2', 'candidates'] }) });
    const [calledUrl] = vi.mocked(fetch).mock.calls[0];
    expect(calledUrl.toString()).toBe('http://test-api:8080/Journeys/1/legs/2/candidates');
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
