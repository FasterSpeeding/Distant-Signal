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

/** Journey tracking is this app's MAIN feature going forward -- tracking a
 * single train is the simple, one-leg case of a journey, not a separate
 * concept, and `/journeys/new` (`JourneyCreationFlow.tsx`) is the one
 * continuous flow for building either: fill in leg 1, then optionally keep
 * adding legs right there, all before ever leaving the page. This constant
 * is what makes that page THE primary, nav-linked entry point for the
 * feature -- first in `PRIMARY_NAV_DESTINATIONS` below, ahead of even
 * "Status", so it reads as the main way to start tracking something rather
 * than one browsing option among several.
 *
 * `/track` (the old single-leg-only form `JourneyCreationFlow` now wraps
 * as its own leg-1 step) deliberately keeps its own separate existence and
 * is NOT redirected here -- see that page's own doc comment for why: it is
 * still the landing target several existing, narrower flows depend on
 * (a standalone ticket's "find or track the train this ticket is for"
 * link, `/stations/[crs]`'s "Track a train from here", `/trains`' manual
 * fallback link, and `trackAgainHref`'s "Track this journey again"), each
 * of which wants a single pre-filled leg-1 form and nothing past it --
 * none of those needs the inline "Add a leg" step this page adds, and
 * redirecting them all through here would cost every one of them their own
 * pre-fill query params for no benefit. */
export const TRACK_JOURNEY_DESTINATION: NavDestination = { href: '/journeys/new', label: 'Track a Journey' };

/** Always visible to everyone, logged in or not. Rendered inline in the
 * bar at `md` and up, and in the drawer below it.
 *
 * Four of these labels ("Lines", "Stations", "Trains", "Incidents") are
 * deliberately terser than the page headings they point at ("All Lines",
 * "Station Disruption Lookup", "Find a Train", "Incident Archive" --
 * unchanged, see each page's own `<Title>`), matching the precedent
 * "Station Lookup" already set for this exact bar: a nav label is a short
 * pointer, not a restatement of the destination's own heading, and a
 * visitor landing on a fuller/differently-worded `<h1>` after a short nav
 * label is a pattern this bar already used before this rename, not one
 * invented for it.
 *
 * They were shortened from "All Lines"/"Station Lookup"/"Find a
 * Train"/"Incident Archive" specifically to buy back width for
 * `TRACK_JOURNEY_DESTINATION` below: added as a NEW, always-inline primary
 * destination, it broke `AppNavBar.tsx`'s single-row-at-`md`-and-`lg`
 * invariant (confirmed against a real rendered bar, both Chromium and
 * Firefox -- the anonymous bar, the widest arrangement per
 * `AppNavBar.tsx`'s own comment, wrapped at BOTH 992px and 1440px, not just
 * the narrower one). Combined with `AppNavBar.tsx`'s own gap reduction,
 * these four renames restore a real (not hairline) margin at 992px: ~30px
 * in both engines, in the same ballpark as the ~42px this file's history
 * already treated as an acceptable working margin -- see
 * `AppNavBar.tsx`'s own comment for the up-to-date figures.
 *
 * "Track a Journey", "Status", "Operators" and "My Trains & Tickets"
 * (below) were deliberately left untouched: the first because
 * `TRACK_JOURNEY_DESTINATION`'s own doc comment is unambiguous that its
 * wording and prominence are the point, not incidental; "Status" is
 * already minimal; "Operators" only just landed at this length and
 * position per review M7/§2.7; and "My Trains & Tickets" is deliberately
 * NOT renamed for a documented, layout-unrelated scope reason (see
 * `TRACKED_TRAINS_DESTINATION`'s own comment) despite being the single
 * widest item on the anonymous bar -- renaming it would have been the
 * cheapest width fix available, and was rejected for that reason. */
export const PRIMARY_NAV_DESTINATIONS: readonly NavDestination[] = [
  // First, ahead of "Status" -- see `TRACK_JOURNEY_DESTINATION`'s own doc
  // comment for why creation, not browsing, now leads the primary nav.
  TRACK_JOURNEY_DESTINATION,
  { href: '/status', label: 'Status' },
  { href: '/lines', label: 'Lines' },
  // Moved beside "Lines" and "Stations" (review M7/§2.7 -- named "All
  // Lines"/"Station Lookup" at the time, since renamed, see this array's
  // own doc comment): three ways to browse status, previously split apart
  // by having this one sit last, after "Incidents" -- reading as an
  // afterthought rather than a sibling of the other two
  // catalogue-browsing destinations.
  { href: '/operators', label: 'Operators' },
  { href: '/stations', label: 'Stations' },
  // The primary train-discovery surface. `/track` is still reachable
  // (from here via /trains' own manual fallback link, from
  // /stations/[crs], and from TicketEntryForm) but is no longer the first
  // thing a visitor is pointed at -- see
  // docs/superpowers/specs/2026-09-07-train-listing-page-design.md §4, and
  // `TRACK_JOURNEY_DESTINATION`'s own doc comment for what replaced it as
  // the primary entry point for tracking specifically.
  { href: '/trains', label: 'Trains' },
  { href: '/incidents', label: 'Incidents' },
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
 * upload-first ticket-tracking plan).
 *
 * NOT renamed to something journey-flavoured when `TRACK_JOURNEY_DESTINATION`
 * was added above, and deliberately so: `/track/mine` already grew its own
 * "Your journeys" section (2026-09-22 UX review C1, `app/track/mine/page.tsx`)
 * well before this constant existed, so its scope was already "everything
 * you're tracking" -- trains, tickets, AND journeys -- not merely trains.
 * Adding a dedicated CREATION page changes where a journey gets STARTED; it
 * doesn't change what this page LISTS, so renaming it here would describe a
 * change that didn't happen on this page. */
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
