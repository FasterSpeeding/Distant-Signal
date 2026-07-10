import { NextRequest, NextResponse } from 'next/server';

// Client Components can't read `API_BASE_URL` (server-only env var, not
// inlined into the browser bundle unless prefixed `NEXT_PUBLIC_`), so
// browser-initiated mutations (pinning, creating a custom line) can't call
// the `api` service directly. This catch-all proxies same-origin `/api/*`
// requests from the browser to `${API_BASE_URL}/public/*` server-side —
// since the browser only ever talks to this Next.js origin, no CORS
// relaxation on the `api` service is needed for these write endpoints.
async function proxy(req: NextRequest, path: string[]): Promise<NextResponse> {
  // Reject `.`/`..`/empty segments — Next.js decodes catch-all segments before
  // populating `path`, so a raw join could otherwise let `..` escape the
  // intended `/public/*` scope and reach other routes on the backend host.
  if (path.some((segment) => segment === '.' || segment === '..' || segment === '')) {
    return new NextResponse('invalid path', { status: 400 });
  }
  const url = `${process.env.API_BASE_URL}/public/${path.join('/')}${req.nextUrl.search}`;
  const init: RequestInit = { method: req.method, headers: { 'Content-Type': 'application/json' } };
  if (req.method !== 'GET' && req.method !== 'DELETE') {
    init.body = await req.text();
  }
  const response = await fetch(url, init);
  const body = await response.text();
  // Null-body statuses (204/205/304) may not carry a body on the outgoing
  // Response, not even an empty string — the backend's PUT/DELETE endpoints
  // return 204 with no content, and `new NextResponse('', { status: 204 })`
  // throws under the Fetch spec, so an empty upstream body must map to null.
  return new NextResponse(body === '' ? null : body, {
    status: response.status,
    headers: { 'Content-Type': response.headers.get('Content-Type') ?? 'application/json' },
  });
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
