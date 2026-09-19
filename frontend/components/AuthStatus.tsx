import { LoginLink } from './LoginLink';
import { AccountMenu } from './AccountMenu';
import { ACCOUNT_MENU_DESTINATIONS } from '@/lib/navLinks';
import type { SessionInfo } from '@/lib/types';

/** Nav-bar auth control. Takes `session` as a prop (rather than fetching
 * it itself) so it stays a plain, server-renderable function — the actual
 * fetch lives in `app/layout.tsx`'s `NavBarWithSession`, which is the one
 * `getSession()` call the whole nav makes. That is the same
 * fetch-above/render-below split `DataFreshnessInfo` uses for its own
 * `freshness` prop, and the same one this component in turn applies to
 * `AccountMenu` below. Only the interactive leaves (`LoginLink`,
 * `AccountMenu`) need `'use client'`; the branch between them is a
 * server-side decision, so it stays here.
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
    // `size="sm"` (14px), not `LoginLink`'s own Mantine-`Text`-default 16px:
    // review §2.16 named this as one leg of "auth controls are
    // inconsistently sized" (a 16px "Log in" beside a ~12px "Log out"), and
    // 14px is where the OTHER leg -- "Log out", now a `Menu.Item` inside
    // `AccountMenu`'s dropdown rather than the bar `Button` the review
    // measured -- already renders by Mantine's own default (see
    // `TextLink.tsx`'s `size` doc comment for the exact CSS variable this
    // traces to). Converging here, rather than bumping "Log out" up to
    // match a 16px "Log in", keeps the footer's own text-link-styled
    // control (`OpenDataAttribution.tsx`) at the same size too.
    return <LoginLink size="sm">Log in</LoginLink>;
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
