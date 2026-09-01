'use client';

import { usePathname, useSearchParams } from 'next/navigation';
import { TextLink } from './TextLink';

/** Wraps `TextLink` to append the current page's path + query string as a
 * `return_to` query parameter on the login link, so `GET /auth/callback`
 * (`crates/api/src/routes/auth.rs`) can send the user back here instead of
 * always to `SSO_POST_LOGIN_REDIRECT_URL`. See
 * docs/superpowers/specs/2026-08-31-dynamic-post-login-redirect-design.md's
 * Design → Where the return path is captured.
 *
 * A separate Client Component rather than adding these hooks to `TextLink`
 * itself: `usePathname()`/`useSearchParams()` are Client-Component-only
 * hooks, and `TextLink` must stay server-renderable (see its own doc
 * comment) since most of its call sites are Server Components. This
 * mirrors the existing `AuthStatus.tsx` embeds `LogoutButton.tsx` pattern
 * -- a small interactive Client Component leaf inside a server-rendered
 * tree -- which is why this works identically at the two Server Component
 * call sites (`AuthStatus.tsx`, `TicketPanel.tsx`) and the five Client
 * Component ones.
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
  const pathname = usePathname();
  const search = useSearchParams().toString();
  const returnTo = search ? `${pathname}?${search}` : pathname;
  const href = `/api/auth/login?return_to=${encodeURIComponent(returnTo)}`;
  return (
    <TextLink href={href} underline={underline}>
      {children}
    </TextLink>
  );
}
