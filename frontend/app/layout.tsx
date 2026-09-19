import '@/app/globals.css';
import { Suspense } from 'react';
import { ColorSchemeScript, mantineHtmlProps, Container } from '@mantine/core';
import type { Metadata, Viewport } from 'next';
import { AppNavBar } from '@/components/AppNavBar';
import { AutoRefresh } from '@/components/AutoRefresh';
import { ColorSchemeMeta } from '@/components/ColorSchemeMeta';
import { ServiceWorkerRegister } from '@/components/ServiceWorkerRegister';
import { OpenDataAttribution } from '@/components/OpenDataAttribution';
import { AppMantineProvider } from '@/components/AppMantineProvider';
import { ConnectivityMonitor } from '@/components/ConnectivityMonitor';
import { getChatbotAccess, getDataFreshness, getMyGroups, getSession } from '@/lib/api';
import { GroupSummariesProvider } from '@/lib/useGroupSummaries';
import type { DataFreshness, SessionInfo } from '@/lib/types';

// Site-wide fallback metadata, and still the live fallback for every route
// that has not overridden it (`/chat`, `/connect-claude`, and the smaller
// sub-pages -- `/lines/new`, `/groups/new`, `/track/mine` and friends --
// still inherit this wholesale; that is not an audit, just a pointer). A
// page that wants its own link-preview card overrides `title`/`description`
// and adds its own `openGraph`/`twitter`: the five detail routes do it via
// `generateMetadata` (see `app/train/[uid]/[date]/page.tsx` for the
// canonical shape), and the seven top-level pages -- `/`,
// `/incidents`, `/trains`, `/stations`, `/lines`, `/track` and `/groups` --
// via a static `export const metadata`. Note that Next merges these
// per-field, not per-object: a page that sets `title` but no `openGraph`
// inherits NOTHING into `og:title` (there is no `openGraph` here to
// inherit), which is exactly why each of those pages repeats the pair into
// all three slots.
//
// Deliberately no `metadataBase`: it exists only to resolve RELATIVE URLs
// in metadata into absolute ones, and nothing in this app emits a metadata
// URL at all -- no `openGraph.images`, no `openGraph.url`, no
// `alternates.canonical`, on any route (grep for those before assuming
// otherwise). With nothing to resolve, a `metadataBase` would be an
// unused, deploy-environment-specific hostname to keep correct, and this
// app has no configured public base URL to source one from
// (`NEXT_PUBLIC_RAILMCP_PUBLIC_URL` is the MCP server's address, not this
// site's). Add one here -- and only here -- the day a route gains an OG
// image or a canonical URL; Next will warn at build time if that day
// arrives and this is still absent.
export const metadata: Metadata = {
  title: 'Distant Signal',
  description:
    'A personal UK rail companion: TfL-style line status, live train tracking, and ticket/Delay-Repay support — with first-class handling of operators whose routes share trunk track, so an incident is only ever flagged on the lines it actually affects.',
  // `capable: false` is required, not redundant: Next's own
  // `resolveAppleWebApp` (node_modules/next/dist/lib/metadata/resolvers/
  // resolve-basics.js) defaults `capable` to `true` whenever `appleWebApp`
  // is set at all and the caller doesn't include a `capable` key -- so
  // omitting it here would *still* emit a `mobile-web-app-capable` meta
  // tag, the exact discouraged/rejected tag this plan's Global Constraints
  // say to never add. Verified empirically against a running `next start`
  // server this session: without this line, `mobile-web-app-capable`
  // appeared in the rendered <head> even though only `statusBarStyle` was
  // set.
  appleWebApp: {
    capable: false,
    statusBarStyle: 'black-translucent',
  },
};

// `colorScheme`'s 'light' is the same deterministic pre-mount fallback
// ThemeToggle.tsx's own useComputedColorScheme('light') call already uses —
// the server can't know a visitor's stored preference (see ThemeToggle.tsx's
// own comment on this), so this agrees with the one opinion the rest of the
// page already commits to rather than inventing a second one. ColorSchemeMeta
// (mounted in RootLayout below) keeps the resulting <meta name="color-scheme">
// tag's content in sync with the actually-resolved theme after mount. See
// docs/superpowers/specs/2026-09-01-dynamic-color-scheme-meta-design.md.
export const viewport: Viewport = {
  colorScheme: 'light',
  themeColor: [
    { media: '(prefers-color-scheme: light)', color: '#ffffff' },
    { media: '(prefers-color-scheme: dark)', color: '#242424' },
  ],
};

// The nav bar takes `freshness` as a prop rather than fetching it. The
// fetch lives in RootLayout below (awaited, not streamed) because its
// *success or failure* is this app's backend-reachability signal -- and a
// fetch inside a <Suspense> boundary resolves after RootLayout has
// already returned its JSX, so RootLayout could never read the outcome to
// pass to a sibling. See
// docs/superpowers/specs/2026-09-02-frontend-disconnect-reconnect-ux-design.md
// Decision 1 and its implementation plan's Correction 1.
//
// The cost, stated plainly: the nav-bar freshness tooltip no longer
// streams in -- RootLayout awaits it before emitting any HTML. Acceptable
// because the call is against the same in-cluster `api` service every
// page already awaits for its own content, and because in the failure
// case (the one this whole design exists for) we specifically need the
// outcome before first paint.

/** The shape `getSession()` returns for a visitor with no session, and
 * the fallback this layout degrades to when the session check fails
 * outright. Named once because it is used twice below — as
 * `NavBarWithSession`'s `.catch()` value and as the `<Suspense>`
 * fallback's session — and the two must agree: both mean "render the nav
 * as if logged out". */
const LOGGED_OUT_SESSION: SessionInfo = {
  authenticated: false,
  id: null,
  email: null,
  name: null,
};

// The nav's one session fetch, in a separate async Server Component so
// `<Suspense>` can stream the check in without blocking the rest of the
// shell, and so an uncaught fetch failure here (this root layout has no
// route-level `error.tsx`) can't take down every page. Falls back to a
// logged-out session rather than rethrowing — an auth-status glitch
// should degrade to "show the log in link", not break navigation for
// every visitor, logged in or not.
//
// ONE fetch, where there used to be two: the old `AuthNavItem` and
// `GroupsNavItem` each called `getSession()` behind their own
// `<Suspense>`, and `getSession()` is `cache: 'no-store'` (see
// `lib/api.ts`), so that really was two round trips to the same
// in-cluster `api` service on every page load. The account menu put both
// of those decisions — which control to show, and whether "Groups" is in
// the list — in one component, so one call now answers both.
//
// Unlike the freshness fetch above, this deliberately keeps its
// `<Suspense>`: it is not a connectivity oracle, and nothing sibling to
// it needs to read its outcome.
// Review §3.1.3: `/chat` was reachable only by typing the URL directly --
// nothing in the nav pointed an allow-listed user at it. Fetched
// concurrently with `session` (both are independent reads of the same
// in-cluster `api` service) rather than as a second sequential round trip,
// and inside the SAME `<Suspense>` boundary as the session check below, so
// this doesn't add a third wait of its own on top of the one the nav
// already streams past first paint. `getChatbotAccess()` never throws (it
// already fails closed to `'forbidden'` on any ambiguous or failed
// response -- see `lib/api.ts`), so unlike `session` this needs no
// `.catch()` of its own.
async function NavBarWithSession({ freshness }: { freshness: DataFreshness }) {
  const [session, chatAccess] = await Promise.all([
    getSession().catch(() => LOGGED_OUT_SESSION),
    getChatbotAccess(),
  ]);
  return <AppNavBar session={session} freshness={freshness} chatAllowed={chatAccess === 'allowed'} />;
}

/** Because the call below is awaited before RootLayout emits any HTML, an
 * unbounded one would hang *every* route for as long as the network takes
 * to give up. A refused connection fails instantly, so the common outage
 * is unaffected -- but a black-holed backend (a pod NotReady behind a
 * Service, a NetworkPolicy drop, a hung upstream) would otherwise stall
 * first paint on the OS TCP connect timeout, which is minutes. That is
 * strictly worse than before this fetch was hoisted, and in precisely the
 * failure mode this feature exists to handle, so the wait is bounded here.
 *
 * 2s: an order of magnitude above a healthy in-cluster round trip to the
 * same-network `api` service (single-digit to low-hundreds of ms), so it
 * cannot fire on a normal request or a brief GC pause; and low enough that
 * a broken backend costs one noticeable pause rather than a hung tab. A
 * freshness tooltip that takes longer than 2s to answer is not worth
 * holding first paint for -- timing out here is not a lost cause, it is
 * the "backend unreachable" signal, which is exactly what
 * ConnectivityMonitor needs and what the nav's "never fetched" fallback
 * already renders honestly.
 *
 * Also reused below for the `getMyGroups()` prefetch that hydrates
 * `GroupSummariesProvider`: same in-cluster `api` service, same "an
 * unbounded await here would hang every route" exposure, so the identical
 * reasoning applies verbatim -- a second, differently-named constant with
 * the same value would just be two numbers to keep in sync by hand. */
const FRESHNESS_TIMEOUT_MS = 2_000;

const UNAVAILABLE_FRESHNESS: DataFreshness = {
  stations: null,
  tocs: null,
  incidents: null,
  tfl: null,
  schedule_feed: null,
};

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  // A root layout has no route-level `error.tsx` boundary (that only
  // catches errors in child segments), so an uncaught fetch failure here
  // would take down every page rather than just one -- fall back to an
  // all-"never fetched" state instead. Unchanged in substance from the
  // previous `.catch()` on this same call; the only addition is that we
  // now also record *whether* it fell back, which is the
  // backend-reachability signal ConnectivityMonitor debounces.
  // Fired here, *before* the freshness `await` below, so the two run
  // concurrently rather than one serialized after the other -- this is a
  // second round-trip to the same in-cluster `api` service added to every
  // page load, and there is no reason to pay for it twice over. `.catch(()
  // => null)` right at the call site (rather than further down, at the
  // final `await groupsPromise`) is load-bearing, not stylistic: a rejected
  // promise nobody has attached a handler to yet triggers Node's
  // unhandledRejection warning the instant it rejects, regardless of when
  // it's later awaited -- attaching the handler here, synchronously, avoids
  // that regardless of how long `getDataFreshness` takes to settle first.
  // `null` on any failure (a timeout via the shared `FRESHNESS_TIMEOUT_MS`,
  // a network blip, a non-2xx from `errorForResponse`) is exactly
  // `getMyGroups()`'s own null-on-401 shape, and `GroupSummariesProvider`/
  // `useGroupSummaries()` already treat `null` as "nothing to offer" --
  // same fail-safe posture `useGroupSummaries` always had, just moved here.
  const groupsPromise = getMyGroups({ signal: AbortSignal.timeout(FRESHNESS_TIMEOUT_MS) }).catch(() => null);

  let freshness: DataFreshness;
  let backendReachable: boolean;
  try {
    freshness = await getDataFreshness({ signal: AbortSignal.timeout(FRESHNESS_TIMEOUT_MS) });
    backendReachable = true;
  } catch {
    freshness = UNAVAILABLE_FRESHNESS;
    backendReachable = false;
  }
  // By now `getDataFreshness` has already taken at least one in-cluster
  // round trip, so this is typically already settled -- not a second
  // sequential wait in practice, just picking up a result that was already
  // being computed alongside it.
  const groups = await groupsPromise;
  return (
    <html lang="en" {...mantineHtmlProps}>
      <head>
        <ColorSchemeScript defaultColorScheme="auto" />
      </head>
      <body>
        <a href="#main-content" className="skip-link">
          Skip to content
        </a>
        <AppMantineProvider>
          {/* Outside ConnectivityMonitor, not inside: the two contexts are
              independent of one another, and this ordering just keeps the
              server-fetched-data providers grouped together at the top of
              the tree rather than implying any dependency between them. */}
          <GroupSummariesProvider groups={groups}>
            {/* Wraps the whole shell rather than only <Container
                component="main">: the banner's fixed positioning is then not
                constrained by the content container, and app/error.tsx --
                which renders inside <Container component="main"> below --
                ends up a descendant, which is what lets it read the context
                and auto-recover. */}
            <ConnectivityMonitor
              backendReachable={backendReachable}
              observedAt={new Date().toISOString()}
            >
              <AutoRefresh />
              <ColorSchemeMeta />
              {/* RootLayout is a Server Component and re-executes on every
                  navigation and every AutoRefresh-triggered router.refresh() --
                  a fresh ISO timestamp here is what lets
                  ServiceWorkerRegister record "last successful load" purely
                  from receiving a new prop value; see that component's own
                  doc comment. */}
              <ServiceWorkerRegister loadedAt={new Date().toISOString()} />
              {/* The fallback is the WHOLE bar rendered logged-out, not a
                  placeholder: the brand, the burger and every primary link
                  are identical either way, so a visitor sees a complete,
                  usable nav from the first byte and only the account
                  control (and the "Groups" entry inside the drawer) swaps
                  when the session check lands. A `null`/skeleton fallback
                  would instead pop the entire header in, which is the one
                  thing a root layout must not do.

                  It also means the anonymous rendering is not a special
                  case that only real anonymous visitors exercise -- every
                  page load renders it, so it cannot quietly rot. */}
              <Suspense fallback={<AppNavBar session={LOGGED_OUT_SESSION} freshness={freshness} />}>
                <NavBarWithSession freshness={freshness} />
              </Suspense>
              {/* `component="main"`: Mantine's Container renders a plain
                  <div> by default, which left every page's actual content
                  outside any landmark -- axe's `landmark-one-main` fired on
                  every route tested, and `region` fired once per unlandmarked
                  node (487 on /lines alone). See
                  docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md.
                  The nav (now components/AppNavBar.tsx) and footer
                  (OpenDataAttribution.tsx) were already landmarked; only
                  the middle was not. Polymorphic
                  `component` swaps the tag only -- size/px/class output is
                  unchanged.

                  `flex: 1`: pairs with `body`'s `display: flex;
                  flex-direction: column; min-height: 100vh` in
                  globals.css to make this the one growable element in the
                  column, so the footer (OpenDataAttribution, rendered
                  right after this) is pushed to the bottom of the
                  viewport on a short-content page instead of hugging the
                  content -- see globals.css's comment on that `body` rule
                  for the full sticky-footer rationale.

                  `w="100%"`: without it, this Container shrink-wrapped to
                  its content's width instead of matching the nav
                  Container's (components/AppNavBar.tsx) identical
                  `size="lg" px={0}`, so a
                  page's content edge drifted from the nav's on every route
                  whose content didn't happen to be exactly 1140px wide --
                  confirmed against the installed
                  node_modules/@mantine/core/styles/Container.css: the
                  `[data-strategy='block']` rule this renders under sets
                  only `max-width` + `margin-inline: auto`, no `width`. A
                  flex item's `width:auto` normally stretches to fill the
                  cross axis (`body`'s default `align-items: normal`
                  computes to `stretch`), but the CSS Flexbox spec (and
                  Chromium/Firefox's actual behaviour, verified against a
                  running dev server) skips that stretch whenever the
                  item's cross-axis margins are auto -- exactly
                  `margin-inline: auto` here -- and falls back to
                  shrink-to-fit sizing instead, capped by `max-width`. That
                  auto-margin/stretch conflict is also why `align-self:
                  stretch` on `main` would NOT have fixed this: it hits the
                  identical spec carve-out and still doesn't stretch a
                  flex item with auto cross-margins. `w="100%"` gives the
                  Container an explicit (non-auto) width instead, which
                  `max-width: 1140px` then clamps exactly like the nav's,
                  with `margin-inline: auto` centering the clamped box --
                  matching the nav Container's box on every route. */}
              <Container id="main-content" component="main" size="lg" px={0} w="100%" style={{ flex: 1 }}>
                {children}
              </Container>
              <OpenDataAttribution />
            </ConnectivityMonitor>
          </GroupSummariesProvider>
        </AppMantineProvider>
      </body>
    </html>
  );
}
