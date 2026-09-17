import { LoginLink } from './LoginLink';
import { AccountMenu } from './AccountMenu';
import { ACCOUNT_MENU_DESTINATIONS } from '@/lib/navLinks';
import type { SessionInfo } from '@/lib/types';

/** Nav-bar auth control. Takes `session` as a prop (rather than fetching
 * it itself) so it stays a plain, server-renderable function — the actual
 * fetch lives in `app/layout.tsx`, following the same split
 * `DataFreshnessNavItem`/`DataFreshnessInfo` already use. Only the
 * interactive leaves (`LoginLink`, `AccountMenu`) need `'use client'`;
 * the branch between them is a server-side decision, so it stays here.
 *
 * Logged out: a plain nav link to `/api/auth/login` — a full browser
 * navigation is enough to kick off the OIDC redirect, no client JS
 * required (see `crates/api/src/routes/auth.rs`'s `login` handler).
 *
 * Logged in: an avatar that opens `AccountMenu`. This used to render the
 * display name as visible text next to a full "Log out" `Button`, which
 * is what made the authenticated bar 12px too wide for its container at
 * 1440px and wrapped the whole nav onto a second row in Chromium — see
 * `AccountMenu`'s own doc comment for the measurements and
 * `components/AppNavBar.tsx` for the rest of the fix. The name is still
 * in the accessibility tree (the avatar's `aria-label`) and still
 * on-screen (its initials, and in full as the open menu's label); it is
 * only the always-on run of bar text that went away. */
export function AuthStatus({ session }: { session: SessionInfo }) {
  if (!session.authenticated) {
    return <LoginLink>Log in</LoginLink>;
  }

  // `?.trim() ||`, not `??`: an identity provider with no name on file for
  // a user sends `"name": ""` rather than omitting the claim, and `??`
  // treats that empty string as a perfectly good label -- leaving an empty
  // gap next to "Log out". Same defect the group member list and
  // shared-train attribution had; see `data::users::non_blank`, which is
  // where the backend now stops blanks at the boundary. Resolving it here
  // rather than inside `AccountMenu` is deliberate: that component takes
  // an already-non-blank `label`, so there is exactly one place this
  // fallback chain can be got wrong.
  const label = session.name?.trim() || session.email?.trim() || 'Signed in';
  return <AccountMenu label={label} destinations={ACCOUNT_MENU_DESTINATIONS} />;
}
