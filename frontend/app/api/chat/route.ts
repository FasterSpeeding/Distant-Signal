import { NextRequest } from 'next/server';

// A dedicated route, not an extension of the existing catch-all
// `frontend/app/api/[...path]/route.ts` -- the embedded-chatbot-option-b
// plan's own Decision 4/Task 4 rationale: that proxy is a generic
// body/status passthrough with special-cased redirect/cookie handling for
// OIDC, never built to hold a connection open and stream a chunked
// response for an indeterminate duration. This route's lifecycle is
// genuinely different (long-lived, streamed, no `Set-Cookie` handling
// needed at all) and costs nothing extra to keep separate -- Next.js
// routes independently per path already.
//
// Forwards the browser's `Cookie` header (carrying `distant_signal_session`)
// and the JSON request body to `orchestrator/`'s `POST /chat`
// (`ClusterIP`-only, per the dual-mode design's Decision 2 -- this route is
// the only thing that can reach it from outside the cluster), then streams
// the orchestrator's own `text/event-stream` response straight back to the
// browser, unmodified.

function orchestratorBaseUrl(): string {
  const url = process.env.ORCHESTRATOR_BASE_URL;
  if (!url) {
    throw new Error('ORCHESTRATOR_BASE_URL environment variable is not set');
  }
  return url;
}

export async function POST(req: NextRequest): Promise<Response> {
  const cookie = req.headers.get('cookie') ?? '';
  const body = await req.text();

  const upstream = await fetch(`${orchestratorBaseUrl()}/chat`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Cookie: cookie },
    body,
  });

  // Stream the orchestrator's own body straight through -- no buffering,
  // no re-encoding. Status/error bodies (401/403 JSON) pass through
  // unmodified too, same as every other status the [...path] proxy
  // forwards; `upstream.body` is the same `ReadableStream` object handed
  // straight to the outgoing `Response`, not read and re-wrapped, so a 200
  // SSE stream keeps streaming incrementally to the browser rather than
  // being buffered here first.
  return new Response(upstream.body, {
    status: upstream.status,
    headers: { 'Content-Type': upstream.headers.get('Content-Type') ?? 'application/json' },
  });
}
