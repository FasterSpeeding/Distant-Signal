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
export async function getSiteOrigin(): Promise<string> {
  const configured = process.env.NEXT_PUBLIC_SITE_URL;
  if (configured) {
    return configured.replace(/\/+$/, '');
  }

  const headerList = await headers();
  const host = headerList.get('host') ?? 'localhost:3000';
  // `x-forwarded-proto` is what a reverse proxy terminating TLS in front of
  // a plain-HTTP origin sets -- `headers()` alone has no scheme. Localhost
  // (dev, and this test suite's implicit default) has no proxy in front of
  // it, so it's the one host that defaults to `http` rather than `https`.
  const proto = headerList.get('x-forwarded-proto') ?? (host.startsWith('localhost') ? 'http' : 'https');
  return `${proto}://${host}`;
}
