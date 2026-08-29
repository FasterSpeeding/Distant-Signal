import { NextRequest, NextResponse } from 'next/server';

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
// passthrough: the incoming `Cookie` header must reach `api` (so
// `/auth/session` and `/auth/callback` can read the session/state cookies
// the browser is holding), and every `Set-Cookie` `api` sends back must
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
async function proxy(req: NextRequest, path: string[]): Promise<NextResponse> {
  // Build the target as a `URL` and check the *resolved* pathname still lives
  // under `/public/` rather than trying to reject specific traversal
  // patterns in the raw segments. Next.js decodes catch-all segments before
  // populating `path`, so a raw join could otherwise let `..` (however it
  // got there — literal, `%2e%2e`, an embedded `%2F`, etc.) escape the
  // intended `/public/*` scope and reach other routes on the backend host.
  // Checking the URL parser's actual normalized output is strictly stronger
  // than enumerating every encoding trick that could produce a traversal.
  const target = new URL(`${process.env.API_BASE_URL}/public/${path.join('/')}${req.nextUrl.search}`);
  if (!target.pathname.startsWith('/public/')) {
    return new NextResponse('invalid path', { status: 400 });
  }

  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  const cookie = req.headers.get('cookie');
  if (cookie) {
    headers.Cookie = cookie;
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
    init.body = await req.text();
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
