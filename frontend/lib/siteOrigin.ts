import { headers } from 'next/headers';

/** Resolves this deployment's own externally-reachable origin (scheme +
 * host, no trailing slash) for building ABSOLUTE URLs server-side --
 * e.g. `GroupInviteLinkCard`'s invite link, which points at a DIFFERENT
 * page than the one it's rendered on and so can't fall back to a relative
 * path the way `ShareButton`'s `window.location.href`-based share can.
 *
 * Computed server-side (a Server Component calls this, not the client
 * component that renders the link) so the URL is correct and copyable/
 * shareable on the very first render -- no `useEffect`/`window.location`
 * round-trip, and no window where `origin === ''` and Share silently does
 * nothing (review §2.11).
 *
 * Prefers `NEXT_PUBLIC_SITE_URL` (one var, set once per deployment -- the
 * same "public URL env var" shape `NEXT_PUBLIC_RAILMCP_PUBLIC_URL` already
 * uses elsewhere in this app for the same "reads correctly regardless of
 * what's in front of the app" reason) since a reverse proxy that drops or
 * mangles `Host`/`X-Forwarded-*` would otherwise produce a wrong link.
 * Falls back to the incoming request's own `Host` header (via
 * `next/headers`, the same module `lib/api.ts`/`lib/liveDataCache.ts`
 * already read request state through) when that var isn't set -- enough
 * for local dev and any deployment that forwards `Host` correctly. */
/** What a well-formed `Host` header looks like: a bare hostname or
 * `hostname:port`, nothing else. Guards the request-header fallback below
 * (Signal Box Audit, flib Low finding: "production never sets the public
 * site URL") -- this value ends up in share links, invite links, and the
 * same-origin checks `app/connect-claude/authorize/route.ts` and
 * `app/api/[...path]/route.ts` build from `getSiteOrigin()`, so a `Host`
 * value that isn't a plain hostname (embedded whitespace/CRLF, a stray `/`
 * or `@` that could turn "the origin" into something else once spliced into
 * a URL) is treated as absent rather than trusted verbatim. Every reverse
 * proxy this app actually runs behind sends a `Host` header that fits this
 * shape, so this never fires in a correctly configured deployment. */
const VALID_HOST = /^[a-zA-Z0-9.-]+(:\d+)?$/;

/** Module-level, not per-call: this is meant to fire once per server
 * process (surfacing a real misconfiguration in the logs) rather than once
 * per request, which would spam it at ordinary traffic volumes. */
let warnedMissingSiteUrl = false;

/** Test-only, mirroring `lib/liveDataCache.ts`'s own `__resetStaleCacheForTests`
 * convention -- lets a test assert the warning fires without leaking state
 * into whichever test happens to run after it in the same file. */
export function __resetSiteOriginWarningForTests(): void {
  warnedMissingSiteUrl = false;
}

export async function getSiteOrigin(): Promise<string> {
  const configured = process.env.NEXT_PUBLIC_SITE_URL;
  if (configured) {
    return configured.replace(/\/+$/, '');
  }

  // The review's own finding: this deployment's chart never actually sets
  // `NEXT_PUBLIC_SITE_URL`, so every share/invite link and same-origin check
  // silently falls back to trusting whatever `Host` a reverse proxy (or,
  // absent one, the request itself) hands this process -- a
  // misconfiguration that should be loud in the server logs rather than an
  // indefinitely silent degradation. This is a code-level guard, not a fix
  // for the underlying deployment gap (see this function's own doc comment
  // above); the real fix is setting the env var in the chart.
  if (!warnedMissingSiteUrl) {
    warnedMissingSiteUrl = true;
    console.warn(
      'getSiteOrigin(): NEXT_PUBLIC_SITE_URL is not set -- falling back to the request Host header for ' +
        'share/invite links and same-origin checks. Set NEXT_PUBLIC_SITE_URL in production so these do not ' +
        "depend on a reverse proxy forwarding Host correctly.",
    );
  }

  const headerList = await headers();
  const rawHost = headerList.get('host') ?? 'localhost:3000';
  // See `VALID_HOST`'s own doc comment: a malformed/hostile `Host` value is
  // treated as absent rather than trusted verbatim.
  const host = VALID_HOST.test(rawHost) ? rawHost : 'localhost:3000';
  // `x-forwarded-proto` is what a reverse proxy terminating TLS in front of
  // a plain-HTTP origin sets -- `headers()` alone has no scheme. Localhost
  // (dev, and this test suite's implicit default) has no proxy in front of
  // it, so it's the one host that defaults to `http` rather than `https`.
  const proto = headerList.get('x-forwarded-proto') ?? (host.startsWith('localhost') ? 'http' : 'https');
  return `${proto}://${host}`;
}
