/** The app's nav destinations, as data rather than as JSX.
 *
 * Exists because the same set of destinations now has to be rendered in
 * three different shapes — the inline bar links, the mobile drawer
 * (`components/AppNavDrawer.tsx`) and the account menu
 * (`components/AccountMenu.tsx`) — and a destination that only got added
 * to two of the three is exactly the bug a single list makes impossible.
 * Every consumer maps over these; nothing hard-codes an href or a label
 * of its own.
 *
 * Plain serializable objects, deliberately: two of the three consumers are
 * Client Components receiving this list as a prop from a Server Component,
 * so anything richer (a component reference, a function) would break
 * `next build`'s Server/Client boundary serialization check — the same
 * constraint `app/layout.tsx`'s own comment records for passing `Link`
 * into a Mantine `component` prop. */
export interface NavDestination {
  href: string;
  label: string;
}

/** Always visible to everyone, logged in or not. Rendered inline in the
 * bar at `md` and up, and in the drawer below it. */
export const PRIMARY_NAV_DESTINATIONS: readonly NavDestination[] = [
  { href: '/status', label: 'Status' },
  { href: '/lines', label: 'All Lines' },
  { href: '/stations', label: 'Station Lookup' },
  // The primary train-discovery surface. `/track` is still reachable
  // (from here via /trains' own manual fallback link, from
  // /stations/[crs], and from TicketEntryForm) but is no longer the first
  // thing a visitor is pointed at -- see
  // docs/superpowers/specs/2026-09-07-train-listing-page-design.md §4.
  { href: '/trains', label: 'Find a Train' },
  { href: '/incidents', label: 'Incident Archive' },
];

/** Reclassified from Tier 3 (hidden entirely when logged out) to
 * always-visible, per
 * docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md
 * Decision 6 -- a deliberate, named reversal of
 * docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md's
 * Decision 4, which chose "hidden entirely" specifically because at the
 * time an anonymous click would have resolved to a bare inline sentence
 * with nothing else on the page. Now that `/track/mine`'s own existing
 * `getMyTrackedTrains()` null-on-401 gate (unchanged -- see
 * `app/track/mine/page.tsx`) opens a real, actionable `LoginPromptModal`
 * instead, "dead weight in the nav bar" no longer describes what a
 * logged-out click produces, so this link is worth advertising rather
 * than hiding.
 *
 * That "always visible" is why it appears in BOTH `navDrawerDestinations`
 * (unconditionally) and, for anonymous visitors only, inline in the bar:
 * a logged-in visitor reaches it through the account menu instead, which
 * is where it moved to stop the authenticated bar wrapping onto a second
 * row at 1440px (see `components/AppNavBar.tsx`).
 *
 * Labelled "My Trains & Tickets," not "My Tracked Trains," now that
 * `/track/mine` is the single merged page for both (Part B of the
 * upload-first ticket-tracking plan). */
export const TRACKED_TRAINS_DESTINATION: NavDestination = {
  href: '/track/mine',
  label: 'My Trains & Tickets',
};

/** Visible only to authenticated users -- unlike
 * `TRACKED_TRAINS_DESTINATION` above, a group has no useful
 * anonymous-visitor landing state at all (an anonymous "Groups" click has
 * nothing to show but a login prompt with zero context), so every
 * consumer gates it on the session rather than advertising it to
 * everyone. */
export const GROUPS_DESTINATION: NavDestination = { href: '/groups', label: 'Groups' };

/** Review §3.1.3: `/chat` was undiscoverable -- reachable only by typing
 * the URL directly, with no nav entry pointing an allow-listed user at it.
 * Visible only when `getChatbotAccess()` (fetched alongside the session in
 * `app/layout.tsx`'s own `NavBarWithSession`, in the same `<Suspense>`
 * boundary) actually resolves to `'allowed'` -- a non-allow-listed logged-in
 * visitor gets the explained dead-end at `/chat` itself (Task 3.1.2), not a
 * nav item pointing at it. */
export const CHAT_DESTINATION: NavDestination = { href: '/chat', label: 'Chat' };

/** What the mobile drawer lists: every primary destination, plus the
 * per-account ones the bar hands to the account menu on desktop. The
 * drawer is the ONLY nav surface below `md`, so it has to carry the union
 * — a destination missing here is unreachable from the nav on a phone. */
export function navDrawerDestinations(authenticated: boolean, chatAllowed: boolean): NavDestination[] {
  return [
    ...PRIMARY_NAV_DESTINATIONS,
    TRACKED_TRAINS_DESTINATION,
    ...(authenticated ? [GROUPS_DESTINATION] : []),
    ...(chatAllowed ? [CHAT_DESTINATION] : []),
  ];
}

/** What the avatar-keyed account menu lists (above "Log out", which the
 * menu adds itself because it is an action, not a destination). Only ever
 * rendered for an authenticated visitor, so the always-on entries are
 * unconditional; `CHAT_DESTINATION` is appended only for an allow-listed
 * caller (`chatAllowed`) -- `getChatbotAccess()` already implies
 * authentication (it fails closed to `'forbidden'`, never `'allowed'`, for
 * an anonymous caller), so this never renders Chat for a logged-out
 * visitor. */
export function accountMenuDestinations(chatAllowed: boolean): NavDestination[] {
  return [TRACKED_TRAINS_DESTINATION, GROUPS_DESTINATION, ...(chatAllowed ? [CHAT_DESTINATION] : [])];
}
