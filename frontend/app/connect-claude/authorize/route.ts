import { NextRequest, NextResponse } from 'next/server';

// SESSION_COOKIE_NAME, crates/api/src/auth.rs:63 -- must match exactly. This
// route is the one place in frontend/ that reads this cookie's raw value
// directly (rather than forwarding the whole Cookie header verbatim, the
// way app/api/[...path]/route.ts does) -- see Open questions/risks #3 of
// docs/superpowers/plans/2026-09-02-embedded-chatbot-shared-foundation-and-option-c.md.
const SESSION_COOKIE_NAME = 'distant_signal_session';

function railMcpBaseUrl(): string {
  const url = process.env.RAILMCP_BASE_URL;
  if (!url) throw new Error('RAILMCP_BASE_URL environment variable is not set');
  return url;
}

function internalCompleteToken(): string {
  const token = process.env.RAILMCP_INTERNAL_COMPLETE_TOKEN;
  if (!token) throw new Error('RAILMCP_INTERNAL_COMPLETE_TOKEN environment variable is not set');
  return token;
}

/** Escapes the only untrusted value this page ever interpolates into HTML --
 * the DCR-registered client_name (Open questions/risks #2: entirely
 * self-reported by the connecting MCP client, never verified). */
function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/** A small, deliberately non-Mantine-styled server-rendered HTML form --
 * this route is protocol machinery, not a product page (Task 9's own
 * /connect-claude page is where the actual designed UI lives). Mirrors this
 * app's existing precedent of bare, minimal auth-adjacent plumbing
 * (crates/api's own auth routes return bare text/redirects, not styled
 * HTML, for the same reason). A Route Handler can't render a React Server
 * Component tree directly, which is the other reason this stays plain HTML
 * rather than JSX. */
function renderConsentScreen({ mcpRequestId, clientName }: { mcpRequestId: string; clientName?: string }): NextResponse {
  const title = clientName ? escapeHtml(clientName) : 'An application';
  const html = `<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>Connect to Distant Signal</title></head>
<body style="font-family: sans-serif; max-width: 32rem; margin: 4rem auto; padding: 0 1rem;">
  <h1>Connect ${title} to Distant Signal</h1>
  <p>${title} wants to use your Distant Signal account to look up train departures, arrivals, and journeys on your behalf.</p>
  <form method="POST" action="/connect-claude/authorize?mcp_request_id=${encodeURIComponent(mcpRequestId)}">
    <button type="submit" name="decision" value="approve">Approve</button>
    <button type="submit" name="decision" value="deny">Deny</button>
  </form>
</body>
</html>`;
  return new NextResponse(html, { status: 200, headers: { 'Content-Type': 'text/html; charset=utf-8' } });
}

export async function GET(req: NextRequest) {
  const mcpRequestId = req.nextUrl.searchParams.get('mcp_request_id');
  if (!mcpRequestId) {
    return new NextResponse('missing mcp_request_id', { status: 400 });
  }

  const sessionCookie = req.cookies.get(SESSION_COOKIE_NAME)?.value;
  if (!sessionCookie) {
    // Same login entry point every other authenticated page uses
    // (LoginLink.tsx) -- return_to is a plain relative path with a query
    // string, exactly the shape crates/api/src/auth.rs's validate_return_to
    // already accepts.
    const returnTo = `/connect-claude/authorize?mcp_request_id=${mcpRequestId}`;
    return NextResponse.redirect(new URL(`/api/auth/login?return_to=${encodeURIComponent(returnTo)}`, req.url));
  }

  // Fetch the requesting client's display name (if DCR captured one) for
  // the consent screen -- best-effort, absent on any failure rather than
  // blocking consent on this call succeeding.
  let clientName: string | undefined;
  try {
    const pendingRes = await fetch(`${railMcpBaseUrl()}/internal/pending-authorization/${mcpRequestId}`, {
      headers: { 'X-Internal-Complete-Token': internalCompleteToken() },
      cache: 'no-store',
    });
    if (pendingRes.ok) {
      clientName = ((await pendingRes.json()) as { clientName?: string }).clientName;
    } else if (pendingRes.status === 404) {
      return new NextResponse('This authorization request has expired. Please try connecting again from Claude.', { status: 410 });
    }
  } catch {
    // Best-effort only -- render the consent screen without a client name
    // rather than fail the whole request on a transient adapter blip.
  }

  return renderConsentScreen({ mcpRequestId, clientName });
}

export async function POST(req: NextRequest) {
  const mcpRequestId = req.nextUrl.searchParams.get('mcp_request_id');
  const sessionCookie = req.cookies.get(SESSION_COOKIE_NAME)?.value;
  if (!mcpRequestId || !sessionCookie) {
    return new NextResponse('invalid request', { status: 400 });
  }
  const form = await req.formData();
  const approved = form.get('decision') === 'approve';

  const path = approved ? 'complete-authorization' : 'deny-authorization';
  const body = approved
    ? { mcp_request_id: mcpRequestId, ds_session_cookie_value: sessionCookie }
    : { mcp_request_id: mcpRequestId };

  const completeRes = await fetch(`${railMcpBaseUrl()}/internal/${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-Internal-Complete-Token': internalCompleteToken() },
    body: JSON.stringify(body),
  });
  if (!completeRes.ok) {
    return new NextResponse('Could not complete the connection. Please try again.', { status: 502 });
  }
  const { redirectUrl } = (await completeRes.json()) as { redirectUrl: string };
  return NextResponse.redirect(redirectUrl);
}
