# Design: Service Worker and Offline Caching

**Status: design proposal, not approved.** This is the deliberately-deferred
follow-on work both `docs/superpowers/specs/2026-09-01-pwa-support-research.md`
and `docs/superpowers/specs/2026-09-01-pwa-manifest-design.md` explicitly
scoped **out** — not an oversight, a planned next step, now commissioned for
real. The manifest spec's own "Explicitly out of scope" section: *"Any
service worker, offline caching, or push notifications — carried forward
unchanged from the research doc's own explicit out-of-scope list; not
re-litigated here."* This document is where that gets picked up. Written to
the same rigor and citation discipline as those two documents.

## Relationship to prior specs

- **`2026-09-01-pwa-support-research.md`** already did the hard survey work
  this design builds on directly: it identified the exact architectural
  hazard this whole feature has to respect (§1, on `AutoRefresh.tsx`'s 30s
  polling and the `/api/[...path]` proxy), sketched three service-worker
  options (Option A: none: Option B: static-assets-only, network-only for
  `/api/*`; Option C: full offline caching, rejected), and recommended
  Option B *only if* a real trigger for it appeared. That trigger is now
  this task. This spec re-verifies the research's findings against the
  current tree (nothing had drifted) and goes one step further than the
  research did: Option B's own sketch was written before this session's
  fresh finding that this app's live data is **not** a discrete
  browser-visible JSON endpoint the way the research's `/api/*` framing
  implied — see Current relevant state and Decision 1.
- **`2026-09-01-pwa-manifest-design.md`** shipped in full since the research
  doc was written (see below) — `app/manifest.ts`, `public/icon-192.png`,
  `public/icon-512.png` all exist now. This spec treats that as the
  finished installability layer and adds the caching/offline layer on top
  of it, reusing its icons as precache candidates (Decision 5) and
  discovering, in the process, a real pre-existing gap in how those icons
  are deployed (Current relevant state, last bullet) that this spec's own
  new static asset (`sw.js`) would inherit if left unfixed.

## Current relevant state (verified 2026-09-02, this session)

- **The manifest spec has shipped**, unlike when the research doc was
  written. `frontend/app/manifest.ts` (read in full) returns exactly the
  object the manifest spec designed: `icons: [{ src: '/icon-192.png', ... },
  { src: '/icon-512.png', ... }]`. `frontend/public/icon-192.png` and
  `frontend/public/icon-512.png` both exist (confirmed by directory
  listing, `git log --oneline -- frontend/public/` shows one commit,
  `86b26b6`). `frontend/app/error.tsx` (read in full, 21 lines) is a small,
  reusable visual precedent this spec leans on directly (Decision 3): a
  plain `Stack`/`Title`("Couldn't load status data")/`Text`(dimmed,
  `error.message`)/`Button`("Try again", calls `reset`) — the app's
  existing tone for "something's wrong with live data, here's a retry."
- **This app has no discrete, browser-visible "live status" fetch for a
  service worker to intercept — this is the central architectural fact
  this whole design has to work around.** Every read of line/station/
  incident/freshness data happens **server-side**, inside React Server
  Components, via `frontend/lib/api.ts`'s `baseUrl()` (`lib/api.ts:33-36`),
  which reads `process.env.API_BASE_URL` — a server-only env var, never
  exposed to the browser. `frontend/app/page.tsx:3-10` imports
  `getLineStatusForMode`/`getMyTrackedTrains`/`getPreferences`/`getSession`/
  `getStationName`/`getStopPointDisruption` directly, all called during SSR.
  The browser never issues a `fetch('/api/lines')`-shaped request for this
  data at all — it only ever receives it two ways: (1) embedded in the
  initial full-document HTML response for a page navigation, and (2) inside
  the serialized React Server Component payload `AutoRefresh.tsx`'s
  `router.refresh()` call triggers every 30s (`components/AutoRefresh.tsx:19-22`,
  `REFRESH_INTERVAL_MS = 30_000`) — an App Router client-side refetch of the
  *current route*, not a call to any `/api/*` path. A service worker
  designed around "treat `/api/*` as network-only, cache everything else"
  (the research doc's own Option B framing) would be **necessary but not
  sufficient**: it would correctly leave the *mutation* proxy alone but say
  nothing about these document/RSC-refresh requests, which are what
  actually carry live status data and which a naive "cache everything not
  `/api/*`" fallback rule would wrongly scoop up.
- **Every content page already independently, explicitly opts out of
  Next's own server-side caching** — this is strong existing precedent for
  "never treat this app's live data as cacheable," now inherited by the
  service-worker layer too, not a new idea this spec is introducing.
  `frontend/app/page.tsx:26`, `frontend/app/lines/page.tsx`,
  `frontend/app/lines/[id]/page.tsx:19`,
  `frontend/app/lines/[id]/history/page.tsx:17`,
  `frontend/app/incidents/[id]/page.tsx:16`, and others all set `export
  const revalidate = 0` (confirmed by grep across `frontend/app`); a
  comment at `page.tsx:21-25` explains the immediate motivation is avoiding
  a `next build`-time prerender failure (the `api` service isn't reachable
  at build time) rather than being framed explicitly as a freshness
  decision, but the practical effect is identical: every one of these
  routes is fully dynamic, per-request, never cached at the Next.js layer.
  `AutoRefresh.tsx:11-13`'s own doc comment separately confirms the
  underlying fetches themselves are `cache: 'no-store'` — this app has
  already, independently, converged on "no server-side staleness window for
  status data" everywhere a service worker would now sit downstream of.
- **`frontend/app/api/[...path]/route.ts`** (read in full, 155 lines): this
  proxy is used only for client-initiated **mutations and the OIDC flow**,
  confirmed by a repo-wide grep for `fetch('/api` /`fetch("/api` outside
  test files: `LogoutButton.tsx:22` (`POST /api/auth/logout`),
  `TrackTrainForm.tsx:91` (`POST /api/Train/track`),
  `CustomLineForm.tsx:108` (`POST`/`PUT /api/lines...`),
  `PinToggle.tsx:64,84` (`GET /api/preferences`, `POST
  /api/preferences/pinned-*`). Every `Set-Cookie` the backend sends is
  relayed unmodified (`route.ts:113,121-123,131-133`) and OIDC redirects are
  forwarded with `redirect: 'manual'` (`route.ts:91`, comment at
  `route.ts:10-25`) so the *browser* follows them with its own session, not
  this server. `PinToggle.tsx:64`'s `GET /api/preferences` is the one read
  that does go through this proxy — it's session-cookie-gated user
  preference state, not live train/line status, but the same
  cookie-relay/non-cacheability argument applies to it for a different
  reason (Decision 1).
- **`frontend/components/AutoRefresh.tsx`** and **`frontend/components/
  ColorSchemeMeta.tsx`** (both read in full) are this app's existing,
  established pattern for "a side-effect-only Client Component, rendering
  `null`, mounted once in `RootLayout` alongside its siblings" —
  `AutoRefresh.tsx`'s own doc comment literally uses that phrase.
  `layout.tsx:129-130` mounts both directly inside `<MantineProvider>`.
  This is the pattern Decision 4 reuses for service-worker registration —
  not a new mounting convention.
- **`frontend/components/DataFreshnessInfo.tsx`** and **`frontend/
  components/LastUpdated.tsx`** (both read in full) are this app's existing
  "how fresh is this" concept, and the one Decision 2/3 explicitly extend
  rather than duplicate. `DataFreshnessInfo` renders four `LastUpdated` rows
  (stations/TOCs/incidents/TfL) inside a nav-bar tooltip, sourced from
  `getDataFreshness()` (`lib/api.ts:243-247`, `GET /public/freshness`,
  `cache: 'no-store'`) — server-side, same as everything else in this
  section. `LastUpdated.tsx:33-40`'s key mechanic: it stores the real ISO
  timestamp, not a pre-computed string, and recomputes `relativeTime(date,
  new Date())` at render time via a 30s `useInterval` tick
  (`RELATIVE_TIME_TICK_MS`, `LastUpdated.tsx:9,38`) — a timestamp captured
  once stays accurate as time passes, because the "now" side of the
  subtraction is always live. This exact mechanic (a captured timestamp +
  a client-side "time since" recomputation) is what Decision 3's offline
  page reuses conceptually.
- **No `<head>` link/meta already points at a service worker, and no
  `sw.js`/`serwist`/`next-pwa`/`workbox` reference exists anywhere in
  `frontend/`** — confirmed by a repo-wide grep. `frontend/package.json`
  (read in full) still lists exactly the same nine runtime dependencies the
  research doc catalogued (`@mantine/*` four packages, `dayjs`,
  `isomorphic-dompurify`, `next`, `react`, `react-dom`, `recharts`) plus
  `recharts`, with `next: "^16.2.10"`, `react`/`react-dom: "^19.2.0"` — no
  drift since the research doc's own reading. No `esbuild`/`workbox-*`/
  `serwist` dev dependency either.
- **`frontend/next.config.mjs`** (read in full, 39 lines): only
  `allowedDevOrigins` (dev-only) and one `redirects()` entry
  (`/track/tickets` → `/track/mine`). No `headers()` function exists yet —
  Decision 5 adds one.
- **This app is genuinely self-hosted via `next start`, not a static
  export, and nothing sits in front of it that would override response
  headers.** `frontend/Dockerfile`'s `runtime-prod` stage (read in full)
  runs `ENTRYPOINT ["npm", "run", "start"]` against a `next build` output —
  no `output: 'export'` anywhere in `next.config.mjs`. `charts/
  distant-signal/templates/` (grepped for `cache-control`/`Cache-Control`/
  `proxy-cache`/`CDN`) has no hits — no ingress-level caching layer that
  would strip or override a `Cache-Control` header this spec's Decision 5
  adds. This matters because it means Decision 5's `headers()`-based
  no-cache rule for `/sw.js` will actually reach the browser unmodified.
- **A real, pre-existing gap this spec's own new static asset would
  otherwise inherit: `frontend/Dockerfile`'s `runtime-prod` stage never
  copies `public/` into the deployed image, even though `public/` now
  exists and is load-bearing.** `Dockerfile:74-81` copies `.next`,
  `node_modules`, `package.json` from `builder-prod`, then has a comment —
  *"No `public/` directory: this plan adds no static assets... If a later
  change adds static assets under `frontend/public/`, add `COPY
  --from=builder-prod --chown=frontend:frontend /app/public ./public` back
  in at that point"* — that was true when written but is now **stale**:
  `frontend/public/icon-192.png` and `icon-512.png` were added by the
  manifest spec's implementation (commit `86b26b6`) and this COPY line was
  never restored. In `next dev` (used for all local testing) `public/` is
  read directly off disk, so this gap is invisible there; in the actual
  `runtime-prod` container, both manifest icons are almost certainly
  **already 404ing today**, undetected because nothing currently checks
  for them at runtime. This spec's new `public/sw.js` (Decision 2) would
  silently inherit the exact same fate — a service worker that can't even
  be *fetched* can't register — so restoring that COPY line is a hard
  prerequisite for this spec's implementation, not an optional cleanup.
  Flagged here as a verified, real, pre-existing bug this design surfaces
  as a side effect of investigating deploy behaviour for Decision 5, not
  something this spec invents new risk by introducing.
- **No `frontend/e2e/` directory exists yet.** `frontend/playwright.config.ts`
  (read in full) points `testDir` at `./e2e` and is otherwise fully set up
  (chromium project, `webServer` pointing at `next dev`,
  `test:e2e` script in `package.json`) but the directory itself has zero
  files in it — this repo has Playwright wired up and unused. Relevant to
  Testing (below): there's no existing e2e pattern to extend, but also no
  existing pattern to conflict with.
- **No sibling push-notification spec exists yet.** A directory listing of
  `docs/superpowers/specs/` found no `2026-09-02-line-status-*-design.md`
  or similarly-named file as of this session — the concurrently-commissioned
  push-notification spec mentioned in this task's brief had not landed by
  the time this document was written. Decision 4 is written so it doesn't
  need to wait for it.

## Decisions

### 1. Caching strategy — the central risk: default-deny, allowlist-only; never cache or fallback-serve live content, only the static shell

**Chosen: a strict allowlist of cacheable URLs — Next's content-hashed
static build output (`/_next/static/*`), the two manifest icons
(`/icon-192.png`, `/icon-512.png`), the manifest document
(`/manifest.webmanifest`), and one static offline fallback page
(`/offline.html`, Decision 3) — cached cache-first. Every other request,
with no exceptions, is passed straight to the network with no caching of
the response at all.** Not network-first-with-cache-fallback for page
content; not stale-while-revalidate anywhere; not a blanket "cache
everything except `/api/*`" rule.

This is a **default-deny** design, not a default-allow-with-exclusions
design, and that distinction is the whole point: Current relevant state
above establishes that this app's live status data does not travel through
one identifiable URL pattern (like a REST endpoint) that a blocklist could
reliably exclude — it arrives as full-document HTML on navigation and as
serialized RSC payloads on `router.refresh()`, both same-origin GET
requests to ordinary page paths (`/`, `/lines/[id]`, etc.) that are
otherwise indistinguishable, at the URL level, from a request for a static
page. A blocklist-shaped rule ("cache everything except `/api/*` and
known-dynamic paths") would need to correctly enumerate every current and
future dynamic route by path, forever — a single new page added without
updating the SW's blocklist would silently start getting cached. An
allowlist has the opposite failure mode: forgetting to add a new *static*
asset just means it isn't cached (a performance miss, caught immediately
in testing), never that live content gets cached by omission. Given this
spec's own framing of "never serve stale live-status data as fresh" as the
central risk, fail-safe (default-deny) is the correct posture over
fail-open (default-allow), and it is also robust to a fact this session
could not fully verify: the exact header/marker shape Next 16.2's App
Router uses to tag an RSC-refresh `fetch` (Open questions/risks #1) — an
allowlist doesn't need to recognize and specifically exclude that request
shape, because it was never going to match the allowlist's static-path
patterns in the first place, whatever headers it carries.

**The specific alternative this spec's own brief asked to weigh —
network-first with fallback to a cached copy plus an explicit staleness
indicator — was investigated seriously and rejected for full page content,
for a reason specific to this app's SSR/RSC architecture, not a generic
objection to the pattern.** That pattern works cleanly when "data" and
"shell" are separable — a SPA fetching a discrete `GET /api/status.json`
can cache *that response* with a `cached-at` timestamp and render a banner
around it. This app has no such seam at the network-request level: the
"data" (each line's status badge, an incident's description) and the
"shell" (nav bar, page chrome) are the same HTML document, produced by one
server render. Caching a navigation response as a same-shape fallback and
overlaying a "you're viewing a snapshot from HH:MM" banner is *technically
buildable*, but the actual line-status badges and incident text — the part
that matters, and the part this whole feature exists to keep accurate —
would still be the dominant visual content of the page, with a banner as a
secondary, dismissable, scrollable-past annotation. That is precisely the
"stale cache showing 'everything fine' when there's a real live disruption"
failure this task named as the central risk, softened by a banner but not
eliminated by one. Rejected specifically because this app's SSR model gives
no reliable, revalidatable way to show *only* the shell from cache while
genuinely withholding the *content* — unlike, say, a news app rendering
article body and chrome as separately-fetched, separately-labelled pieces.
**Instead: on a genuine network failure for a navigation/RSC request, show
one static, generic, unmistakably-not-live offline page (Decision 3) — not
a reconstruction of any previously-viewed real content, however
labelled.** This is the more conservative of the two options this task
asked to weigh, chosen deliberately: an offline visitor gets nothing of
their previously-viewed status pages at all, rather than something that
could be mistaken for current information even briefly. Named honestly in
Open questions/risks as a real UX cost, not hidden.

**`PinToggle.tsx:64`'s `GET /api/preferences` read is excluded from
caching for a related but distinct reason**: this proxy relays
`Set-Cookie` and reads the incoming `Cookie` header per-request
(`route.ts:76-79`), and the browser's Cache Storage API has no built-in
concept of "this cached response is scoped to this user's session" — a
cached response under `/api/preferences` would be replayed to *whatever
cookie jar is active on a later request*, which is wrong for session-scoped
data regardless of whether it's "live" in the train-status sense. Since
this request already falls outside the static-asset allowlist by URL
(`/api/*` never matches `/_next/static/`, `/icon-*.png`, `/manifest.
webmanifest`, or `/offline.html`), it needs no separate rule — the
allowlist's default-deny posture already covers it, which is itself
evidence the allowlist design generalizes correctly beyond just the
live-status case it was designed around.

**Considered and rejected: naive cache-first for everything not
explicitly excluded** (the shape a generic PWA tutorial/template
defaults to). This is exactly Option C from the research doc, and the
harm is concrete and specific to this app: a visitor who loaded the home
page once, then re-opens the (now installed, standalone) app later with
degraded or no connectivity, would see a cache-first service worker
happily serve yesterday's "Good Service" badges as the current page with
no indication anything was wrong — actively worse than showing nothing,
for an app whose entire value proposition is "the status shown is
current." Rejected outright, not weighed as a close call.

### 2. Library vs. hand-rolled: hand-rolled, no new runtime dependency

**Chosen: a hand-written service worker (`frontend/public/sw.js`, plain
JavaScript, no build/bundle step for the file itself) plus one small,
already-precedented build-time script for cache-busting (Decision 5) — no
`next-pwa`, no `@ducanh2912/next-pwa`, no Serwist, no Workbox.**

The research doc already did the tooling survey this decision rests on:
`next-pwa`'s upstream repo is archived; **Serwist** (`@serwist/next` +
`serwist`) is its actively-maintained, App-Router-compatible successor,
integrating via `withSerwistInit` in `next.config.mjs` and a source
`app/sw.ts` compiled to `public/sw.js`. Re-weighed fresh for this specific
design, not just carried forward:

- **Serwist's core value-add — Workbox-style runtime-caching *strategies*
  (cache-first, network-first, stale-while-revalidate) applied by route
  pattern, plus build-time precache-manifest injection for a large static
  surface — is a poor fit for exactly the part of this app that carries
  the most risk.** Decision 1's allowlist needs to be *narrow and
  legible*: a handful of exact/prefix URL matches, all statically
  knowable, checked in one small function. Workbox's routing model is
  built around composing named strategies across potentially many route
  matchers — powerful for an app with a large, varied static+API surface,
  but for this app it's more machinery than the actual cache surface (four
  URL patterns) needs, and every additional configured route is one more
  place a live-content path could accidentally get matched by a
  broader-than-intended rule. A hand-rolled allowlist has no such surface:
  the entire caching decision is one small, directly-readable function,
  not a configuration object whose effective behaviour depends on rule
  ordering and specificity.
- **This app has no existing precedent for a build-pipeline-integrating
  framework plugin**, confirmed again this session (package.json, current
  relevant state above) — nine runtime dependencies, all either Mantine's
  own suite or narrowly-scoped single-purpose libraries (`dayjs`,
  `recharts`, `isomorphic-dompurify`), and `next.config.mjs` has never
  needed a `with*Init`-shaped wrapper. Serwist would be a new *category* of
  dependency for this codebase (a build-time-integrated framework plugin),
  not an incremental one — the same conclusion the research doc already
  reached, re-confirmed rather than assumed.
- **Forward-compatibility for the sibling push-notification spec (this
  task's item 4) favours hand-rolled too, if only slightly.** Serwist does
  let a project add its own listeners inside the source `app/sw.ts` file
  alongside its generated precaching setup, so it isn't strictly
  incompatible with a later `push`/`notificationclick` handler — but a
  hand-rolled file has no generated-vs-hand-written seam to navigate at
  all: a future push handler is just another `self.addEventListener(...)`
  block in the same plain file, with nothing to learn about how Serwist's
  own generated code is laid out around it.
- **The actual amount of hand-rolled logic needed is genuinely small.**
  Decision 1's allowlist matcher, an `install` handler that precaches five
  known URLs, an `activate` handler that purges old cache-name generations
  (Decision 5), and a `fetch` handler that branches on the matcher and
  either serves cache-first or passes through to network — this is the
  same "~30-40 lines, no new dependency" estimate the research doc already
  made for its own Option B sketch, still accurate now that the actual
  URLs and rules are concrete rather than illustrative.

**What "hand-rolled" costs, stated honestly, not glossed over**: this app
now owns precache-list maintenance (adding a new static asset means adding
one line to `sw.js`'s precache array) and cache-generation invalidation
logic (Decision 5) that Serwist would otherwise generate automatically from
Next's own build manifest. For this app's narrow, slow-changing static
surface (two icons, one manifest, one offline page, plus the `/_next/
static/` prefix which needs no per-file enumeration at all — see Decision
5), this is a small, one-time cost, not an ongoing maintenance burden
proportional to app growth.

**The one small new devDependency this design does introduce is not a
service-worker library — Decision 5's build-id-stamping script needs no
dependency beyond Node's own `fs` module (reads `.next/BUILD_ID`, string-
replaces a placeholder in `sw.js`), matching this repo's existing
precedent of small, dependency-free one-off Node scripts (the manifest
spec's own `sharp`-based icon-generation script).**

### 3. What "offline" means for this app: the shell loads and shows an unmistakable "you're offline" state — not full offline read/write

**Confirmed, not just assumed: "offline" here means "a visitor with no
network connection gets a clear, branded, non-misleading page telling them
so, with a manual retry — not a stale copy of real content, and not any
queued/offline mutation capability."** This matches the task's own framing
and Decision 1's reasoning directly: since no cached copy of real status
content is ever served (Decision 1), there is nothing for an "offline
mode" to do beyond present that fact honestly.

**Chosen: `frontend/public/offline.html`, a single static file with fully
inlined CSS — no dependency on `_next/static/*` chunks, no Mantine, no
React runtime.** Deliberately reuses `app/error.tsx`'s existing tone and
structure (Current relevant state: heading, one line of dimmed
explanatory text, one retry button) translated into plain HTML/CSS rather
than JSX, specifically so it does not depend on anything else being
precached correctly to render — this is the one page in the entire app
that must render even if every other precache entry is somehow missing or
corrupted (a fresh install where precaching hasn't completed yet, a
partially-failed `install` event, browser storage pressure that evicted
part of the cache). A Next-rendered `/offline` App Router page was
considered and rejected for this reason: it would look pixel-identical to
the rest of the app, but every path to serving it while genuinely offline
still depends on its own `_next/static` JS/CSS chunks being present in the
precache, reintroducing exactly the kind of multi-piece dependency this
one page is supposed to be immune to. The visual cost (a plain-HTML page
that doesn't exactly match Mantine's styling) is accepted deliberately,
not overlooked.

**Reuses the `LastUpdated`/`DataFreshnessInfo` *concept* — a captured
timestamp plus a live "time since" recomputation — without reusing the
component itself (a framework boundary neither side can cross: this page
has no React runtime, `LastUpdated` is a Client Component).** A tiny
vanilla-JS snippet inline in `offline.html` reads a `lastSuccessfulLoadAt`
ISO timestamp from `localStorage` (written on every successful navigation/
`router.refresh()` — see Architecture) and renders "Last connected around
X ago," recomputed the same way `LastUpdated.tsx:34-40` does (a stored
absolute timestamp, "now" evaluated at render/tick time, not baked in) —
deliberately the same information shape this app's visitors already
recognize from `DataFreshnessInfo`'s nav-bar tooltip, applied to a new
context (session-level "am I connected at all," not per-source "how fresh
is this specific data") rather than inventing an unrelated new pattern.
If `localStorage` has no value yet (first-ever visit, offline before any
successful load), the line is simply omitted — no fabricated fallback
timestamp.

**Explicitly not designed: queuing a mutation (ticket upload, pin toggle,
custom line edit) made while offline for later replay.** Named directly
in this task's brief as the alternative to weigh, and rejected for the
same reason the research doc already gave and re-confirmed this session:
this app's write paths are all synchronous, session-cookie-gated mutations
through `/api/[...path]` (Current relevant state), and none of Background
Sync's browser support gaps have changed in a way that would make this
newly viable — see Explicitly out of scope.

### 4. Registration and lifecycle: a side-effect-only Client Component, mounted at root scope, deliberately shaped for a later `push` handler

**Chosen: `frontend/components/ServiceWorkerRegister.tsx`, mounted in
`RootLayout` alongside `AutoRefresh`/`ColorSchemeMeta` (`layout.tsx:129-130`),
feature-detecting `'serviceWorker' in navigator` and calling
`navigator.serviceWorker.register('/sw.js')` inside a `useEffect` gated on
`useMounted()`** — the exact same "side-effect-only, renders null, mounted
once at root" shape `AutoRefresh.tsx`/`ColorSchemeMeta.tsx` already
establish, not a new mounting convention. No new provider, no new context.

**`register('/sw.js')` with no explicit `scope` option is a deliberate
choice, not an omission**: registering from a root-level path gives the
service worker the maximum possible scope (`/`, the whole origin) by
default — this matters now (Decision 1's allowlist logic needs to see
every navigation across the whole app to correctly pass live content
through) and matters more for the sibling push-notification spec later: a
`notificationclick` handler that needs to focus-or-open a specific
in-app URL (e.g. deep-linking to the affected line's page) via
`clients.matchAll()`/`clients.openWindow()` needs a registration whose
scope already covers that URL. Narrowing scope now to reduce blast radius
was considered and rejected — there's no part of this app Decision 1's
allowlist needs to be hidden from, and a narrower scope now would just be
undone by the sibling spec later.

**Why this SW's shape is already push-ready, stated explicitly per this
task's forward-compatibility ask**: `install`/`activate`/`fetch` are
independent `addEventListener` blocks in one flat file (Decision 2); a
later `self.addEventListener('push', ...)` and `self.addEventListener(
'notificationclick', ...)` are two more such blocks, touching none of the
existing three. The registration call itself (`.register('/sw.js')`)
already returns the `ServiceWorkerRegistration` object whose `.pushManager`
a later `subscribe()` call would use — nothing about *this* spec's
registration code needs to change for that to work; only new code needs to
be added elsewhere (the subscribe-button UI, the backend subscription
storage — the sibling spec's job, not this one's).

**Update-detection is left to the browser's native mechanism, not
polled from application code**: the browser automatically re-fetches
`/sw.js` on every navigation and does its own byte-for-byte comparison
against the currently-installed worker; Decision 5's `Cache-Control:
no-cache` header on that one URL (not the *service worker's own internal
caching*, a plain HTTP response header) is what keeps that comparison
honest deploy-to-deploy, rather than this app needing to invent its own
version-check polling.

### 5. Cache invalidation on deploy: build-ID-stamped cache name, purge-on-activate, `skipWaiting`/`clients.claim`, and an explicit no-cache header on `/sw.js` itself

**Chosen, four parts working together:**

1. **`frontend/public/sw.js` contains a placeholder cache-name constant
   (e.g. `const CACHE_NAME = '__BUILD_ID__'`) that a small build-time Node
   script rewrites in place, substituting the real value from `.next/
   BUILD_ID`** — a file Next.js writes after every `next build`, containing
   a fresh unique identifier per build. This script runs as an added step
   in `package.json`'s `build` script (`next build && node scripts/
   stamp-sw-version.mjs`), after `next build` (so `.next/BUILD_ID` exists)
   and before the Docker image layer is finalized (so the stamped file, not
   the placeholder, is what ships). This makes `sw.js`'s own byte content
   change on every deploy — the mechanism Decision 4's last paragraph
   depends on for the browser to notice an update at all — without this
   app needing to invent its own versioning scheme; it borrows Next's,
   the same way the manifest spec borrowed `icon.svg`'s existing artwork
   rather than inventing new brand assets.
2. **`activate` enumerates `caches.keys()` and deletes every cache name
   that isn't the current `CACHE_NAME`.** Combined with (1), this
   guarantees a new deploy's service worker never serves anything from a
   prior deploy's precache — the standard, necessary pairing for
   build-scoped cache names to actually achieve invalidation rather than
   just accumulating old, never-cleaned caches forever.
3. **`self.skipWaiting()` in `install`, `self.clients.claim()` in
   `activate`** — a new SW version takes control immediately rather than
   waiting for every open tab to close. Chosen deliberately given this app
   already has a 30s foreground-polling habit (`AutoRefresh`) — a visitor
   who leaves a tab open across a deploy would otherwise keep running the
   old SW indefinitely, which is a worse default for an app whose users are
   expected to have long-lived open tabs than the alternative. **Named
   trade-off, not hidden**: a tab that survives past `activate` without a
   full reload could, in principle, request an old-build-hashed
   `/_next/static/*` chunk after the new SW has taken over; since
   Decision 1's allowlist caches by exact/prefix URL match (not by
   "whatever the current build's manifest says"), an old-hashed chunk
   request from a stale tab simply isn't in the new precache and falls
   through to network — which, if the *server* has also fully rolled over
   to the new build and no longer serves the old build's static output,
   would 404. This is a pre-existing Next.js characteristic of any
   content-hashed-asset deploy (App Router's own client-side router
   already surfaces a "chunk load error" prompting a hard refresh in this
   exact scenario, with or without a service worker in the picture) — not
   a new failure mode this spec introduces, but worth naming plainly
   rather than implying the service worker makes deploys perfectly seamless
   for a tab that's been open since before the deploy.
4. **`next.config.mjs` gets a new `headers()` function setting
   `Cache-Control: no-cache` on `/sw.js` specifically** (confirmed
   deliverable per Current relevant state: `next start`, no CDN/ingress
   layer overriding response headers). Without this, an aggressively
   browser-HTTP-cached `sw.js` response could mask (1)'s whole point —
   the browser's *service-worker-specific* update check still runs, but a
   stale HTTP-cached response body served to that check would make it look
   like nothing changed. `no-cache` (not `no-store`) is intentional: it
   still permits a conditional revalidation request, cheaper than forcing
   a full re-download every time, while guaranteeing the browser never
   trusts a locally-cached copy without checking.

**A hard prerequisite this decision surfaced, not introduced**: none of
the above matters if `/sw.js` itself 404s in the deployed environment,
which — per Current relevant state's last bullet — is the current,
pre-existing, undetected state of `frontend/public/`'s *existing* contents
(the manifest icons) in `runtime-prod`. This spec's implementation must
restore `Dockerfile`'s `COPY --from=builder-prod --chown=frontend:frontend
/app/public ./public` line (already anticipated by that stage's own
now-stale comment) as a blocking step, not an optional follow-up.

## Architecture

```
frontend/
├── app/
│   ├── layout.tsx              MODIFIED: mounts <ServiceWorkerRegister />
│   │                             alongside <AutoRefresh />/<ColorSchemeMeta />
│   ├── manifest.ts              unchanged (already shipped)
│   ├── error.tsx                unchanged — its tone is reused, not its code
│   └── ...                      no other route changes
├── components/
│   └── ServiceWorkerRegister.tsx   NEW: side-effect-only Client Component
│                                     (Decision 4) — feature-detects, calls
│                                     navigator.serviceWorker.register('/sw.js'),
│                                     and (via a tiny helper, see below) keeps
│                                     `localStorage['lastSuccessfulLoadAt']`
│                                     updated on each successful
│                                     navigation/router.refresh() tick, for
│                                     offline.html's "last connected" line
├── public/
│   ├── icon-192.png              unchanged (already shipped)
│   ├── icon-512.png               unchanged (already shipped)
│   ├── sw.js                     NEW: hand-rolled service worker (Decision 2)
│   │                               - install: precache the fixed allowlist
│   │                               - activate: purge non-current-CACHE_NAME
│   │                                 caches (Decision 5), skipWaiting/claim
│   │                               - fetch: allowlist match → cache-first;
│   │                                 else → network passthrough, no caching;
│   │                                 navigation-request network failure →
│   │                                 serve cached /offline.html
│   ├── sw-cache-rules.js         NEW: the allowlist-matcher as one small,
│   │                               pure, exported function — imported by
│   │                               sw.js (classic `importScripts`, no build
│   │                               step needed for a same-origin script URL)
│   │                               AND directly importable by a Vitest test
│   │                               (Testing, below) — the one piece of SW
│   │                               logic worth unit-testing in isolation
│   │                               from the ServiceWorkerGlobalScope
│   └── offline.html              NEW: static, zero-JS-framework-dependency
│                                    fallback page (Decision 3)
├── next.config.mjs               MODIFIED: adds headers() for
│                                    Cache-Control: no-cache on /sw.js
│                                    (Decision 5)
└── scripts/
    └── stamp-sw-version.mjs      NEW: tiny Node script (Decision 5), wired
                                     into package.json's `build` script,
                                     substitutes .next/BUILD_ID into sw.js's
                                     CACHE_NAME placeholder

frontend/Dockerfile               MODIFIED (prerequisite, Decision 5):
                                     restores the `COPY .../public ./public`
                                     line in the runtime-prod stage.
```

No change to `frontend/app/api/[...path]/route.ts`, `AutoRefresh.tsx`,
`ColorSchemeMeta.tsx`, or any data-fetching path in `lib/api.ts` — this
entire feature sits alongside the existing request flow, never inside it.

## Error handling

- **`navigator.serviceWorker.register()` rejecting** (unsupported browser,
  a `sw.js` fetch failure, a syntax error in a bad deploy): caught and
  swallowed in `ServiceWorkerRegister.tsx`, same `.catch(() => ...)`
  degrade-quietly shape `AuthNavItem`/`DataFreshnessNavItem`
  (`layout.tsx:62,79`) already use for a failed fetch in a root layout with
  no route-level `error.tsx` — a broken service worker registration must
  never break the page it's mounted on. The app functions identically to
  today (no offline support, no asset precaching) if this fails.
- **`install`'s precache fetch failing for one of the five allowlisted
  URLs** (e.g. a transient network blip fetching `/icon-512.png` during
  first install): a single failed `cache.addAll()` call fails the whole
  `install` event by design (this is the standard Cache API behaviour, not
  a choice this spec is making) — the browser retries `install` on a later
  visit rather than leaving a half-precached SW active. Accepted as
  correct: better to retry installation than run indefinitely with an
  incomplete precache and unpredictable which allowlisted assets are
  actually cached.
- **A `fetch` event for a navigation request failing** (genuine offline, or
  DNS/connection failure): this is the one caching decision this spec
  designs deliberately, not an error path to merely handle — Decision 1/3:
  serve the precached `/offline.html`, never a stale copy of real content.
- **A `fetch` event for a non-navigation, non-allowlisted request failing**
  (e.g. a `PinToggle` mutation attempted while offline): passed straight
  through to `fetch()` with no SW-level interception or fallback at all;
  it fails exactly as it already does today with no service worker present
  — a rejected promise the calling component's own existing error handling
  deals with, unchanged by this spec.
- **`caches.keys()`/`caches.delete()` failing during `activate`** (storage
  quota/permission issues, rare): left unguarded — a failure here should
  surface as a browser-level SW activation failure (visible in DevTools),
  not silently swallowed, since a service worker that can't clean up its
  own old caches is a real signal something's wrong with this browser's
  storage, worth being loud about during development/debugging rather than
  papered over.

## Testing

- **`frontend/public/sw-cache-rules.js`'s allowlist-matcher function is the
  one piece of this feature worth a real Vitest unit test**, and the one
  place this design deliberately extracts pure logic out of the
  `ServiceWorkerGlobalScope`-dependent parts of `sw.js` specifically to
  make that possible — matching the research doc's own §4 finding that
  Vitest's default `jsdom` environment doesn't naturally host a
  `ServiceWorkerGlobalScope`/`fetch`-event-listener test, but *does* run
  a plain exported JS function without needing one. Test cases: each of
  the five allowlisted URL shapes returns `true`
  (`/_next/static/anything`, `/icon-192.png`, `/icon-512.png`,
  `/manifest.webmanifest`, `/offline.html`); representative live-content
  URLs return `false` (`/`, `/lines/123`, `/api/preferences`,
  `/api/Train/track`) — this is the test that most directly guards
  Decision 1's central safety property, so it earns real coverage, not
  just a shape assertion the way `manifest.test.ts` covers static config.
- **`install`/`activate`/`fetch` event-listener glue itself is not
  Vitest-testable** for the same reason the research doc already
  established (no natural `ServiceWorkerGlobalScope` host in `jsdom`) —
  this is the repo's first genuine use case for its already-configured but
  currently-empty `frontend/e2e/` Playwright suite (Current relevant
  state). Concretely worth covering there, since Playwright's `Page`/
  `BrowserContext` APIs directly support what's needed: register the SW
  and wait for `activated` state; assert precached URLs are actually in
  Cache Storage (`page.evaluate(() => caches.open(...))`); use
  `context.setOffline(true)` to simulate a real network failure and assert
  a navigation to `/` renders `offline.html`'s content, not a blank error
  or a stale real page; a second test asserting a *mutation* request
  (`PinToggle`'s flow) still fails normally offline rather than being
  served from any cache. A build-id-bump-then-reload test for Decision 5's
  purge-on-activate behaviour is plausible but would need two separate
  `sw.js` builds in the test fixture — flagged as valuable but more
  involved than the others, not designed to the same level of detail here.
- **Everything genuinely install-flow/platform-specific stays manual**,
  same honesty the manifest spec's own Testing section already committed
  to: whether an installed, standalone-mode PWA on real iOS/Android
  hardware actually registers and updates its service worker correctly
  across a real deploy is not something this repo's Vitest/Playwright setup
  can fully stand in for, and this spec doesn't pretend otherwise.

## Explicitly out of scope

- **Push notifications themselves** (the `push`/`notificationclick` event
  listeners, subscription UI, backend subscription storage, and the
  product question of what triggers a push) — this is the sibling spec's
  job, named explicitly in this task's brief. Decision 4 is written so
  that spec can add those listeners to this same `sw.js` and use this same
  registration without redesigning anything here; no sibling spec document
  existed to cross-check against as of this session (Current relevant
  state) — if one lands later naming a different registration shape or
  scope than Decision 4's, that would need reconciling at that point, not
  something this document can pre-empt without seeing it.
- **Full offline read/write** — queuing a ticket upload, pin toggle, or
  custom line edit made while offline for later replay (Background Sync
  or an equivalent hand-rolled retry queue). Named directly in this task's
  brief as the alternative to "the shell loads, offline is shown honestly"
  and rejected for the reasons in Decision 3: no iOS support regardless,
  and this app's write paths have no existing conflict-handling story to
  build a replay queue on top of.
- **Caching or offline-serving any *live* content whatsoever** — this is
  the entire point of Decision 1, restated here for completeness: no
  navigation response, no RSC-refresh payload, no `/api/*` response is ever
  cached under any strategy, staleness-labelled or not.
- **A generic app-shell caching strategy that reuses cached navigation
  chrome (nav bar, layout) while still fetching content live** — considered
  briefly in Decision 1's reasoning and rejected as infeasible without a
  deeper architectural change: this app's SSR/RSC model renders shell and
  content as one response, with no existing seam a service worker could
  cache one side of without the other. Splitting them would need a
  genuinely different app architecture (e.g., a separate client-fetched
  JSON status endpoint), not a caching-layer decision — out of scope for
  this spec.
- **Serwist/Workbox, or any service-worker-generating build plugin** —
  Decision 2's rejected alternative, not re-litigated further.
- **A maskable icon, or any other manifest-content change** — the manifest
  spec's own scope, already shipped, unchanged here.
- **Fixing `frontend/Dockerfile`'s stale `public/` comment for its own
  sake** — surfaced as a hard prerequisite this spec's implementation must
  address (Decision 5, Current relevant state), but the underlying bug
  (existing manifest icons likely already 404ing in production) predates
  and is independent of this spec; this document doesn't attempt a full
  audit of what else in `runtime-prod` might be affected by it beyond what
  this feature itself needs.

## Open questions/risks

1. **The exact request shape (headers, method) of an App Router
   `router.refresh()`-triggered RSC fetch under this app's pinned Next
   16.2.10 was not empirically verified this session** (would need a live
   running server + a captured network trace to confirm). Decision 1's
   allowlist design is deliberately robust to not knowing this precisely —
   it never needs to *recognize* an RSC-refresh request specifically, only
   to *not accidentally match* it, which a narrow static-URL allowlist
   satisfies regardless — but anyone implementing this should still verify
   empirically (DevTools Network tab, `next dev`) that no allowlist entry
   coincidentally matches a real RSC-refresh request's URL, as a sanity
   check rather than a design gap.
2. **Offline visitors get nothing of previously-viewed content, not even a
   labelled stale copy** — a deliberate, conservative choice (Decision 1),
   named as a real UX cost, not hidden. If this is ever judged too
   conservative, the natural next step would be a much narrower carve-out
   (e.g., caching only a specific, backend-confirmed-static reference
   dataset like the station list, if such an endpoint existed
   independently of live status) — not attempted here, and not obviously a
   good idea even then, given Decision 1's reasoning.
3. **`clients.claim()`'s stale-tab chunk-loading edge case** (Decision 5,
   point 3) is named but not solved — it's a pre-existing Next.js
   characteristic of hashed-asset deploys, not unique to this design, but
   worth confirming this app's existing deploy process doesn't prune old
   builds' static output faster than realistic tab lifetimes, if that
   hasn't already been characterized elsewhere.
4. **The `frontend/Dockerfile` `public/` COPY gap (Current relevant state,
   Decision 5) needs verifying against the actual deployed environment**,
   not just inferred from reading the Dockerfile — if some other mechanism
   (a volume mount, a sidecar, something not visible from the Dockerfile
   alone) already serves `public/` in production despite the missing COPY,
   this finding would be wrong; this session found no such mechanism in
   `charts/distant-signal/templates/` but did not exhaustively rule one
   out.
5. **No sibling push-notification spec existed to cross-check Decision 4
   against as of this session** — noted under Explicitly out of scope;
   revisit once it lands.
