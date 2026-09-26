import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { NextRequest } from 'next/server';

// `getSiteOrigin()` (lib/siteOrigin.ts) -- used by this proxy's own new
// Origin check on mutating methods -- reads `next/headers` when
// `NEXT_PUBLIC_SITE_URL` isn't set. There is no Next request context in a
// unit test (same stub shape `app/connect-claude/authorize/route.test.ts`'s
// own `next/headers` mock uses). Left empty by default: with no `host`
// header, `getSiteOrigin()` falls back to `http://localhost:3000` --
// exactly the origin every request below is already built against, so
// every pre-existing test needs no changes; only the tests below that care
// about a *different* real origin set `host` explicitly.
const incomingHeaders = new Map<string, string>();
vi.mock('next/headers', () => ({
  headers: async () => ({
    get: (name: string) => incomingHeaders.get(name) ?? null,
  }),
}));

import { GET, POST, PUT, DELETE } from './route';

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
    incomingHeaders.clear();
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

  // Regression: api's own strict same-origin check on POST /auth/logout
  // (2026-09-25 Low-severity auth-core review) reads Origin/Referer off
  // whatever request IT receives -- this proxy's own server-side fetch,
  // not the browser's original request -- and fails closed (403) when
  // BOTH are absent. Node's fetch doesn't fabricate either header the way
  // a browser does, so without forwarding them explicitly, every single
  // proxied request looked origin-less to api regardless of what the
  // browser actually sent, and every real logout was rejected.
  it('forwards the browser\'s Origin and Referer headers through to the backend', async () => {
    const req = makeRequest('/api/auth/logout', {
      method: 'POST',
      headers: {
        // Matches makeRequest's own base URL -- this app's real public
        // origin as far as hasAcceptableOriginForMutation is concerned in
        // this test file's default (no `host` header) setup, same as
        // every other passing Origin-check test above.
        origin: 'http://localhost:3000',
        referer: 'http://localhost:3000/settings',
        cookie: 'nr_session=abc123',
      },
    });
    await POST(req, { params: Promise.resolve({ path: ['auth', 'logout'] }) });
    const [calledUrl, init] = vi.mocked(fetch).mock.calls[0];
    expect(calledUrl.toString()).toBe('http://test-api:8080/public/auth/logout');
    const headers = (init as { headers: Record<string, string> }).headers;
    expect(headers.Origin).toBe('http://localhost:3000');
    expect(headers.Referer).toBe('http://localhost:3000/settings');
  });

  it('omits Origin/Referer from the outbound fetch when the browser sent neither', async () => {
    const req = makeRequest('/api/auth/logout', { method: 'POST' });
    await POST(req, { params: Promise.resolve({ path: ['auth', 'logout'] }) });
    const [, init] = vi.mocked(fetch).mock.calls[0];
    const headers = (init as { headers: Record<string, string> }).headers;
    expect(headers.Origin).toBeUndefined();
    expect(headers.Referer).toBeUndefined();
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

  it('forwards a bare POST /api/JourneyTemplates (no trailing path) to the bare-root backend /JourneyTemplates path', async () => {
    // Regression for the final-review Critical finding on
    // docs/superpowers/plans/2026-09-22-reusable-journeys-phaseB-durable-templates-plan.md:
    // ROOT_MOUNTED_PREFIXES was never widened for the new
    // `routes::journey_templates::router()` (also `.merge`d onto the
    // backend's root router, immediately after `routes::journeys::router()`
    // in `crates/api/src/main.rs`, not nested under `/public`), so every
    // browser-side write in the templates feature (save-as-template, edit,
    // delete, run-now) resolved to the non-existent
    // `/public/JourneyTemplates...` and 404ed. Like `POST /api/Journeys`,
    // this resolves to the BARE `/JourneyTemplates` path with no trailing
    // segment.
    const req = makeRequest('/api/JourneyTemplates', {
      method: 'POST',
      headers: { cookie: 'nr_session=abc123' },
      body: JSON.stringify({ customName: 'Commute', legs: [] }),
    });
    await POST(req, { params: Promise.resolve({ path: ['JourneyTemplates'] }) });
    const [calledUrl, init] = vi.mocked(fetch).mock.calls[0];
    expect(calledUrl.toString()).toBe('http://test-api:8080/JourneyTemplates');
    expect((init as RequestInit).method).toBe('POST');
  });

  it('forwards a PUT /api/JourneyTemplates/1 to the bare-root backend path', async () => {
    const req = makeRequest('/api/JourneyTemplates/1', {
      method: 'PUT',
      headers: { cookie: 'nr_session=abc123' },
      body: JSON.stringify({ customName: 'Commute', legs: [] }),
    });
    await PUT(req, { params: Promise.resolve({ path: ['JourneyTemplates', '1'] }) });
    const [calledUrl, init] = vi.mocked(fetch).mock.calls[0];
    expect(calledUrl.toString()).toBe('http://test-api:8080/JourneyTemplates/1');
    expect((init as RequestInit).method).toBe('PUT');
  });

  it('forwards a POST /api/JourneyTemplates/1/materialize to the bare-root backend path', async () => {
    const req = makeRequest('/api/JourneyTemplates/1/materialize', {
      method: 'POST',
      headers: { cookie: 'nr_session=abc123' },
      body: JSON.stringify({ serviceDate: '2026-09-23' }),
    });
    await POST(req, { params: Promise.resolve({ path: ['JourneyTemplates', '1', 'materialize'] }) });
    const [calledUrl] = vi.mocked(fetch).mock.calls[0];
    expect(calledUrl.toString()).toBe('http://test-api:8080/JourneyTemplates/1/materialize');
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

  // Finding 5 of the 2026-09-24 security review: every browser mutation
  // through this app (creating a group, promoting a member, generating a
  // share link, logging out -- roughly 50 call sites) relied solely on the
  // backend's `SameSite=Lax` session cookie for CSRF protection, with this
  // proxy doing no Origin verification of its own before forwarding a
  // POST/PUT/DELETE. Mirrors the Origin check
  // `app/connect-claude/authorize/route.ts` already carries for its own
  // single state-changing POST, applied here at the shared-proxy level.
  describe('Origin check on mutating methods', () => {
    it('403s a POST whose Origin does not match this app\'s real public origin', async () => {
      const req = makeRequest('/api/preferences', {
        method: 'POST',
        headers: { 'content-type': 'application/json', origin: 'https://evil.example.com' },
        body: '{}',
      });
      const res = await POST(req, { params: Promise.resolve({ path: ['preferences'] }) });
      expect(res.status).toBe(403);
      expect(fetch).not.toHaveBeenCalled();
    });

    it('403s a PUT whose Origin does not match', async () => {
      const req = makeRequest('/api/preferences/pinned-lines', {
        method: 'PUT',
        headers: { 'content-type': 'application/json', origin: 'https://evil.example.com' },
        body: '[]',
      });
      const res = await PUT(req, { params: Promise.resolve({ path: ['preferences', 'pinned-lines'] }) });
      expect(res.status).toBe(403);
      expect(fetch).not.toHaveBeenCalled();
    });

    it('403s a DELETE whose Origin does not match', async () => {
      const req = makeRequest('/api/Train/1', {
        method: 'DELETE',
        headers: { origin: 'https://evil.example.com' },
      });
      const res = await DELETE(req, { params: Promise.resolve({ path: ['Train', '1'] }) });
      expect(res.status).toBe(403);
      expect(fetch).not.toHaveBeenCalled();
    });

    it('accepts a POST whose Origin matches this app\'s real public origin', async () => {
      const req = makeRequest('/api/preferences', {
        method: 'POST',
        headers: { 'content-type': 'application/json', origin: 'http://localhost:3000' },
        body: '{}',
      });
      const res = await POST(req, { params: Promise.resolve({ path: ['preferences'] }) });
      expect(res.status).toBe(200);
      expect(fetch).toHaveBeenCalled();
    });

    // Not every legitimate same-origin request sends an Origin header
    // (and this proxy has no CSRF-token convention to fall back on to
    // require one) -- see `hasAcceptableOriginForMutation`'s own doc
    // comment. Also the exact regression shape every pre-existing
    // POST/PUT/DELETE test above already relies on (none of them set an
    // Origin header at all).
    it('accepts a POST with no Origin header at all', async () => {
      const req = makeRequest('/api/preferences', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: '{}',
      });
      const res = await POST(req, { params: Promise.resolve({ path: ['preferences'] }) });
      expect(res.status).toBe(200);
      expect(fetch).toHaveBeenCalled();
    });

    // A GET is never subject to this check at all, matching header,
    // mismatched header, or none.
    it('never blocks a GET, regardless of Origin', async () => {
      const req = makeRequest('/api/preferences', {
        headers: { origin: 'https://evil.example.com' },
      });
      const res = await GET(req, { params: Promise.resolve({ path: ['preferences'] }) });
      expect(res.status).toBe(200);
    });

    // Regression for the same underlying bug Finding 1 describes for
    // `connect-claude/authorize/route.ts`: this app's real public origin
    // (from the `Host`/`X-Forwarded-Proto` headers `getSiteOrigin()`
    // reads) can legitimately differ from `req.nextUrl.origin` (this
    // app's bare bound host under plain `next start`) -- a mutating
    // request whose Origin matches the REAL origin must be accepted even
    // though it does not match `req.nextUrl.origin`.
    it("accepts a POST whose real public origin (Host header) differs from req.nextUrl.origin", async () => {
      incomingHeaders.set('host', 'ds.cursed.solutions');
      incomingHeaders.set('x-forwarded-proto', 'https');
      const req = makeRequest('/api/preferences', {
        method: 'POST',
        headers: { 'content-type': 'application/json', origin: 'https://ds.cursed.solutions' },
        body: '{}',
      });
      const res = await POST(req, { params: Promise.resolve({ path: ['preferences'] }) });
      expect(res.status).toBe(200);
      expect(fetch).toHaveBeenCalled();
    });

    it('403s a POST whose Origin matches req.nextUrl.origin but not the real public origin', async () => {
      incomingHeaders.set('host', 'ds.cursed.solutions');
      incomingHeaders.set('x-forwarded-proto', 'https');
      const req = makeRequest('/api/preferences', {
        method: 'POST',
        headers: { 'content-type': 'application/json', origin: 'http://localhost:3000' },
        body: '{}',
      });
      const res = await POST(req, { params: Promise.resolve({ path: ['preferences'] }) });
      expect(res.status).toBe(403);
      expect(fetch).not.toHaveBeenCalled();
    });
  });

  // Finding 2 of the deferred fapp Low-severity batch (2026-09-24 security
  // review): Next.js decodes each catch-all segment before populating
  // `path`, so a segment can carry a *decoded* `#`/`?`/`/` by the time
  // `resolveTargetPath` rejoins it into a template-string URL. Rejoining
  // raw (pre-fix) let a decoded `#`/`?` re-assert itself as a literal
  // fragment/query separator once `new URL(...)` parsed the result,
  // silently truncating the intended pathname.
  describe('segment re-encoding on rejoin', () => {
    it('preserves a decoded "#" in a segment as a literal path character, not a fragment separator', async () => {
      const req = makeRequest('/api/Train/placeholder');
      await GET(req, { params: Promise.resolve({ path: ['Train', 'abc#def'] }) });
      const [calledUrl] = vi.mocked(fetch).mock.calls[0];
      // Pre-fix, the rejoined `/Train/abc#def` was parsed by `new URL()`
      // with `#def` as a fragment -- dropped entirely off the wire -- so
      // the backend would have received `/Train/abc` instead.
      expect(calledUrl.toString()).toBe('http://test-api:8080/Train/abc%23def');
    });

    it('preserves a decoded "?" in a segment as a literal path character, not a query separator', async () => {
      const req = makeRequest('/api/Train/placeholder');
      await GET(req, { params: Promise.resolve({ path: ['Train', 'abc?evil=1'] }) });
      const [calledUrl] = vi.mocked(fetch).mock.calls[0];
      // Pre-fix, `?evil=1` would have been parsed as the query string
      // instead of part of the path, resolving to pathname `/Train/abc`
      // with an attacker-controlled query string appended.
      // `encodeURIComponent` also escapes `=` (it isn't in the unreserved
      // set), so the whole trailing segment comes out escaped, not just
      // the `?`.
      expect(calledUrl.toString()).toBe('http://test-api:8080/Train/abc%3Fevil%3D1');
    });

    it('preserves a decoded "/" (from an embedded %2F) in a segment as a literal path character, not a new path separator', async () => {
      const req = makeRequest('/api/Train/placeholder');
      await GET(req, { params: Promise.resolve({ path: ['Train', 'abc/def'] }) });
      const [calledUrl] = vi.mocked(fetch).mock.calls[0];
      expect(calledUrl.toString()).toBe('http://test-api:8080/Train/abc%2Fdef');
    });
  });
});
