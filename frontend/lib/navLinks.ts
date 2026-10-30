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
  /** Paths inside `section` that belong to another entry instead, as the
   * same kind of prefix. */
  except?: readonly string[];
}

/** The one journeys entry: "Journeys", `/journeys` (DS-115, 21 Oct 2026).
 * It was "Track" on `/track/mine` (round 2, 12 Oct 2026), which had folded
 * four tracking entries into one; `/track/mine` now redirects here
 * (`next.config.mjs`). The page lists your journeys, upcoming then past,
 * with "Plan a journey", "Find a train", "Add a ticket" and journey
 * templates one tap away. The other tracking pages, the planner included,
 * keep their URLs (deep links and pre-fill query params depend on them) and
 * mark this entry as the current section: `/track`, `/plan`, and a tracked
 * train's own page (`/train/by-id/...`). `/trains` is not one of them since
 * it became the Trains section (2026-10-30). */
export const JOURNEYS_DESTINATION: NavDestination = {
  href: '/journeys',
  label: 'Journeys',
  section: ['/track', '/journeys', '/plan', '/train/by-id'],
};

/** The Trains section, `/trains` (2026-10-30): the timetable for seeing
 * schedules and finding a specific train (a station's departures and
 * arrivals, the trains between two stations, a train by headcode or UID),
 * with no tracking or journey steps in the way. It was "Find a train",
 * reached from the Journeys page and marked as part of Journeys. Its
 * results open a train's page (`/train/[uid]/[date]`), which is marked as
 * part of it too; a tracked train's page (`/train/by-id/...`) is Journeys'. */
export const TRAINS_DESTINATION: NavDestination = {
  href: '/trains',
  label: 'Trains',
  section: ['/train'],
  except: ['/train/by-id'],
};

/** `/plan`, "Plan a journey": where every journey starts (DS-194). It
 * plans a route, or builds the journey from a train the visitor already
 * knows, or from a Tube, bus or walk leg (`JourneyCreationFlow`), and links
 * to tickets and journey templates. `/journeys/new`, the "New journey" page
 * it absorbed, redirects here with its query (`next.config.mjs`).
 *
 * Not a nav entry: it is reached from the button at the top of the Journeys
 * page and of the home page's "Upcoming journeys", and from station and
 * stop pages' "Plan a journey from here". */
export const PLAN_JOURNEY_HREF = '/plan';

/** The words on every link and button that opens `PLAN_JOURNEY_HREF`,
 * which is also that page's h1 and title (docs/style-guide.md, "Tone"). */
export const PLAN_JOURNEY_LABEL = 'Plan a journey';

/** Always visible to everyone, logged in or not. Rendered inline in the
 * bar at `md` and up, and in the drawer below it. Each label is the h1 of
 * the page it opens (docs/style-guide.md, "Tone"). The travel entries come
 * first (your journeys, then the timetable), then the network's
 * state and reference pages. */
export const PRIMARY_NAV_DESTINATIONS: readonly NavDestination[] = [
  JOURNEYS_DESTINATION,
  TRAINS_DESTINATION,
  { href: '/status', label: 'Status', section: ['/network'] },
  { href: '/lines', label: 'Lines' },
  { href: '/operators', label: 'Operators' },
  { href: '/stations', label: 'Stations' },
  { href: '/incidents', label: 'Incidents' },
];

/** Whether a nav entry marks the current page: an exact match on its own
 * href, or a match on one of its `section` prefixes that none of its
 * `except` prefixes also matches. Exact, not `startsWith`, for the href
 * itself: `/` would otherwise light up on every route, and `/lines` on a
 * single line's page. Shared by the bar (`PrimaryNavLink`) and the drawer
 * so the two agree. */
export function isActiveNavHref(pathname: string | null, destination: NavDestination | string): boolean {
  if (pathname === null) return false;
  const { href, section = [], except = [] } = typeof destination === 'string' ? { href: destination } : destination;
  if (pathname === href) return true;
  const under = (prefix: string) => pathname === prefix || pathname.startsWith(`${prefix}/`);
  return section.some(under) && !except.some(under);
}

/** Visible only to authenticated users: a group has no useful anonymous
 * landing state at all. */
export const GROUPS_DESTINATION: NavDestination = { href: '/groups', label: 'Groups' };

/** `/chat`. Listed for every logged-in visitor, like `GROUPS_DESTINATION`:
 * the chat is open to every account (since 2026-10-14), and a logged-out
 * visitor would only meet a login prompt there. */
export const CHAT_DESTINATION: NavDestination = { href: '/chat', label: 'Ask about trains' };

/** The account page: download your data, delete your account (UK legal
 * audit LEG-4). Authenticated only. */
export const ACCOUNT_DESTINATION: NavDestination = { href: '/account', label: 'Account & data' };

/** `/account/preferences`: colour theme, notifications, pride flag and the
 * Claude API key (round 2, item 4). Open to everyone, since most of it is
 * saved in the browser, so the drawer always lists it, the footer links to
 * it, and logged-out desktop visitors, who have no account menu, get a gear
 * in the bar (`PreferencesNavIcon`). */
export const PREFERENCES_DESTINATION: NavDestination = { href: '/account/preferences', label: 'Preferences' };

/** `/transit`: the tram and metro networks Distant Signal serves
 * (docs/superpowers/specs/2026-10-09-transit-networks-design.md; TfL's
 * Underground, DLR and trams since the TfL transit design). Listed
 * only while the api serves at least one network, in the drawer after
 * "Lines": the inline bar has no width to spare, and `/lines` links to it
 * for desktop visitors. */
export const TRANSIT_DESTINATION: NavDestination = { href: '/transit', label: 'Trams and metros' };

/** What the mobile drawer lists: every primary destination, plus the
 * per-account ones the bar hands to the account menu on desktop. The drawer
 * is the ONLY nav surface below `md`, so it carries the union. */
export function navDrawerDestinations(authenticated: boolean, transitOn = false): NavDestination[] {
  return [
    ...PRIMARY_NAV_DESTINATIONS.flatMap((destination) =>
      destination.href === '/lines' && transitOn ? [destination, TRANSIT_DESTINATION] : [destination],
    ),
    ...(authenticated ? [GROUPS_DESTINATION, CHAT_DESTINATION, ACCOUNT_DESTINATION] : []),
    PREFERENCES_DESTINATION,
  ];
}

/** What the avatar-keyed account menu lists (above the session actions,
 * which the menu adds itself). Only rendered for an authenticated visitor,
 * so every entry is unconditional. */
export function accountMenuDestinations(): NavDestination[] {
  return [GROUPS_DESTINATION, CHAT_DESTINATION, ACCOUNT_DESTINATION, PREFERENCES_DESTINATION];
}
