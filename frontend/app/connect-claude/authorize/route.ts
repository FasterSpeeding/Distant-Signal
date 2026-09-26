import { NextRequest, NextResponse } from 'next/server';
import { getSiteOrigin } from '@/lib/siteOrigin';

// SESSION_COOKIE_NAME, crates/api/src/auth.rs:63 -- must match exactly. This
// route is the one place in frontend/ that reads this cookie's raw value
// directly (rather than forwarding a `Cookie` header on through to `api` at
// all) -- see Open questions/risks #3 of
// docs/superpowers/plans/2026-09-02-embedded-chatbot-shared-foundation-and-option-c.md.
// (`app/api/[...path]/route.ts` forwards only this cookie and the OIDC
// login-state one by name, not the whole `Cookie` header verbatim, since
// the 2026-09-26 "Repeater Signal" review's finding L5.)
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

/** Rejects a cross-site POST before it ever touches the ambient session
 * cookie. This route completes an OAuth grant server-to-server using
 * whatever `distant_signal_session` cookie the browser happens to attach --
 * exactly the shape a confused-deputy/CSRF-vulnerable endpoint takes: a
 * hostile page that gets a victim's browser to POST here would otherwise
 * complete a grant the victim never consented to.
 *
 * The session cookie is already `SameSite=Lax` (crates/api/src/auth.rs's
 * `set_cookie_header`), which blocks the classic cross-site
 * form-post-from-another-site case in a spec-compliant browser -- a
 * cross-site top-level POST navigation isn't a "safe" method, so Lax
 * withholds the cookie (the same reasoning crates/api/src/main.rs's CORS
 * comment relies on for every other cookie-authenticated mutation this app
 * has). But that's this app's *only* other CSRF precedent, it's enforced
 * entirely client-side with nothing backing it up server-side, and this is
 * the one route that turns an ambient cookie into a completed OAuth grant --
 * so, belt-and-suspenders, this also verifies the request actually
 * originated from this app's own origin (falling back to Referer when
 * Origin is absent, and refusing to guess when neither is present), the
 * standard OWASP-recommended Origin check for a state-changing handler like
 * this one. There's no CSRF-token convention anywhere else in this
 * codebase to reuse instead.
 *
 * The "this app's own origin" it compares against is `getSiteOrigin()`
 * (lib/siteOrigin.ts), NOT `req.nextUrl.origin`. This app is served via
 * `next start` with no explicit host/`trustHostHeader` config, so
 * `req.nextUrl.origin` is always derived from whatever bare host Next
 * itself bound to (effectively `https://localhost:3000`-shaped), never the
 * real public origin a browser's `Origin`/`Referer` header actually
 * carries -- comparing against it made this check reject every real
 * Approve/Deny POST, not just cross-site ones. `getSiteOrigin()` is this
 * app's one existing answer to "what's our real public origin" (already
 * used by `app/groups/[id]/page.tsx`/`app/journeys/[id]/page.tsx` for
 * building shareable absolute links), so this reuses it rather than
 * inventing a second way to resolve the same fact. */
async function isSameOriginRequest(req: NextRequest): Promise<boolean> {
  const expectedOrigin = await getSiteOrigin();
  const origin = req.headers.get('origin');
  if (origin !== null) {
    return origin === expectedOrigin;
  }
  const referer = req.headers.get('referer');
  if (referer !== null) {
    try {
      return new URL(referer).origin === expectedOrigin;
    } catch {
      return false;
    }
  }
  // Neither header present -- a real browser-submitted POST always sends
  // at least one of these today, so treat the absence of both as hostile
  // (or at best a client this check can't vouch for) rather than let it
  // through.
  return false;
}

/** `mcp_request_id` is an opaque identifier this route both interpolates
 * into an internal URL (the `railMcp` `pending-authorization` lookup below,
 * carrying `X-Internal-Complete-Token`) and echoes back into a `return_to`
 * redirect target. Neither use ever encodes/escapes it beyond this shape
 * check, so an attacker-crafted value (`../`, an embedded `?`/`#`, etc.)
 * could otherwise redirect that internal fetch to an attacker-chosen path
 * on the `railMcp` host with the internal completion token attached, or
 * corrupt the `return_to` query string. The real values this route ever
 * generates or receives from `railMcp` are opaque request ids with no
 * reason to contain anything outside this set, so this rejects anything
 * else outright rather than trying to sanitise it. */
function isValidMcpRequestId(value: string): boolean {
  return /^[A-Za-z0-9_-]+$/.test(value);
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
  if (!isValidMcpRequestId(mcpRequestId)) {
    return new NextResponse('invalid mcp_request_id', { status: 400 });
  }

  // This app's own real public origin, NOT `req.url` -- `req.url` is
  // resolved off the same bare-localhost-shaped base `req.nextUrl.origin`
  // is (see `isSameOriginRequest`'s doc comment above), so building the
  // login redirect from it sent every logged-out visitor's browser to
  // `https://localhost:3000/api/auth/login?...` instead of this
  // deployment's real host.
  const origin = await getSiteOrigin();

  const sessionCookie = req.cookies.get(SESSION_COOKIE_NAME)?.value;
  if (!sessionCookie) {
    // Same login entry point every other authenticated page uses
    // (LoginLink.tsx) -- return_to is a plain relative path with a query
    // string, exactly the shape crates/api/src/auth.rs's validate_return_to
    // already accepts. `mcpRequestId` is encoded here too (on top of the
    // shape check above) purely as defense in depth -- it's already
    // validated to a safe charset, but an unencoded id interpolated
    // straight into a query string is still the wrong habit to leave in
    // place next to `encodeURIComponent(returnTo)` just below it.
    const returnTo = `/connect-claude/authorize?mcp_request_id=${encodeURIComponent(mcpRequestId)}`;
    return NextResponse.redirect(new URL(`/api/auth/login?return_to=${encodeURIComponent(returnTo)}`, origin));
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
  if (!(await isSameOriginRequest(req))) {
    return new NextResponse('cross-site request rejected', { status: 403 });
  }

  const mcpRequestId = req.nextUrl.searchParams.get('mcp_request_id');
  const sessionCookie = req.cookies.get(SESSION_COOKIE_NAME)?.value;
  if (!mcpRequestId || !sessionCookie) {
    return new NextResponse('invalid request', { status: 400 });
  }
  if (!isValidMcpRequestId(mcpRequestId)) {
    return new NextResponse('invalid mcp_request_id', { status: 400 });
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
  // `redirectUrl` is `railMcp`'s own (external, cross-service) value --
  // `NextResponse.redirect()` builds a `new URL(redirectUrl)` internally
  // with no base, which throws a raw `TypeError` for anything that isn't
  // an absolute URL. Left unhandled, that surfaced as an unhandled 500
  // *after* the grant/denial above had already taken effect server-side --
  // a confusing failure point for something checkable up front. Validating
  // the shape here turns that into a clean, well-understood 400, and the
  // explicit http(s)-only check also stops a non-navigable or otherwise
  // unexpected scheme (e.g. `javascript:`) from ever reaching a `Location`
  // header, on the same "don't trust an external value's shape" footing as
  // this route's own `isValidMcpRequestId` check above.
  if (!isValidHttpRedirectUrl(redirectUrl)) {
    return new NextResponse('Received an invalid redirect target from the authorization adapter.', { status: 400 });
  }
  return NextResponse.redirect(redirectUrl);
}

function isValidHttpRedirectUrl(value: string): boolean {
  try {
    const parsed = new URL(value);
    return parsed.protocol === 'http:' || parsed.protocol === 'https:';
  } catch {
    return false;
  }
}
