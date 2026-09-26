import { NextRequest, NextResponse } from 'next/server';
import { getSiteOrigin } from '@/lib/siteOrigin';

// Client Components can't read `API_BASE_URL` (server-only env var, not
// inlined into the browser bundle unless prefixed `NEXT_PUBLIC_`), so
// browser-initiated mutations (pinning, creating a custom line) can't call
// the `api` service directly. This catch-all proxies same-origin `/api/*`
// requests from the browser to `${API_BASE_URL}/public/*` server-side —
// since the browser only ever talks to this Next.js origin, no CORS
// relaxation on the `api` service is needed for these write endpoints.
//
// The OIDC login flow (`/auth/login`, `/auth/callback`) also runs through
// this proxy, which adds two more requirements beyond a plain body/status
// passthrough: the session/login-state cookies named by
// `FORWARDED_COOKIE_NAMES` below must reach `api` (so `/auth/session` and
// `/auth/callback` can read whichever of the two the browser is holding --
// NOT the whole incoming `Cookie` header verbatim; see that constant's own
// doc comment for why only these two, added for the 2026-09-26 "Repeater
// Signal" review's finding L5), and every `Set-Cookie` `api` sends back must
// reach the browser unmodified (so it can store the session cookie
// `/auth/callback` and `/auth/logout` set). `/auth/login` and
// `/auth/callback` also respond with `3xx` redirects that must be handed
// back to the *browser* to follow — the browser has to be the one that
// hits the SSO server's authorization endpoint, using its own
// cookies/session with that server, rather than have this Next.js server's
// own `fetch` call follow the redirect transparently and hand the browser
// whatever the final destination returned instead. `redirect: 'manual'`
// below disables `fetch`'s default auto-follow so those redirects (and
// their `Set-Cookie`s) can be forwarded as-is.
// Backend prefixes this proxy is allowed to reach. `/public/...` is the
// existing, general-purpose authenticated-mutation scope (preferences,
// custom lines, auth). `/Train/...` was added for individual train
// tracking (`POST /Train/track`,
// docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md
// Decision 4) -- that route is mounted directly on the backend's root
// router (`crates/api/src/main.rs`'s `.merge(routes::train::router())`),
// not nested under `/public`, the same way `/StopPoint/...`/`/Line/...`
// aren't. `/Journeys/...` was added the same way for journey tracking
// (`POST /Journeys`, `GET /Journeys/mine`, `GET /Journeys/{id}`,
// `GET /Journeys/{id}/legs/{id}/candidates`,
// `POST /Journeys/{id}/legs/{id}/train`,
// docs/superpowers/plans/2026-09-22-journey-tracking-phase1-single-leg-migration-plan.md)
// -- `routes::journeys::router()` is likewise `.merge`d onto the backend's
// root router in `crates/api/src/main.rs`, not nested under `/public`.
// `/JourneyTemplates/...` was added the same way again for durable journey
// templates (`POST /JourneyTemplates`, `GET /JourneyTemplates/mine`,
// `GET`/`PUT`/`DELETE /JourneyTemplates/{id}`,
// `POST /JourneyTemplates/{id}/materialize`,
// docs/superpowers/plans/2026-09-22-reusable-journeys-phaseB-durable-templates-plan.md)
// -- `routes::journey_templates::router()` is `.merge`d onto the backend's
// root router in `crates/api/src/main.rs` immediately after
// `routes::journeys::router()`, not nested under `/public`. `/Trips/...` was
// added the same way for dynamic trip planning (`GET /Trips/plan`,
// docs/superpowers/plans/2026-09-22-dynamic-trip-planning-phase6-frontend-integration-plan.md)
// -- `routes::trips::router()` is `.merge`d onto the backend's root router in
// `crates/api/src/main.rs` immediately after `routes::journey_templates::router()`,
// not nested under `/public`. Each prefix
// maps to how the *backend* path is actually built: everything else still
// gets `/public/` prepended (unchanged from before this list existed); a
// `Train/...`, `Journeys/...`, or `JourneyTemplates/...` request is passed
// straight through with no prefix inserted, since the backend already
// expects it bare.
const ROOT_MOUNTED_PREFIXES = new Set(['Train', 'Journeys', 'JourneyTemplates', 'Trips']);

// The two cookies this proxy's own backend routes can ever need --
// `distant_signal_session` (`SESSION_COOKIE_NAME`, matching
// `crates/api/src/auth.rs` and `lib/api.ts`'s own copy of this constant) for
// every ordinary authenticated request, and `distant_signal_login`
// (`LOGIN_STATE_COOKIE_NAME`, `crates/api/src/auth.rs`) for `GET
// /auth/callback` alone, which reads it to look up the PKCE verifier/nonce/
// csrf_state `GET /auth/login` stored server-side under it moments earlier.
//
// Forwarding the whole incoming `Cookie` header verbatim (this proxy's own
// shape until the 2026-09-26 "Repeater Signal" review, finding L5) shipped
// every OTHER cookie this origin might ever hold -- an analytics/consent
// cookie a future feature adds, say -- to the backend's own separate origin
// unconditionally, on every one of the ~50 call sites through this proxy.
// `lib/api.ts`'s own `cookieForwardInit` was already narrowed to just the
// session cookie for exactly this reason (Signal Box Audit, flib Low
// finding); this mirrors that fix here, widened by the one extra cookie this
// proxy alone needs that no SSR `fetch` in `lib/api.ts` ever does (no page
// render ever needs the short-lived, mid-login-flow state cookie).
const SESSION_COOKIE_NAME = 'distant_signal_session';
const LOGIN_STATE_COOKIE_NAME = 'distant_signal_login';
const FORWARDED_COOKIE_NAMES: readonly string[] = [SESSION_COOKIE_NAME, LOGIN_STATE_COOKIE_NAME];

/** Rebuilds a `Cookie` header value carrying only `FORWARDED_COOKIE_NAMES`
 * out of whatever the browser's own request carried -- see that constant's
 * own doc comment for why. Returns `null` (no `Cookie` header at all) when
 * neither is present, matching `cookieForwardInit`'s own "omit the header
 * entirely" shape in `lib/api.ts`.
 *
 * A plain manual split rather than a cookie-parsing dependency: a cookie
 * pair's name can't itself contain `;` or `=` (RFC 6265 §4.1.1's `cookie-av`
 * grammar), so splitting on `;` and then the FIRST `=` is exact for
 * extracting a pair's name, never a best-effort heuristic -- the value may
 * itself validly contain further `=` characters (e.g. base64url padding-free
 * output never does, but this doesn't assume that), which `slice(eq + 1)`
 * preserves whole rather than also splitting on. */
function forwardedCookieHeader(rawCookieHeader: string | null): string | null {
  if (!rawCookieHeader) return null;
  const forwarded: string[] = [];
  for (const pair of rawCookieHeader.split(';')) {
    const eq = pair.indexOf('=');
    if (eq === -1) continue;
    const name = pair.slice(0, eq).trim();
    if (FORWARDED_COOKIE_NAMES.includes(name)) {
      forwarded.push(`${name}=${pair.slice(eq + 1).trim()}`);
    }
  }
  return forwarded.length > 0 ? forwarded.join('; ') : null;
}

// Next.js decodes each catch-all segment before populating `path`, so a
// segment can legitimately contain a *decoded* `#`, `?`, or `/` (from an
// incoming `%23`, `%3F`, or `%2F`) by the time it gets here. Joining the raw
// segments with `/` and splicing that straight into a template-string URL
// (below) let a decoded `#`/`?` re-assert itself as a literal fragment/query
// separator once `new URL(...)` parsed the rejoined string -- silently
// truncating the intended pathname and turning the remainder into a
// fragment (dropped entirely) or query string, so the request could reach a
// different backend path/query than the one the segments actually named.
// Re-encoding each segment with `encodeURIComponent` before rejoining
// guarantees a decoded special character stays a literal, inert path
// character (e.g. `%23`) in the final URL instead of being reinterpreted as
// a structural separator.
function resolveTargetPath(path: string[]): string {
  const encoded = path.map(encodeURIComponent).join('/');
  return ROOT_MOUNTED_PREFIXES.has(path[0]) ? `/${encoded}` : `/public/${encoded}`;
}

/** Every browser-initiated mutation this app makes (creating a group,
 * promoting a member, generating a share link, logging out, roughly 50 call
 * sites across the component tree) goes through this one proxy, and relies
 * on nothing but the backend's `SameSite=Lax` session cookie for CSRF
 * protection -- this proxy itself forwarded a POST/PUT/DELETE with no
 * origin verification of its own at all. `SameSite=Lax` alone is sound
 * against a fully cross-site attacker (the same reasoning
 * `crates/api/src/main.rs`'s CORS comment relies on), but not against a
 * same-site sibling subdomain or a future XSS that can issue same-site
 * fetches -- either can still ride the ambient cookie through this proxy
 * today. Mirrors the Origin check `app/connect-claude/authorize/route.ts`
 * already carries for its own single state-changing POST, applied here at
 * the shared-proxy level instead: reject a non-GET request whose `Origin`
 * header is present and doesn't match this app's real public origin
 * (`getSiteOrigin()`, not `req.nextUrl.origin` -- see that route's own doc
 * comment on why `req.nextUrl.origin` is useless for this comparison under
 * plain `next start`). An ABSENT Origin is let through rather than
 * rejected: some legitimate same-origin requests (and any non-browser API
 * client that exists) may not send one, and this app has no CSRF-token
 * convention to fall back on to tell those apart from a forged request --
 * same posture `isSameOriginRequest`'s own doc comment describes, except
 * that route also falls back to Referer, which this shared proxy does not
 * (its ~50 call sites are same-origin `fetch()` calls that always send
 * Origin on a non-GET request in every browser this app supports, so a
 * Referer fallback would add complexity with no real call site to serve). */
async function hasAcceptableOriginForMutation(req: NextRequest): Promise<boolean> {
  if (req.method === 'GET' || req.method === 'HEAD') return true;
  const origin = req.headers.get('origin');
  if (origin === null) return true;
  const expectedOrigin = await getSiteOrigin();
  return origin === expectedOrigin;
}

async function proxy(req: NextRequest, path: string[]): Promise<NextResponse> {
  if (!(await hasAcceptableOriginForMutation(req))) {
    return new NextResponse('cross-site request rejected', { status: 403 });
  }

  // Build the target as a `URL` and check the *resolved* pathname still
  // lives under one of the allowed prefixes (`/public/`, `/Train/`,
  // `/Journeys`, `/JourneyTemplates`), rather than trying to reject specific
  // traversal patterns in the raw segments. Next.js decodes catch-all
  // segments before populating `path`, so a raw join could otherwise let
  // `..` (however it got there — literal, `%2e%2e`, an embedded `%2F`,
  // etc.) escape the intended scope and reach other routes on the backend
  // host. Checking the URL parser's actual normalized output is strictly
  // stronger than enumerating every encoding trick that could produce a
  // traversal -- same check as before this prefix list existed, just
  // checked against whichever allowed prefix applies instead of one.
  const target = new URL(`${process.env.API_BASE_URL}${resolveTargetPath(path)}${req.nextUrl.search}`);
  // `POST /api/Journeys` and `POST /api/JourneyTemplates` both resolve to
  // their bare root path (no trailing segment at all, unlike every
  // `/Train/...` call this proxy has ever forwarded), so this can't just
  // check a `/Journeys/`/`/JourneyTemplates/` prefix the way `/public/` and
  // `/Train/` are checked below -- it has to accept the bare path exactly
  // too.
  const isAllowed =
    target.pathname.startsWith('/public/') ||
    target.pathname.startsWith('/Train/') ||
    target.pathname === '/Journeys' ||
    target.pathname.startsWith('/Journeys/') ||
    target.pathname === '/JourneyTemplates' ||
    target.pathname.startsWith('/JourneyTemplates/') ||
    target.pathname.startsWith('/Trips/');
  if (!isAllowed) {
    return new NextResponse('invalid path', { status: 400 });
  }

  // Forward the incoming Content-Type verbatim rather than hardcoding
  // 'application/json' -- a browser's fetch(url, { body: formData }) sets
  // its own 'multipart/form-data; boundary=...' header, and axum's
  // Multipart extractor needs that exact boundary value to parse an
  // uploaded file field at all (the ticket-upload routes this proxy must
  // now support --
  // docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md
  // Correction 2). Every existing JSON caller (PinToggle, TrackTrainForm,
  // preferences, the OIDC flow) already sets its own
  // 'Content-Type': 'application/json' header on the request it sends to
  // this proxy, so this is inert for them -- the fallback below only
  // matters for a request that somehow reaches this proxy with no
  // Content-Type header at all.
  const headers: Record<string, string> = {
    'Content-Type': req.headers.get('content-type') ?? 'application/json',
  };
  const cookie = forwardedCookieHeader(req.headers.get('cookie'));
  if (cookie) {
    headers.Cookie = cookie;
  }
  // Forward whatever `X-Forwarded-For` this app's own Ingress already set on
  // the incoming request (`charts/distant-signal/templates/ingress.yaml` --
  // a plain host-routed `nginx` Ingress, which sets this header itself on
  // every request it forwards, same as any standard reverse proxy) through
  // to the backend fetch call -- same reasoning as the Origin/Referer
  // forwarding immediately below: Node's own `fetch` does not carry over ANY
  // of the inbound request's headers automatically, so without this, every
  // request `api` ever saw through this proxy looked like it came from the
  // frontend pod's own address, with nothing to attribute a future per-IP
  // rate limit (on `/auth/login`, say) to the real client (2026-09-26
  // review, finding L16). This app's own Next.js server (App Router route
  // handlers, this Next.js version) exposes no lower-level access to the raw
  // TCP peer address of the request it received to append its own hop onto
  // the chain -- what's relayed here is exactly what the Ingress already put
  // in the header, unmodified.
  const forwardedFor = req.headers.get('x-forwarded-for');
  if (forwardedFor) {
    headers['X-Forwarded-For'] = forwardedFor;
  }
  // Forward the browser's own Origin/Referer through verbatim -- api's
  // own strict same-origin check on POST /auth/logout (2026-09-25
  // Low-severity auth-core review) reads these headers directly off the
  // request IT receives, which is THIS proxy's own server-side fetch, not
  // the browser's original request. A browser always sends Origin (and
  // usually Referer) on a same-origin POST/PUT/DELETE per the fetch spec
  // -- `hasAcceptableOriginForMutation` above already relies on exactly
  // that fact -- but Node's own `fetch` does not fabricate either header
  // on an outbound call the way a browser does, so without this the
  // backend saw neither header on every single proxied request and its
  // strict check (fails closed when both are absent) rejected every real
  // logout with a 403.
  const origin = req.headers.get('origin');
  if (origin) {
    headers.Origin = origin;
  }
  const referer = req.headers.get('referer');
  if (referer) {
    headers.Referer = referer;
  }

  const init: RequestInit = {
    method: req.method,
    headers,
    // 'manual': a 3xx response from `api` (the OIDC login/callback
    // redirects -- see this file's module doc comment) must reach the
    // *browser* as a redirect, not be followed transparently by this
    // server-side fetch call. Node's fetch (unlike a browser's) still
    // gives back a normal, readable Response for a manual redirect --
    // status in [300, 400) and a real `location` header -- rather than an
    // opaque one, so this is safe to branch on below.
    redirect: 'manual',
  };
  if (req.method !== 'GET' && req.method !== 'DELETE') {
    // arrayBuffer(), not text(): .text() decodes the incoming body as
    // UTF-8 before this function ever sees it, which is LOSSY for
    // non-UTF-8 bytes -- a .pkpass (zip) or PDF's raw bytes are binary and
    // not valid UTF-8 in general, so any invalid byte sequence becomes a
    // U+FFFD replacement character on the way through, silently
    // corrupting the file before it reaches the backend. arrayBuffer()
    // forwards the exact bytes the browser sent, with no decode/re-encode
    // step -- a JSON body round-trips identically (JSON is always valid
    // UTF-8, so this is inert for PinToggle/TrackTrainForm/preferences/
    // auth) and a binary multipart body survives byte-for-byte.
    init.body = await req.arrayBuffer();
  }

  const response = await fetch(target, init);

  // A response can carry *multiple* Set-Cookie headers, which
  // `Headers.get()` collapses into one comma-joined string -- unusable
  // for cookies, since a cookie's own `Expires` attribute contains a
  // comma. `getSetCookie()` returns them as a proper string array.
  const setCookies = response.headers.getSetCookie();

  if (response.status >= 300 && response.status < 400) {
    const responseHeaders = new Headers();
    const location = response.headers.get('location');
    if (location) {
      responseHeaders.set('location', location);
    }
    for (const setCookie of setCookies) {
      responseHeaders.append('set-cookie', setCookie);
    }
    return new NextResponse(null, { status: response.status, headers: responseHeaders });
  }

  const body = await response.text();
  const responseHeaders = new Headers({
    'Content-Type': response.headers.get('Content-Type') ?? 'application/json',
  });
  for (const setCookie of setCookies) {
    responseHeaders.append('set-cookie', setCookie);
  }
  // Null-body statuses (204/205/304) may not carry a body on the outgoing
  // Response, not even an empty string -- see the existing PUT/DELETE
  // endpoints this handled before this change; unaffected by this edit.
  return new NextResponse(body === '' ? null : body, { status: response.status, headers: responseHeaders });
}

export async function GET(req: NextRequest, { params }: { params: Promise<{ path: string[] }> }) {
  return proxy(req, (await params).path);
}

export async function POST(req: NextRequest, { params }: { params: Promise<{ path: string[] }> }) {
  return proxy(req, (await params).path);
}

export async function PUT(req: NextRequest, { params }: { params: Promise<{ path: string[] }> }) {
  return proxy(req, (await params).path);
}

export async function DELETE(req: NextRequest, { params }: { params: Promise<{ path: string[] }> }) {
  return proxy(req, (await params).path);
}
