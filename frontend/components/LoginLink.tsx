'use client';

import { useLoginHref } from './useLoginHref';
import { TextLink } from './TextLink';

/** Wraps `TextLink` with the shared `return_to`-bearing login href (see
 * `useLoginHref.ts`), so `GET /auth/callback`
 * (`crates/api/src/routes/auth.rs`) can send the user back here instead of
 * always to `SSO_POST_LOGIN_REDIRECT_URL`. See
 * docs/superpowers/specs/2026-08-31-dynamic-post-login-redirect-design.md's
 * Design → Where the return path is captured.
 *
 * A separate Client Component rather than adding these hooks to `TextLink`
 * itself: `usePathname()`/`useSearchParams()` (inside `useLoginHref`) are
 * Client-Component-only hooks, and `TextLink` must stay server-renderable
 * (see its own doc comment) since most of its call sites are Server
 * Components. This mirrors the existing `AuthStatus.tsx` embeds
 * `LogoutButton.tsx` pattern -- a small interactive Client Component leaf
 * inside a server-rendered tree.
 *
 * `prefetch={false}` is required, not decorative: this href is never a
 * real Next.js page -- it's `crates/api/src/routes/auth.rs`'s `login`
 * handler, proxied through `/api/[...path]`. Next's `<Link>` has no way to
 * know that, so its default viewport-visibility prefetching treats it like
 * any other in-app route and fires a background `fetch()` the moment this
 * link scrolls into view -- which, since this link lives in the
 * always-visible nav bar (`AuthStatus.tsx`, rendered whenever a visitor
 * isn't logged in), means essentially every anonymous page load. That
 * `login` handler is not a safe, side-effect-free GET: it calls
 * `app.oidc.authorize_url()`, inserts a fresh row into the `login_state`
 * table, and overwrites the browser's `login_state` cookie with a new
 * CSRF/PKCE/nonce triple -- confirmed happening for real by reading this
 * app's own live console/network logs, which showed `/api/auth/login`
 * being hit with a `return_to` matching whatever page merely happened to
 * be open, with no click involved (the follow-on redirect to the SSO
 * provider is same-originless and gets blocked by CORS, but that's *after*
 * the side-effecting request to this app's own backend has already
 * landed). A background prefetch silently overwriting that cookie can race
 * a real, intentional login click's own state -- the eventual
 * `/auth/callback` then either 400s on a state mismatch or, worse, honours
 * whichever `return_to` was written last, landing the user on a page they
 * never asked for. Disabling prefetch here doesn't change what a real
 * click does (it was always a hard, full-page redirect out of this app's
 * router entirely, never a soft client-side navigation) -- it only stops
 * the *unclicked* background request.
 *
 * Deliberately cannot capture a URL fragment (`#...`) -- a fragment is
 * never sent to the server on any HTTP request, by construction of the
 * URL/HTTP specs; there is no mechanism here or anywhere else that could
 * round-trip one through a full-page OIDC redirect. Known, accepted
 * limitation -- see the design spec's Open Questions. */
export function LoginLink({
  children,
  underline,
}: {
  children: React.ReactNode;
  underline?: 'hover' | 'always';
}) {
  const href = useLoginHref();
  return (
    <TextLink href={href} underline={underline} prefetch={false}>
      {children}
    </TextLink>
  );
}
