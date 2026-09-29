import { NextRequest, NextResponse } from 'next/server';
import { getSiteOrigin } from '@/lib/siteOrigin';
import { NONCE_HEADER } from '@/lib/csp';
import { renderConsentScreen, renderErrorPage } from './consentPage';

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

export async function GET(req: NextRequest) {
  // The per-request CSP nonce proxy.ts mints and forwards on the request
  // headers -- the consent/error pages' one inline script needs it.
  const nonce = req.headers.get(NONCE_HEADER);
  const mcpRequestId = req.nextUrl.searchParams.get('mcp_request_id');
  if (!mcpRequestId || !isValidMcpRequestId(mcpRequestId)) {
    return renderErrorPage({
      status: 400,
      heading: "This connection link isn't valid",
      message: "This link doesn't include a valid connection request. Please try connecting again from Claude.",
      nonce,
    });
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
      return renderErrorPage({
        status: 410,
        heading: 'This connection request has expired',
        message: 'This authorization request has expired. Please try connecting again from Claude.',
        nonce,
      });
    } else {
      // FE-12: any other non-OK answer (a 401 from a mis-set
      // RAILMCP_INTERNAL_COMPLETE_TOKEN, a 500) means approving would only
      // fail after the round trip, so stop here and log the real cause.
      console.error(`connect-claude/authorize: pending-authorization lookup failed with HTTP ${pendingRes.status}`);
      return renderErrorPage({
        status: 502,
        heading: "Couldn't start the connection",
        message: 'Could not start the connection. Please try again later.',
        nonce,
      });
    }
  } catch (err) {
    // Best-effort only -- render the consent screen without a client name
    // rather than fail the whole request on a transient adapter blip. Logged
    // (FE-12) so a persistent failure is visible.
    console.error('connect-claude/authorize: pending-authorization lookup threw', err);
  }

  return renderConsentScreen({ mcpRequestId, clientName, nonce });
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
    return renderErrorPage({
      status: 502,
      heading: "Couldn't complete the connection",
      message: 'Could not complete the connection. Please try again.',
      nonce: req.headers.get(NONCE_HEADER),
    });
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
