# Design: Frontend Disconnect/Reconnect UX

**Status: design proposal, not approved.** Written to the same rigor and
citation discipline as `docs/superpowers/specs/2026-09-02-pwa-service-worker-design.md`
(the closest recent precedent — see Relationship to prior specs) and
`docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md` (the
closest recent precedent for a spec whose central mechanism is "add one
small piece of shared client state, consumed by several existing
components, without inventing a new state-management layer"). No
implementation is included; that is a separate, later step in this repo's
process.

## Goal

Per the task brief, verbatim: *"the frontend should still regularly try to
reconnect while disconnected. when disconnected the frontend should keep
displaying the stale state with a loading indicator or some sort of popup
which just indicates its disconnected and trying to reconnect (rather than
blanking out the whole page)."*

Concretely, this spec has to answer, for **this specific app's actual
architecture** (see Corrections and Current relevant state — it is not a
typical client-fetch SPA):

1. What "disconnected" means, precisely, and where that detection lives.
2. How already-displayed data stays on screen instead of being replaced by
   an error page when a background refresh fails.
3. What the retry/backoff behaviour is, including tab-visibility handling.
4. What the non-blocking "reconnecting" indicator looks like and where it's
   mounted.
5. Whether "no data yet" (first load, backend already down) and "stale data
   already showing" need different treatment.

## Relationship to prior specs

- **`2026-09-02-pwa-service-worker-design.md`** already established the one
  fact this entire design has to be built around, and this spec re-verifies
  it against the live tree rather than re-deriving it: **this app has no
  discrete, browser-visible "live status" fetch.** Every read of line/
  station/incident/freshness data happens server-side, inside React Server
  Components (`frontend/lib/api.ts`'s `baseUrl()` reads a server-only
  `API_BASE_URL` env var, never exposed to the browser). The browser
  receives that data only two ways: embedded in a navigation's full HTML
  response, or inside the serialized RSC payload `AutoRefresh.tsx`'s
  `router.refresh()` call triggers every 30s. That spec's own Decision 1
  explicitly rejected caching or stale-labelling any navigation/RSC
  response at the **service-worker** layer, specifically because this
  app's SSR model gives no seam to show "shell from cache, content live" —
  and it scoped "any live-data caching" as entirely out of that spec.
  **This spec is not a reopening of that decision.** It addresses a
  different failure domain: that spec's service worker sits between the
  *browser* and *this app's own Next.js frontend server*, and only fires
  when that hop itself fails (genuine no-network, or the frontend pod
  unreachable) — handled today by `offline.html`. **This spec is about the
  frontend server being reachable but the upstream Rust `api` service (the
  thing the frontend server itself talks to) being unreachable or erroring**
  — a failure the service worker cannot see or help with at all, since
  from the browser's perspective the HTTP round-trip to the frontend server
  still completes normally (whatever it responds with, including a 500 or
  a rendered error page). The two designs are complementary, not
  overlapping: one covers "no path to Distant Signal at all," this one
  covers "Distant Signal is up but its own backend isn't."
- **`2026-09-02-modal-login-prompt-design.md`** is the most recent example
  of this repo's working pattern for "one small piece of shared client
  state, consumed by multiple existing call sites, surfaced through a thin
  presentational component" (`useNeedsLogin()` + `LoginPromptModal`) — the
  shape Decision 3/4 below follow for connectivity state, and its
  `useMounted()`-gated, root-layout-mounted, side-effect-only component
  convention (`AutoRefresh`/`ColorSchemeMeta`/`ServiceWorkerRegister`) is
  what the new component in this spec also follows.

## Corrections to the brief's assumptions

Following this repo's own established "Corrections" convention (see the
two specs above, and `2026-08-30-inferred-time-ranges-design.md` before
them): the brief's framing, read literally, assumes a client-fetch
architecture (a data-fetching library with built-in retry/cache-on-error,
`navigator.onLine`-style detection of "the backend," a client-side polling
loop that can be paused/resumed independently of page rendering). None of
that is what this app actually has, verified by direct inspection this
session, not assumed:

1. **There is no SWR, React Query, or any client-side data-fetching
   library in this codebase.** `frontend/package.json`'s full dependency
   list is nine packages: the four `@mantine/*` packages actually used
   (`charts`, `core`, `dates`, `dropzone`, `hooks`), `dayjs`,
   `isomorphic-dompurify`, `next`, `react`, `react-dom`, `recharts` — plus
   `@anthropic-ai/sdk`/`@modelcontextprotocol/sdk` for the chatbot feature.
   No cache-with-retry-on-error library exists to "already handle" the
   stale-data-on-error behaviour the brief hints at (`docs/superpowers/specs/2026-09-02-pwa-service-worker-design.md`'s
   own survey confirms the same nine runtime dependencies, re-confirmed
   here). Whatever "keep the stale data, don't blank the page" behaviour
   this spec designs has to be built from what's actually here — Next's
   Server/Client Component split, `router.refresh()`, and plain React
   state — not layered onto an existing cache.
2. **"The backend" the brief means is not one thing the browser ever talks
   to directly for reads.** Line status, incidents, tracked trains,
   freshness — all fetched server-side. The browser's *only* direct paths
   to the Rust `api` service are: (a) mutations and the OIDC flow, through
   `frontend/app/api/[...path]/route.ts`'s proxy, and (b) one existing
   read, `PinToggle.tsx`'s `GET /api/preferences`, through the same proxy.
   So `navigator.onLine`/the browser's online/offline events tell you
   nothing about whether the Rust `api` service specifically is reachable
   — they only reflect this device's own network adapter, which is a
   materially different, complementary signal (see Decision 1).
3. **Next.js's `error.tsx` boundary does not self-heal on a later
   `router.refresh()` at the same URL — verified by reading the framework
   source, not assumed.** `frontend/node_modules/next/dist/client/components/error-boundary.js`'s
   `ErrorBoundaryHandler.getDerivedStateFromProps` only clears a tripped
   error state when `props.pathname !== state.previousPathname` — i.e. only
   on a navigation to a *different* route. `AutoRefresh.tsx`'s
   `router.refresh()` never changes the pathname. **This means that once a
   page's error boundary trips (a background refresh's data fetch throws),
   `AutoRefresh`'s ongoing 30s refreshes do nothing to recover it — the
   visitor is stuck on `app/error.tsx`'s "Couldn't load status data" card
   until they manually click "Try again" (which calls the boundary's own
   `reset()`) or navigate away and back.** This is the concrete mechanism
   behind the brief's "blanking out the whole page" complaint, and it
   directly rules out the naive fix of "just keep calling
   `router.refresh()` and it'll sort itself out once the backend's back" —
   see Decision 6.
4. **The failure that actually blanks the page happens narrower than "the
   whole page," but still reads as exactly what the brief describes.**
   `app/error.tsx` is a route-segment boundary — it wraps `{children}` in
   `RootLayout` (`app/layout.tsx:195`), not `RootLayout` itself. The nav
   bar, `AutoRefresh`, `ColorSchemeMeta`, and `ServiceWorkerRegister` are
   all siblings of `{children}` in `RootLayout`'s own JSX, so they keep
   working (and `AutoRefresh` keeps quietly calling `router.refresh()`,
   per point 3, uselessly) even while the *page content* area is replaced
   by the error card. Worth naming precisely rather than either
   overclaiming "the entire page" or underclaiming "just a small widget" —
   from a visitor's point of view, the entire content area still goes
   blank-then-error, which is exactly what they're describing.

## Current relevant state (verified 2026-09-02, this session)

- **`AutoRefresh.tsx`** (`frontend/components/AutoRefresh.tsx`, read in
  full, 24 lines): `'use client'`, calls `router.refresh()` on a
  `useInterval(..., 30_000, { autoInvoke: true })` from `@mantine/hooks`.
  No error handling of any kind (`router.refresh(): void`, no promise to
  catch). No visibility-awareness — it refreshes on a background tab
  exactly as often as a foreground one. Mounted once in `RootLayout`
  (`app/layout.tsx:127`), alongside `ColorSchemeMeta`/`ServiceWorkerRegister` —
  the established "side-effect-only, renders `null`, mounted once at root"
  shape all three, plus this spec's new component, share.
- **`app/error.tsx`** (read in full, 21 lines): `'use client'`, a plain
  `Stack`/`Title`("Couldn't load status data")/`Text`(dimmed,
  `error.message`)/`Button`("Try again", calls the injected `reset`).
  No existing `error.test.tsx` (confirmed by directory listing — this is
  currently untested). No retry logic, no connectivity awareness, no
  auto-recovery — a human has to click.
- **`frontend/lib/api.ts`** (460 lines, read in full): `fetchJson<T>()` is
  the one place a non-`ok` response becomes an exception
  (`errorForResponse`, lines 53–61) — maps 404/401/403 to
  `ApiNotFoundError`/`ApiUnauthorizedError`/`ApiForbiddenError`, anything
  else (5xx, or the generic `Error` a rejected `fetch()` throws for a
  genuine network failure — Node's `fetch` throws a `TypeError: fetch
  failed` with a `cause`) becomes a plain `Error`. Nothing in this file
  currently distinguishes "the request reached `api` and it 5xx'd" from
  "the request never reached `api` at all" — both surface as the same kind
  of thrown `Error`, which is fine for this spec's purposes (both mean
  "can't get live data right now").
- **Most page-level live-data fetches are unguarded and will throw all the
  way to `app/error.tsx`.** Three concrete, representative call sites, read
  in full:
  - `app/page.tsx:99` (the home page): `getLineStatusForMode(DISPLAYED_MODES_PARAM)`
    inside an unguarded `Promise.all([getPreferences(), getLineStatusForMode(...),
    getMyTrackedTrains()])` (lines 95–100) — no `.catch()`, no `try`. A
    backend outage here throws straight through `DashboardPage` to
    `app/error.tsx`.
  - `app/lines/page.tsx:10-14`: same shape, `getLineStatusForMode` inside
    an unguarded `Promise.all`.
  - `app/lines/[id]/page.tsx:28-36`: `getLineStatus([id], true)` **is**
    already inside a `try`/`catch` — but only to special-case
    `ApiNotFoundError` into `notFound()` (line 33); every other error,
    including a network failure, is explicitly `throw err;` (line 35) and
    still reaches `app/error.tsx`. This is useful precedent: any fix has to
    compose with this existing catch, not replace it (see Decision 4).
  - By contrast, `app/layout.tsx`'s own `DataFreshnessNavItem` (lines 58–70)
    and `AuthNavItem` (lines 79–87) **already** wrap their fetches in
    `.catch()` and degrade to a fallback value, with an explicit comment
    explaining why: *"A root layout has no route-level `error.tsx`
    boundary... an uncaught fetch failure here would take down every
    page."* This is the one place in the app that already treats "the
    backend didn't answer" as a non-fatal, silently-degraded outcome —
    and, not incidentally, it's also the one fetch this spec reuses as its
    connectivity oracle (Decision 1).
- **`frontend/app/api/[...path]/route.ts`** (155 lines, read in full): its
  `proxy()` function's own upstream call, `const response = await
  fetch(target, init)` (around line 106), has **no `try`/`catch` at all**.
  If the Rust `api` service is unreachable, this `fetch()` rejects, the
  route handler throws, and Next.js's own Route Handler machinery turns
  that into an unhandled-exception `500` response to whatever
  same-origin client called `/api/...`. This means a client-side
  `fetch('/api/...')` reliably distinguishes "browser can't even reach
  this Next.js server" (a browser-level network failure — see Decision 1)
  from "this Next.js server is up but the thing it proxies to failed" (a
  `500`, or occasionally a 502/504-shaped upstream status forwarded
  through unchanged) — without this spec needing to add any error handling
  to the proxy itself.
- **`@mantine/hooks@9.5.2` (already an installed, pinned dependency) ships
  `useNetwork()` and `useDocumentVisibility()`, unused anywhere in this
  codebase today** (confirmed: `grep -rn "useNetwork\|useDocumentVisibility"
  frontend/` has zero hits outside `node_modules`). Read both in full from
  `node_modules/@mantine/hooks/esm/`:
  - `useNetwork()` returns `{ online: boolean, ... }`, backed by the
    `online`/`offline` `window` events plus an initial
    `navigator.onLine` read — exactly `navigator.onLine`-style detection,
    with no polling.
  - `useDocumentVisibility()` returns `'visible' | 'hidden'`, backed by the
    `visibilitychange` event — the Page Visibility API the brief asks
    about, already available with no new dependency.
  Both are safe to adopt with **zero new packages**, which matters given
  this repo's repeatedly-stated preference (see the PWA spec's Decision 2,
  the modal spec's reuse of existing primitives) for reusing what's already
  a dependency over adding one.
- **No toast/notification manager is installed.** `@mantine/notifications`
  (the separate package providing `<Notifications />` + `notifications.show()`)
  is not in `package.json`. `@mantine/core`'s own `Notification` component
  (a plain, static, presentational card — icon, title, body, close button,
  and a built-in `loading` spinner state) **is** available, since it ships
  inside `@mantine/core` itself, not the separate notifications package —
  confirmed present at `node_modules/@mantine/core/esm/components/Notification/`.
  No existing fixed-position/toast-shaped element exists anywhere in
  `frontend/app/globals.css` (`grep -n "position: fixed"` finds nothing) —
  this spec's banner is the first.
- **The `useMounted()`-gated SSR pattern is this codebase's established way
  of handling browser-only state that would otherwise mismatch between the
  server's pre-hydration render and the client's real value** — used by
  `PrideToggle.tsx`, `ThemeToggle.tsx`, `ServiceWorkerRegister.tsx`,
  `ColorSchemeMeta.tsx`, and `LastUpdated.tsx` (all read in full or cited
  above). Connectivity state is exactly this shape: the server has no
  concept of "is this visitor's browser online" or "did the last client-side
  probe succeed," so the same pattern applies directly — see Decision 8.
- **No existing "offline"/"disconnected"/"reconnect"/"stale" UI exists
  anywhere in the live-data path.** A repo-wide grep for those words across
  `frontend/lib`, `frontend/components`, `frontend/app` (excluding tests)
  turns up only unrelated hits (offline-*.html* naming in the PWA spec's
  own files, an unrelated `IssueList.tsx` "network error" mention in a
  comment about a different concern, `ChatPanel.tsx`'s own chat-request
  error copy, `useSuggestions.ts`'s autocomplete abort handling). Nothing
  today tells a visitor "the site itself thinks it's disconnected."
- **The Helm chart's `frontend.replicaCount` (`charts/distant-signal/values.yaml:1083`)
  defaults to `1`, but — unlike `postgresql.replicaCount` a few hundred
  lines above it, which carries an explicit comment declaring it
  intentionally single-instance with a stated reason (a migration
  advisory-lock argument) — there is no equivalent documented constraint
  against an operator raising `frontend.replicaCount`.** This matters for
  Decision 5's in-memory cache and is called out plainly in Open
  questions/risks rather than assumed away.

## Decisions

### 1. Two independent connectivity signals — browser-offline vs. backend-unreachable — not one merged check

**Chosen: track both, and treat either as sufficient to show the
"disconnected" banner, but keep them as two separately-sourced booleans
under the hood rather than one fetch that tries to detect both.**

- **Browser-offline**: `useNetwork().online` from `@mantine/hooks` — purely
  event-driven (`online`/`offline` window events + initial
  `navigator.onLine`), no polling, effectively instant. This is the
  brief's "the browser itself is offline" case. It cannot detect "the Rust
  `api` service specifically is down" (Corrections #2) — a visitor can be
  fully online while `api` itself is unreachable, and `useNetwork()` would
  report `online: true` throughout.
- **Backend-unreachable**: piggybacked on `app/layout.tsx`'s existing
  `getDataFreshness()` call (`DataFreshnessNavItem`, lines 58–70) — this
  fetch already runs, unconditionally, on **every** `RootLayout` render
  (every hard navigation, and every `AutoRefresh`-triggered
  `router.refresh()`, since `RootLayout` re-executes on both, per
  `ServiceWorkerRegister.tsx`'s own doc comment confirming exactly this).
  Today it swallows any failure into an all-`null` fallback
  (`.catch(() => ({ stations: null, ... }))`) with no visibility into
  *why* it fell back. This spec's only change to that call site is to also
  capture whether the call succeeded (a `try`/`catch` around the existing
  logic, not a new fetch) and pass that boolean down as a prop — the same
  "thread a fact the Server Component already knows down to a Client
  Component as a prop" shape `RootLayout` already uses for
  `ServiceWorkerRegister`'s `loadedAt` (`app/layout.tsx:135`).
  **This adds zero new network requests in the common case** — it reuses a
  fetch that already exists for an unrelated reason (the nav-bar freshness
  tooltip), rather than introducing a second poll against the same
  backend.

**Alternative considered and rejected: a dedicated client-side health-check
fetch (e.g. `fetch('/api/freshness')` through the existing proxy) on its
own interval, independent of `AutoRefresh`/`RootLayout`.** This was the
first design sketched (see the proxy's already-clean error semantics in
Current relevant state) and it would work — but it doubles backend
traffic in the steady state (one probe on its own timer, plus
`getDataFreshness()` already firing every `RootLayout` render for the nav
tooltip, checking the same underlying fact twice) for no benefit over
reusing the one that's already there. Kept as the fallback design if
Decision 4's reliance on `AutoRefresh`'s render cycle to keep re-checking
connectivity turns out to be too coarse in practice (see Open
questions/risks).

### 2. Debounce rule: two consecutive backend-probe failures to declare "disconnected," one success to clear it

**Chosen, asymmetric on purpose:** a single failed `getDataFreshness()`
call does not flip the banner on — it takes **two consecutive** failed
`RootLayout` renders (i.e. two consecutive failed `router.refresh()`
cycles, ~60s apart at the normal cadence) before the backend-unreachable
signal is treated as real. Recovery is the opposite: **one** successful
render immediately clears it. This directly answers the brief's own
framing ("N consecutive failed fetches... to avoid flapping on a single
transient blip") — a single dropped request (a brief GC pause, a
transient connection reset) shouldn't flash a banner at all, but the app
should never *delay* telling a visitor good news once it's true.

`useNetwork()`'s browser-offline signal gets **no debounce** — the
`online`/`offline` browser events are already debounced at the OS/browser
network-stack level (they don't fire on every packet loss), and there's no
"maybe transient" reading of a browser explicitly telling you its network
adapter went down.

The two-strikes counter lives in the new client component from Decision 3,
as a plain `useState` incrementing/resetting on each new `reachable` prop
value it receives — no timer of its own; it only ever changes in step with
`RootLayout`'s own render cadence (Decision 4 explains why that's the
right cadence to hang this off of, rather than a faster dedicated timer).

### 3. New component: `ConnectivityMonitor`, mounted in `RootLayout` alongside `AutoRefresh`/`ColorSchemeMeta`/`ServiceWorkerRegister`

**Chosen:** one new Client Component, `frontend/components/ConnectivityMonitor.tsx`,
following the exact established shape of its three siblings (side-effect
component + this spec's addition of actually rendering something, since
unlike its siblings it owns the visible banner — see Decision 8 for why
that's still the right place for it rather than a second new component).
It:

- Receives `backendReachable: boolean` as a prop from `RootLayout`
  (Decision 1's threaded fact), the same way `ServiceWorkerRegister`
  receives `loadedAt`.
- Consumes `useNetwork()` directly (no prop needed — this is genuinely
  client-only information with no meaningful server-computed equivalent).
- Holds the two-strikes counter (Decision 2) and derives one boolean,
  `disconnected`, from `!online || backendUnreachable`.
- Renders the banner (Decision 8) when `disconnected` is true, `null`
  otherwise.

**Why one component owning both detection and rendering, rather than a
hook (`useConnectivity()`) plus a separate presentational
`ConnectivityBanner`:** the modal-login-prompt spec's own precedent
(`useNeedsLogin()` + `LoginPromptModal`) deliberately *does* split
state from presentation — but that split exists there because
`useNeedsLogin()` has **five separate call sites**, each owning its own
instance of the state, reset independently per form/button. This spec's
connectivity state has **exactly one instance for the whole app**,
mounted once in `RootLayout` like `AutoRefresh` — there's no second
consumer that would ever need the hook half without the component half.
Splitting it into a hook + a presentational component with no second
caller would be speculative generality this app's own conventions don't
otherwise practice (`AutoRefresh`/`ColorSchemeMeta`/`ServiceWorkerRegister`
are each a single fused component too, not a hook-plus-view pair).

**How `AutoRefresh` and `app/error.tsx` (Decision 6) learn the same
`disconnected` state without a new global store:** a small React Context,
`ConnectivityContext`, created alongside `ConnectivityMonitor` and provided
from the same place it's mounted (`AppMantineProvider`'s children in
`app/layout.tsx`, wrapping `AutoRefresh`/`{children}`/etc., not a new
top-level provider file) — exposing just `{ disconnected: boolean }`. This
is the smallest new piece of shared state this design needs, and it's
scoped narrowly (one boolean, one producer, two consumers:
`AutoRefresh` for Decision 4's pause-on-hidden-tab logic doesn't
actually need it, only Decision 6's `app/error.tsx` does — see below). A
plain module-level mutable variable plus a manual subscriber list was
considered and rejected as reinventing what Context already does for one
value with one producer.

### 4. Retry cadence: reuse `AutoRefresh`'s existing 30s loop; add tab-visibility pausing (new); do not speed up during an outage

**Chosen: no new timer for "retrying."** `AutoRefresh`'s existing 30s
`router.refresh()` **is** the retry loop — every tick re-runs
`RootLayout`, which re-runs `getDataFreshness()`, which is Decision 1's
connectivity probe. Once it succeeds again, `ConnectivityMonitor`'s next
`backendReachable` prop flips to `true` and the banner clears on the very
next render, satisfying "regularly try to reconnect while disconnected"
with the cadence the app already has, rather than inventing a second,
faster interval that would just mean two independent timers hitting the
same backend on two different schedules.

**Deliberately not sped up while disconnected.** A faster retry interval
during an outage was considered (a common pattern for this kind of
feature) and rejected for two reasons specific to this app: (a) per
Corrections #3, a faster `router.refresh()` cadence doesn't actually fix
the thing that's broken — the *page-level* fetch still throws to
`app/error.tsx` on every attempt until Decision 5's fallback exists there
too, so refreshing faster just means hitting an already-down backend
harder for no improvement in outcome; (b) this is a small, likely
single-instance, personal-scale deployment (Current relevant state) —
there's no load-shedding infrastructure in front of `api` that a faster
client-side retry storm would be protecting against, so there's no upside
to weigh against the downside of extra load on a service that's already
struggling.

**New: `AutoRefresh` gains `useDocumentVisibility()`-based pausing —
genuinely new behaviour, not present today, added because the brief asks
for it directly ("pause when backgrounded... to avoid wasted requests")
and there's no reason not to regardless of connectivity state.**
`useInterval`'s own implementation (read in full,
`node_modules/@mantine/hooks/esm/use-interval/use-interval.mjs`) restarts
cleanly whenever its `interval`/`active`-driving inputs change (its effect
lists `interval` as a dependency and re-runs `stop()`/`start()`), so
`AutoRefresh` can call `useInterval(..., 30_000, { autoInvoke:
document.visibilityState === 'visible' })`-shaped logic — concretely,
gate the interval's own `start()`/`stop()` on `useDocumentVisibility()`'s
value via a small `useEffect`, and, on transitioning back to `'visible'`,
call `router.refresh()` once immediately (don't wait up to 30s for the
first tick) so a visitor returning to a backgrounded tab sees current data
right away rather than staring at whatever was on screen when they left.
This pausing applies **unconditionally**, independent of `disconnected` —
it's a general efficiency fix the brief calls out on its own merits, not
gated behind connectivity state.

### 5. Keeping stale data on screen: an opt-in `withStaleFallback()` cache, applied per data source, not globally

This is the load-bearing decision — everything else in this spec is
detection and UI; this is the piece that actually stops `app/error.tsx`
from replacing a page's content.

**Chosen:** a new module, `frontend/lib/liveDataCache.ts`, exporting:

```
withStaleFallback<T>(key: string, fetcher: () => Promise<T>): Promise<T>
```

Internally: a process-local `Map<string, { data: unknown; at: number }>`.
On success, store the result under `key` and return it. On failure: if a
cached entry exists **and is younger than a TTL** (Decision 6), return the
cached `data` silently — the caller never sees the throw at all. If no
entry exists, or the cached one is past the TTL, **rethrow** — there is
nothing honest to show, so this falls through to whatever the call site's
own error handling already does (today, `app/error.tsx`; Decision 7 makes
that page itself connectivity-aware too, so this isn't a dead end even in
the worst case).

Call sites opt in explicitly, keyed by something that disambiguates their
actual request (mirroring how each fetcher's own arguments already vary):

```
// app/page.tsx / app/lines/page.tsx
const allReports = await withStaleFallback(
  `lineStatusForMode:${DISPLAYED_MODES_PARAM}`,
  () => getLineStatusForMode(DISPLAYED_MODES_PARAM),
);

// app/lines/[id]/page.tsx — composes with the existing ApiNotFoundError catch
let reports;
try {
  reports = await withStaleFallback(`lineStatus:${id}`, () => getLineStatus([id], true));
} catch (err) {
  if (err instanceof ApiNotFoundError) notFound();
  throw err;
}
```

Note the composition in the second example: `withStaleFallback` only
swallows a failure when it has a *usable stale value to substitute* — an
`ApiNotFoundError` (a real 404, not a connectivity failure) still isn't
something a stale cache entry should paper over on a *first* request for
that `id` (there's nothing cached to fall back to, so it rethrows
immediately per the "no entry exists" branch above), and even on a
*repeat* request `ApiNotFoundError` specifically should probably not be
served from a stale cache at all (a line that's since been deleted
shouldn't keep showing its last-known status forever) — implementation
should have `withStaleFallback` re-throw `ApiNotFoundError`/
`ApiUnauthorizedError`/`ApiForbiddenError` unconditionally rather than
ever treating them as "transient, fall back to stale," since those three
are meaningful application states (`lib/api.ts`'s own doc comments), not
connectivity failures. Only the generic `Error` fallback case (network
failure, 5xx, timeout) should ever trigger the stale substitution.

**Deliberately NOT applied to every fetch in `lib/api.ts` — an explicit
allowlist of read-only "live status" call sites, not a blanket wrapper
around `fetchJson`.** Three concrete reasons a call site should *not* opt
in, each with a real example already in this codebase:

- **Anything session/auth-shaped** (`getSession()`) — silently serving a
  stale "you are logged in" (or logged out) state on a connectivity blip
  is a correctness/security problem, not a staleness inconvenience. Stays
  exactly as it is today (`AuthNavItem`'s own `.catch()` degrading to
  "treat as logged out," which is the *safe* direction to fail in, unlike
  stale-serving a cached "authenticated" state).
- **Anything mutation-shaped or write-adjacent** — `getMyTrackedTrains()`,
  ticket data, custom line ownership — showing a stale list after a
  connectivity blip risks a visitor acting on data that's since changed in
  a way that matters (a ticket they deleted still appearing, a train
  they've since stopped tracking still listed). Read-only, low-stakes
  "which lines currently have Good Service" is a very different risk
  profile from "here is your ticket/tracking data."
- **`getStationName()`** and other genuinely-cacheable reference data
  (`next: { revalidate: 3600 }`, `lib/api.ts:110-121`) — already has its
  own, better-fitting caching story (an hour-long Next.js data cache
  window); wrapping it in this spec's cache would be redundant, not
  additive.

**Scope for this spec's first pass**, matching the brief's own examples
("line status, incidents, train tracking, notifications"): `getLineStatusForMode`,
`getLineStatus`, `getStopPointDisruption`, `getAllLines` — the read-only
status surface the home page, `/lines`, `/lines/[id]`, and `/stations/[crs]`
all build on. `/incidents/[id]`, the history/trends pages, and the chat
panel are explicitly deferred (Explicitly out of scope) — this is a real
scoping call for whoever plans the implementation, not an oversight; see
Open questions/risks for the honest cost of the full rollout.

### 6. The "no data yet" case: `app/error.tsx` becomes connectivity-aware and auto-retries

Per Corrections #3, once a page's error boundary trips, `AutoRefresh`'s
continued 30s refreshes do nothing to clear it — the pathname hasn't
changed, so `ErrorBoundaryHandler` never resets. This matters for exactly
the case Decision 5 can't cover: a **first-ever** request for something
(no stale entry to fall back to), or the TTL having lapsed on a long
outage, or a call site not yet migrated (Decision 5's scope is
deliberately partial). This is the brief's own named second case — "a page
that has no data yet at all" — and it needs its own answer, not just "hope
Decision 5 always has something cached."

**Chosen:** `app/error.tsx` (already `'use client'`) reads
`ConnectivityContext`'s `disconnected` boolean (Decision 3) and:

- Replaces the current static "Try again" framing with connectivity-aware
  copy while `disconnected` is true — "Couldn't load status data. Trying
  to reconnect…" with the same non-blocking-style spinner treatment as the
  banner (Decision 8), rather than a dead-end error card.
- **Auto-calls the boundary's own injected `reset()` the moment
  `disconnected` flips back to `false`** (a `useEffect` keyed on that
  value), rather than requiring the visitor to notice and click "Try
  again" themselves. This directly closes the gap Corrections #3 found in
  Next's own reset semantics: the boundary still won't reset itself on a
  same-pathname refresh, but this page-level component now does that job
  explicitly, using the exact signal (`disconnected: false`) that means
  "the next `router.refresh()` is worth attempting."
- Keeps the manual "Try again" button too, for a failure that **isn't**
  connectivity-shaped at all (a genuine application bug in a Server
  Component, unrelated to `api` being reachable) — `disconnected` can be
  `false` while `error.message` still describes a real thrown error, and a
  human should still be able to force a retry in that case. The
  auto-retry is additive, not a replacement for the existing affordance.

**Why this lives in `app/error.tsx` itself rather than, say,
`ConnectivityMonitor` calling `router.refresh()` on every reconnect (which
would also eventually clear a tripped boundary once the pathname-based
exception above didn't apply)**: `ConnectivityMonitor` calling
`router.refresh()` on reconnect only helps a *future* refresh render
successfully — per Corrections #3, the *existing* tripped boundary
instance doesn't care that a new successful RSC payload exists unless
something calls its own `reset()`. `reset()` is a prop only the boundary's
own `errorComponent` (`app/error.tsx`) receives — there's no way for a
sibling component elsewhere in the tree to call it. `app/error.tsx` is
inherently the only place this fix can live.

### 7. Not adding a `Suspense` fallback/loading-indicator layer for the connectivity feature itself

Considered whether `ConnectivityMonitor`/`app/error.tsx`'s reconnecting
state should route through Next's `loading.tsx` convention instead of
plain component state. **Rejected**: `loading.tsx` is Next's mechanism for
"this route segment's *initial* data fetch is still in flight" (a
`Suspense` boundary around the page itself) — it has no concept of "a
background refresh of an *already-rendered* page failed and is retrying,"
which is this spec's actual scenario. Nothing here needs a new
`loading.tsx` file; the existing `<Suspense fallback={...}>` boundaries
around `DataFreshnessNavItem`/`AuthNavItem` in `app/layout.tsx` are
unrelated (they cover streaming-in nav-bar data on first render, not
reconnect behaviour) and are left untouched.

### 8. UI treatment: a fixed, non-blocking Mantine `Notification`, bottom-of-viewport, `useMounted()`-gated

**Chosen:** `ConnectivityMonitor` renders a `@mantine/core` `Notification`
(the plain presentational component — Current relevant state confirmed
this ships in `@mantine/core` itself, no `@mantine/notifications`
dependency needed), wrapped in a small fixed-position container:

```
<div style={{ position: 'fixed', bottom: 16, left: '50%', transform: 'translateX(-50%)', zIndex: 300 }}>
  <Notification
    loading
    withCloseButton={false}
    title="Reconnecting…"
    role="status"
    aria-live="polite"
  >
    Distant Signal can&apos;t reach the server right now — showing the last data it had.
  </Notification>
</div>
```

- **Bottom-center, not a top banner.** The nav bar already owns the top of
  every page (`app/layout.tsx`'s `<Box component="nav">`, with its own
  border and fixed-height content) and several pages render their own
  page-level alerts near the top of their content (`Alert` usages in
  `TicketEntryForm`/`TrackTrainForm`/`DelayRepayEstimate`/etc.). A
  top-anchored global banner would compete with both for the same
  vertical real estate and would need to push page content down whenever
  it appeared/disappeared — exactly the "disrupting the rest of the page"
  the brief explicitly asks to avoid. Fixed at the bottom, it overlays
  without reflowing anything else.
- **`loading` (Mantine's built-in spinner state on `Notification`), not a
  custom icon** — this is literally the "loading indicator" half of the
  brief's own phrasing ("a loading indicator or some sort of popup"),
  satisfied by a prop the component already supports rather than new
  iconography.
- **`withCloseButton={false}`** — deliberately not dismissable. Per the
  brief, this should "clear automatically on reconnect," and a manually
  dismissable banner that a visitor closes while still genuinely
  disconnected would then have no way to know reconnection is even being
  attempted. This mirrors `app/error.tsx`'s existing precedent of no
  "dismiss and move on" affordance for a data-availability problem.
  the visitor didn't cause and can't fix by hiding it.
- **`role="status"`/`aria-live="polite"`** — a transient, non-modal status
  change that should be announced to assistive tech without stealing
  focus, matching the general accessibility posture
  `2026-09-02-frontend-accessibility-audit-research.md`/`-fixes.md`
  already established for this codebase (not re-litigated here, just
  applied).
- **`zIndex={300}`**, below `DataFreshnessInfo`'s tooltip (`zIndex={400}`,
  `DataFreshnessInfo.tsx`) and below Mantine's `Modal` default (which sits
  higher still) — chosen so it never visually competes with an open
  tooltip or the login/delete confirmation modals (`LoginPromptModal`,
  `DeleteLineButton`/`DeleteTrainButton`'s own `Modal`s) if one happens to
  be open at the same time; it should read as background chrome, not a
  competing overlay.

### 9. `useMounted()`-gating: mandatory, same reasoning as every other browser-only-state component in this codebase

**Chosen:** `ConnectivityMonitor` renders `null` until `useMounted()` is
`true`, exactly matching `PrideToggle`/`ThemeToggle`/`ServiceWorkerRegister`/
`ColorSchemeMeta`/`LastUpdated`. The server has no notion of
`navigator.onLine` or of whether a client-side probe has run yet — the
only honest SSR value is "no banner," the same "deterministic,
server/client-byte-identical pre-hydration render" constraint
`ThemeToggle.tsx`'s own comment documents for the identical class of
problem. Since `disconnected` starts `false` until the first
post-mount check completes, a visitor's very first paint never shows the
banner even if they happen to load the page while genuinely offline
(`useNetwork()`'s initial value requires a mount to read
`navigator.onLine`) — acceptable, since that same first paint would
already be failing to render real content in that scenario (Decision 6
covers it), and the banner appearing a moment later once `useMounted()`
flips is no different in kind from every other client-only indicator in
this app.

### 10. Applied uniformly (the banner) but not uniformly (the stale-data cache)

Direct answer to the brief's own question:

- **The banner (`ConnectivityMonitor`) is global and uniform** — mounted
  once in `RootLayout`, visible on every route, with no per-page opt-in.
  There's no reason a visitor on `/stations/[crs]` should get worse
  reconnect feedback than one on `/`.
- **The stale-data cache (Decision 5) is deliberately not uniform** — an
  explicit, reasoned allowlist of read-only status endpoints (see
  Decision 5's scope), not a property every page automatically gets. A
  page with "no data yet at all" (first load, nothing cached) and a page
  "showing stale data" genuinely need different treatment, per the
  brief's own framing — the former is Decision 6's job (an honest,
  auto-retrying error state), the latter is Decision 5's job (silently
  keep showing what's there). Both share the same underlying
  `disconnected` signal and the same banner, so the visitor sees one
  consistent story regardless of which of the two states their current
  page happens to be in.

## Architecture

```
frontend/
├── app/
│   ├── layout.tsx                MODIFIED:
│   │                                - DataFreshnessNavItem: try/catch
│   │                                  around getDataFreshness(), threads a
│   │                                  new `backendReachable: boolean` prop
│   │                                  (Decision 1) alongside the existing
│   │                                  freshness fallback -- no new fetch.
│   │                                - mounts <ConnectivityMonitor
│   │                                  backendReachable={...} /> alongside
│   │                                  AutoRefresh/ColorSchemeMeta/
│   │                                  ServiceWorkerRegister.
│   │                                - wraps children in the new
│   │                                  ConnectivityContext.Provider
│   │                                  (Decision 3).
│   └── error.tsx                 MODIFIED: reads ConnectivityContext,
│                                    connectivity-aware copy + auto-reset()
│                                    on reconnect (Decision 6).
├── components/
│   ├── AutoRefresh.tsx           MODIFIED: useDocumentVisibility()-gated
│   │                                pause/resume (Decision 4). No change
│   │                                to its own 30s interval value or its
│   │                                "renders nothing" contract.
│   └── ConnectivityMonitor.tsx   NEW: useNetwork() + backendReachable
│                                    prop -> two-strikes counter ->
│                                    `disconnected` -> provides
│                                    ConnectivityContext + renders the
│                                    fixed Notification banner (Decisions
│                                    1-3, 8-9).
├── lib/
│   ├── connectivity.ts           NEW (or co-located in
│   │                                ConnectivityMonitor.tsx if small
│   │                                enough at implementation time):
│   │                                ConnectivityContext + its Provider
│   │                                type.
│   └── liveDataCache.ts          NEW: withStaleFallback<T>() (Decision 5).
│                                    No dependency on React/Next -- plain
│                                    module-scope Map, importable from any
│                                    Server Component page.
├── app/page.tsx                  MODIFIED: getLineStatusForMode(...) call
│                                    wrapped in withStaleFallback (Decision
│                                    5's scoped first pass).
├── app/lines/page.tsx             MODIFIED: same.
├── app/lines/[id]/page.tsx        MODIFIED: getLineStatus(...) wrapped,
│                                    composed with the existing
│                                    ApiNotFoundError catch (Decision 5).
└── app/stations/[crs]/page.tsx    MODIFIED: getStopPointDisruption(...)
                                     wrapped (same pattern; not read in
                                     full this session, flagged for
                                     implementation-time verification).
```

No change to `frontend/app/api/[...path]/route.ts`, `frontend/lib/api.ts`'s
existing exports (only new call-site usage, no signature changes), or
`frontend/public/sw.js`/`offline.html` — this design sits entirely above
those layers.

## Error handling

- **`withStaleFallback()`'s own failure modes**: if the in-memory `Map`
  somehow holds a value whose shape no longer matches what the caller
  expects (a hypothetical future API shape change while an old cached
  value is still within its TTL) — not treated as a realistic risk worth
  designing around now; the cache is populated exclusively by the same
  typed fetcher functions that would otherwise be called directly, so a
  shape mismatch would only occur alongside a genuine `lib/api.ts` type
  change, which is a broader migration concern unrelated to this spec.
- **A connectivity flap during the two-strikes window (Decision 2)**: a
  single transient failure never surfaces at all — by design, this is
  what the debounce exists to absorb, not an error case to handle.
- **`ConnectivityContext` read outside its provider** (a future page
  rendered without going through `RootLayout` — not currently possible in
  this App Router structure, since every route renders inside it):
  default the context's value to `{ disconnected: false }` rather than
  `undefined`/throwing, so a hypothetical future consumer degrades to "no
  banner" rather than crashing — same fail-safe posture Decision 5 already
  takes for auth-shaped data.
- **`app/error.tsx`'s auto-`reset()` racing a still-broken page** (backend
  flickers reachable for one `getDataFreshness()` call, Decision 6 fires
  `reset()`, but the actual page's own data fetch fails again
  immediately): the boundary simply trips again on the next render,
  showing the same connectivity-aware fallback — no special-casing needed,
  this is just the two-strikes counter (Decision 2) doing its job on the
  next cycle.
- **`useNetwork()` in a browser with no `navigator.onLine` support at all**
  (extremely old browsers): Mantine's own implementation (read in full,
  Current relevant state) already defaults `status.online` to `true` when
  `navigator.onLine` isn't a boolean — this app inherits that "assume
  online" fail-open default for the browser-offline half of Decision 1,
  same as Mantine's own documented behaviour; not a new risk this spec
  introduces.

## Testing

Following this repo's established conventions (`renderWithMantine`,
Vitest, colocated `*.test.tsx`, fake timers for interval-driven components
— `AutoRefresh.test.tsx` is the direct precedent for testing this spec's
`AutoRefresh` changes):

- **`ConnectivityMonitor.test.tsx`** (new): renders nothing before
  `useMounted()` flips (mock or real timer flush, matching
  `LastUpdated.test.tsx`'s existing pattern for the same gate); shows the
  banner after two consecutive `backendReachable={false}` prop updates,
  not after one; clears immediately on the next `backendReachable={true}`;
  shows the banner immediately (no two-strikes delay) when a mocked
  `useNetwork()` reports `online: false`; `role="status"`/`aria-live`
  present when rendered.
- **`AutoRefresh.test.tsx`** (extend the existing file): add cases for
  the new `useDocumentVisibility()` gating — no `router.refresh()` calls
  while mocked visibility is `'hidden'`; an immediate `router.refresh()`
  call on transitioning back to `'visible'`, in addition to the existing
  "calls on a 30s interval"/"stops once unmounted" assertions, which
  should still pass unchanged for the visible-tab case.
- **`liveDataCache.test.ts`** (new, plain Vitest, no React involved —
  `withStaleFallback` has no framework dependency): first call with a
  throwing fetcher and no cached entry rethrows; a subsequent call with a
  succeeding fetcher populates the cache and returns fresh data; a later
  call with a throwing fetcher and a fresh cached entry returns the stale
  value silently; the same with an entry older than the TTL rethrows
  instead of serving it. Each test needs to reset the module's internal
  `Map` between cases (an exported test-only `__resetForTests()` or a
  fresh `vi.resetModules()` import per test, matching whichever pattern
  this repo's other module-scope-state tests already use — worth checking
  at implementation time whether one already exists to follow).
- **`app/error.tsx` gains its first test file, `error.test.tsx`** (none
  exists today — Current relevant state): asserts the existing "Try
  again" button still calls `reset`; asserts connectivity-aware copy
  appears when `ConnectivityContext` reports `disconnected: true`; asserts
  `reset` is auto-called when the context's `disconnected` value flips
  from `true` to `false` across a rerender.
- **`app/layout.test.tsx`**: extend `DataFreshnessNavItem`'s existing
  coverage (mirroring how the modal-login-prompt spec's Testing section
  updated this same file for `TrackedTrainsNavItem`) to assert
  `backendReachable={false}` is threaded to `ConnectivityMonitor` when
  `getDataFreshness()` rejects, and `true` when it resolves — via
  whatever `vi.mock('@/lib/api')` shape this file already uses for
  `getDataFreshness`/`getSession`.
- **End-to-end, in `frontend/e2e/` (Playwright, configured but currently
  empty — per the PWA spec's own Current relevant state, still true)**:
  this is the one layer that can genuinely simulate "the backend is
  reachable from the Next.js server's perspective but returns errors,"
  which is awkward to fake purely client-side. Using Playwright's route
  interception (`page.route()`) against the *frontend's own* origin isn't
  quite right either, since the failure this spec cares about happens
  server-side, inside the Next.js process's own `fetch()` to `api` — the
  most faithful e2e setup would run against a real `docker compose`
  environment (this repo already has one, per other specs' references to
  it) with the `api` container stopped mid-test, navigate, assert the
  banner appears within the two-strikes window, restart `api`, assert the
  banner clears and stale content was visible throughout. Flagged as
  valuable but the most involved test in this list, same honesty the PWA
  spec's own Testing section already applied to its hardest-to-simulate
  case.

## Explicitly out of scope

- **Applying `withStaleFallback` to every live-data fetch in the app.**
  Decision 5 scopes the first pass to four call sites (home page, `/lines`,
  `/lines/[id]`, `/stations/[crs]`). `/incidents/[id]`, the history/trends
  pages (`TrendsResults`/`HalfHourlyTrendsResults`), tracked-train pages,
  and the chat panel are not touched here — a real scoping decision for
  implementation planning, not an oversight (see Open questions/risks).
- **Any change to `getSession()`, ticket data, or tracked-train data's
  error handling.** Decision 5 explicitly excludes these categories on
  correctness grounds; they keep their current fail-closed behaviour.
- **Redesigning `app/error.tsx`'s visual layout beyond adding
  connectivity-aware copy and auto-retry.** Its existing
  `Stack`/`Title`/`Text`/`Button` structure and styling are untouched.
- **A dedicated backend health-check endpoint** (e.g. a cheap
  `/public/health` distinct from `/public/freshness`). Decision 1
  deliberately reuses the existing freshness fetch rather than asking the
  Rust `api` service for a new route — if `getDataFreshness()` ever turns
  out to be a poor proxy for "is `api` reachable" in practice (e.g. it
  gets its own independent caching added later that would mask a real
  outage), that would be the trigger to revisit this, not something to
  pre-empt now.
- **Changing `AutoRefresh`'s 30s cadence value itself**, in either
  direction, connected or disconnected. Decision 4 keeps it exactly as-is;
  only the new visibility-based pause/resume is added.
- **Any interaction with `frontend/public/sw.js`/`offline.html`.** Per
  Relationship to prior specs, that layer handles a different failure
  domain (browser can't reach the frontend server at all) and needs no
  changes for this spec's failure domain (frontend server reachable,
  backend behind it isn't).
- **Differentiated copy between "you're offline" and "having trouble
  reaching the server."** Decision 8's Notification uses one message
  regardless of which of Decision 1's two signals tripped it — named
  explicitly as a simplification, not an oversight; see Open
  questions/risks.
- **Push-notification or any other cross-tab/cross-device connectivity
  awareness.** This is purely a per-tab, client-side concern; no
  interaction with the line-status-notifications spec's push
  infrastructure.

## Open questions/risks

1. **`frontend.replicaCount` defaulting to `1` with no documented
   guard against raising it (Current relevant state) is an assumption
   Decision 5's in-memory cache depends on for consistent behaviour.** If
   an operator scales the frontend Deployment horizontally, each pod
   would hold its own independent `withStaleFallback` cache — a visitor
   could see different staleness behaviour (or a bare error where another
   pod would have served something stale) depending on which pod a
   request lands on. Not a correctness bug (each pod's cache is still
   internally consistent), but worth flagging for whoever owns the Helm
   chart's documented deployment story, the same way this spec flagged it
   rather than silently assuming single-instance.
2. **The stale-data TTL's actual value is not chosen here** — Decision 5
   names the need for a cutoff but leaves the number to implementation.
   Something on the order of minutes (rail status can meaningfully change
   within 5–10 minutes) rather than the PWA spec's completely different
   "never serve anything stale, ever" posture (that spec's Decision 1,
   for a genuinely different failure domain — see Relationship to prior
   specs) — a first guess of 10 minutes is reasonable but not verified
   against any real outage-duration data, since this app has no existing
   incident history to calibrate against.
3. **Relying on `AutoRefresh`'s existing 30s cadence as the reconnect
   probe interval (Decision 4) means up to ~30s of "still disconnected"
   before the *next* check even starts, plus Decision 2's two-strikes
   delay before the banner first appears** — roughly a 60–90s worst-case
   window between an outage starting and the banner appearing, and up to
   30s between the backend recovering and the banner clearing. If this
   reads as too sluggish in practice, the fallback design in Decision 1's
   "alternative considered and rejected" (a dedicated, faster client-side
   probe) is the documented next step, not a redesign from scratch.
4. **Decision 6's `reset()` auto-call assumes `app/error.tsx` receiving a
   fresh `ConnectivityContext` value is enough to trigger a meaningful
   re-render at the right time** — this wasn't verified against a running
   dev server this session (unlike the pathname-reset finding, which came
   from reading the framework source directly). Worth a live check at
   implementation time that a Context value change inside an already-
   error-tripped subtree actually reaches `app/error.tsx`'s own
   `useEffect` the way this design assumes, given `ErrorBoundaryHandler`
   is a class component sitting between the provider and this file in the
   tree.
5. **Scope of Decision 5's rollout (four call sites now, more later) is a
   real, named implementation-planning decision, not resolved here** — a
   later phase extending `withStaleFallback` to `/incidents/[id]` and the
   trends/history pages is anticipated but not designed in detail; those
   pages' own data shapes and existing error-handling (if any) weren't
   read in full this session the way the four in-scope pages were.
6. **One unified banner message for both connectivity signals (Decision 8,
   Explicitly out of scope) may read as slightly wrong for the pure
   browser-offline case** ("Distant Signal can't reach the server" while
   the actual problem is the visitor's own device) — accepted as a
   reasonable simplification for a first pass, revisit if user feedback
   says otherwise.
