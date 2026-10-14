/** The app's nav destinations, as data rather than as JSX.
 *
 * The same destinations are rendered in three shapes: the inline bar links
 * (`components/AppNavBar.tsx`), the mobile drawer
 * (`components/AppNavDrawer.tsx`) and the account menu
 * (`components/AccountMenu.tsx`). One list makes "added to two of the three"
 * impossible. Every consumer maps over these; nothing hard-codes an href or
 * a label of its own.
 *
 * Plain serializable objects, deliberately: two of the three consumers are
 * Client Components receiving this list as a prop from a Server Component,
 * so anything richer (a component reference, a function) would break
 * `next build`'s Server/Client boundary serialization check. */
export interface NavDestination {
  href: string;
  label: string;
  /** Other paths that belong to this entry, so it stays marked as the
   * current section on them (`isActiveNavHref`). Each is a path prefix
   * ending at a segment boundary: `/trains` matches `/trains` and
   * `/trains/x`, not `/trainsx`. */
  section?: readonly string[];
}

/** The one "Track" entry (round 2 decision, 12 Oct 2026). Tracking used to
 * be four nav entries for one task: "Track a journey" (`/journeys/new`),
 * "Trains" (`/trains`), "My trains & tickets" (`/track/mine`) and the
 * deep-linked `/track` form. Now `/track/mine` is the Track page: what you
 * are tracking, with "Track a journey", "Find a train", "Add a ticket" and
 * journey templates one tap away. The other pages keep their URLs (deep
 * links and pre-fill query params depend on them) and mark this entry as
 * the current section. */
export const TRACK_DESTINATION: NavDestination = {
  href: '/track/mine',
  label: 'Track',
  section: ['/track', '/trains', '/journeys'],
};

/** Where a new journey (or a single train, its one-leg case) starts:
 * `JourneyCreationFlow` on `/journeys/new`. Linked from the Track page and
 * the planner; no longer a nav entry of its own. */
export const TRACK_JOURNEY_HREF = '/journeys/new';

/** `/plan`: the trip planner. Planning needs no account, so it is in the
 * main bar for everyone, straight after Track. */
export const PLAN_JOURNEY_DESTINATION: NavDestination = { href: '/plan', label: 'Plan a journey' };

/** Always visible to everyone, logged in or not. Rendered inline in the
 * bar at `md` and up, and in the drawer below it. Each label is the h1 of
 * the page it opens (docs/style-guide.md, "Tone"). */
export const PRIMARY_NAV_DESTINATIONS: readonly NavDestination[] = [
  TRACK_DESTINATION,
  PLAN_JOURNEY_DESTINATION,
  { href: '/status', label: 'Status', section: ['/network'] },
  { href: '/lines', label: 'Lines' },
  { href: '/operators', label: 'Operators' },
  { href: '/stations', label: 'Stations' },
  { href: '/incidents', label: 'Incidents' },
];

/** Whether a nav entry marks the current page: an exact match on its own
 * href, or a match on one of its `section` prefixes. Exact, not
 * `startsWith`, for the href itself: `/` would otherwise light up on every
 * route, and `/lines` on a single line's page. Shared by the bar
 * (`PrimaryNavLink`) and the drawer so the two agree. */
export function isActiveNavHref(pathname: string | null, destination: NavDestination | string): boolean {
  if (pathname === null) return false;
  const { href, section = [] } = typeof destination === 'string' ? { href: destination } : destination;
  if (pathname === href) return true;
  return section.some((prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`));
}

/** Visible only to authenticated users: a group has no useful anonymous
 * landing state at all. */
export const GROUPS_DESTINATION: NavDestination = { href: '/groups', label: 'Groups' };

/** `/chat`. Visible only when `getChatbotAccess()` resolves to `'allowed'`
 * (fetched alongside the session in `app/layout.tsx`'s
 * `NavBarWithSession`). */
export const CHAT_DESTINATION: NavDestination = { href: '/chat', label: 'Ask about trains' };

/** The account page: download your data, delete your account (UK legal
 * audit LEG-4). Authenticated only. */
export const ACCOUNT_DESTINATION: NavDestination = { href: '/account', label: 'Account & data' };

/** `/account/preferences`: colour theme, notifications, pride flag and the
 * Claude API key (round 2, item 4). Open to everyone, since most of it is
 * saved in the browser, so the drawer always lists it and the footer links
 * to it for logged-out desktop visitors, who have no account menu. */
export const PREFERENCES_DESTINATION: NavDestination = { href: '/account/preferences', label: 'Preferences' };

/** `/transit`: the tram networks Distant Signal serves
 * (docs/superpowers/specs/2026-10-09-transit-networks-design.md). Listed
 * only while the api serves at least one network, in the drawer after
 * "Lines": the inline bar has no width to spare, and `/lines` links to it
 * for desktop visitors. */
export const TRANSIT_DESTINATION: NavDestination = { href: '/transit', label: 'Trams' };

/** What the mobile drawer lists: every primary destination, plus the
 * per-account ones the bar hands to the account menu on desktop. The drawer
 * is the ONLY nav surface below `md`, so it carries the union. */
export function navDrawerDestinations(
  authenticated: boolean,
  chatAllowed: boolean,
  transitOn = false,
): NavDestination[] {
  return [
    ...PRIMARY_NAV_DESTINATIONS.flatMap((destination) =>
      destination.href === '/lines' && transitOn ? [destination, TRANSIT_DESTINATION] : [destination],
    ),
    ...(authenticated ? [GROUPS_DESTINATION] : []),
    ...(chatAllowed ? [CHAT_DESTINATION] : []),
    ...(authenticated ? [ACCOUNT_DESTINATION] : []),
    PREFERENCES_DESTINATION,
  ];
}

/** What the avatar-keyed account menu lists (above the session actions,
 * which the menu adds itself). Only rendered for an authenticated visitor;
 * `CHAT_DESTINATION` only for an allow-listed one. */
export function accountMenuDestinations(chatAllowed: boolean): NavDestination[] {
  return [GROUPS_DESTINATION, ...(chatAllowed ? [CHAT_DESTINATION] : []), ACCOUNT_DESTINATION, PREFERENCES_DESTINATION];
}
