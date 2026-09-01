# Progressive Web App (PWA) Support — Landscape Research

**Status: research/survey only, not an approved design.** Written to the
same rigor as `docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`
(this document's structural template — "no invented details" citation
discipline, findings organized by sub-question, an explicit method section
disclosing tooling limits, and a ranked Recommendation that isn't afraid to
say "less than the full feature set is the right call"). This is not an
implementation plan; no manifest file, dependency, or source-file change was
made as part of this research.

## Problem being researched

The user wants Distant Signal usable as an installable, app-like PWA —
**primarily to make iOS installation easier** (Safari's "Add to Home
Screen" is the practical way to get an app-like icon/launch experience on
iPhone without an App Store listing), with Android/Chrome support treated
as a secondary nice-to-have, since Chrome's PWA support is already broader
and more capable (a real install-prompt API, fuller manifest feature
support). This document asks three things: (1) what does this app's
frontend actually have and lack today, read directly rather than assumed;
(2) what do iOS Safari and Android/Chrome concretely require for an
app-like install, given the two platforms' real and different capability
levels; (3) given this app's specific runtime behaviour — a 30-second
polling auto-refresh over live status data, all client fetches funnelled
through one same-origin proxy route — what service-worker strategy (if
any) is appropriate, without silently scope-creeping into a bigger feature
(offline support, push notifications) than what was actually asked for.

## Method

Findings below are split into two kinds of claim, each held to a different
standard:

- **Claims about this app's own code** are all grounded in files read
  directly during this research pass, cited `file:line`. Nothing about
  this app's current state is asserted from memory of earlier sessions.
- **Claims about iOS Safari / Android Chrome platform behaviour** were
  checked against live web search rather than asserted from training
  data, since PWA platform support (especially iOS's) has changed
  materially release-to-release and stale claims are a known failure mode
  for this kind of research. Each platform claim below is cited to at
  least one source; where sources disagreed or looked like
  SEO-blog-recycled content rather than a primary source, that's flagged
  explicitly rather than silently picking one. One genuine correction
  surfaced by this process: several 2026-dated blog posts (MagicBell,
  webscraft.org) currently claim EU users get a browser-tab-only
  experience under the DMA "as of iOS 17.4+" — a follow-up search found
  this refers to a *reversed* February–March 2024 episode (Apple announced
  the restriction in an iOS 17.4 beta, then reversed it before iOS 17.4
  shipped, per
  [9to5Mac](https://9to5mac.com/2024/03/01/apple-home-screen-web-apps-ios-17-eu/)
  and [TechCrunch](https://techcrunch.com/2024/03/01/apple-reverses-decision-about-blocking-web-apps-on-iphones-in-the-eu/)).
  The restriction never shipped and there's no evidence of a later 2026
  reintroduction — treat any blog repeating "EU gets a degraded PWA
  experience" as stale/wrong unless independently re-verified.

## Findings

### 1. Current state of this frontend

**No manifest exists.** A repo-wide search for `manifest*` under
`frontend/` returns only `frontend/app/apple-icon.png` and
`frontend/app/icon.svg` (Next.js's own icon-file-convention names, not a
manifest) — no `manifest.json`, `manifest.webmanifest`, or `app/manifest.ts`
anywhere in the tree.

**No PWA-adjacent tooling exists.** `frontend/package.json` (read in
full) lists dependencies `@mantine/*`, `dayjs`, `isomorphic-dompurify`,
`next`, `react`/`react-dom`, `recharts`, and a standard dev toolchain
(`vitest`, `@playwright/test`, `typescript`, `postcss`-plus-Mantine
plugins). Nothing PWA-, service-worker-, or `next-pwa`/`serwist`-related.
`frontend/next.config.mjs` (read in full, 19 lines) only configures
`allowedDevOrigins` for the dev Compose network — no service-worker
plugin wrapping, no PWA config of any kind.

**No `<head>` PWA metadata exists today.** `frontend/app/layout.tsx:16-20`
exports a `Metadata` object with only `title` and `description` — no
`viewport` export, no `themeColor`, no `manifest` link. A repo-wide search
for `viewport|themeColor|theme-color` under `frontend/app` turns up
nothing relevant (one unrelated comment about screen viewport widths in
`stations/[crs]/page.tsx:65`). `frontend/app/layout.tsx:104-107`'s
`<head>` contains only Mantine's `<ColorSchemeScript>`.

**Icons exist but don't cover manifest/Android requirements.**
`frontend/app/icon.svg` (32×32 viewBox SVG, Next.js's auto-discovered
favicon convention) and `frontend/app/apple-icon.png` (confirmed via
direct PNG-header read: exactly **180×180**, Next.js's auto-discovered
`apple-touch-icon` convention) both already exist and are wired up
automatically by Next.js's file-convention metadata system — no manual
`<link>` tags needed for either. Per current guidance (see §2 below),
180×180 PNG is in fact the *complete* modern `apple-touch-icon`
requirement, so **`apple-icon.png` already satisfies iOS's icon need with
zero changes**. What's missing is **raster icons sized for a web app
manifest** — Chrome's installability check wants at minimum a 192×192 and
a 512×512 PNG (§2) — and neither `icon.svg` (vector, fine for a favicon,
not what the manifest spec's raster-icon field wants) nor `apple-icon.png`
(180×180, one fixed size) covers that on their own. A manifest addition
would need at least two new raster PNG exports generated from the same
source art `icon.svg` already establishes (grape-6→grape-8 gradient,
signal-arm mark), not a new icon design.

**The 30-second `router.refresh()` polling pattern is real and matters
directly for service-worker design.** `frontend/components/AutoRefresh.tsx:1-23`
mounts a client-only, render-nothing component once in the root layout
(`frontend/app/layout.tsx:110`) that calls `router.refresh()` every
`REFRESH_INTERVAL_MS = 30_000`ms via `@mantine/hooks`' `useInterval`. Its
own doc comment (`AutoRefresh.tsx:8-18`) is explicit about why: this
re-runs the current route's server-side data fetches, which read live,
`cache: 'no-store'` responses (a comment that itself references a past
migration away from `next: { revalidate: 30 }`, i.e. this app already
made a deliberate choice to stop time-window-caching status data
server-side). Any service worker that intercepted and cached these
requests — even briefly — would work directly against this existing
"no server-side staleness window" decision: a cached response served
under a service worker's control wouldn't be re-validated by
`router.refresh()`'s `no-store` semantics the way an uncached network
round-trip is.

**The `/api/[...path]` proxy is the single funnel every client mutation
and live-status read goes through, and a naive cache-everything-under-`/api`
service worker would be actively wrong here.**
`frontend/app/api/[...path]/route.ts:1-155` (read in full) is a same-origin
proxy: browser-initiated GET/POST/PUT/DELETE requests to `/api/*` are
forwarded server-side to the backend's `/public/*` (or `/Train/*` for
train-tracking) routes, carrying the browser's `Cookie` header through and
relaying every `Set-Cookie` back unmodified (`route.ts:76-79`, `109-133`).
This single route handles: read traffic for live line/station/incident
status, the OIDC login/callback/logout flow (whose 3xx redirects are
deliberately *not* auto-followed —`redirect: 'manual'`, `route.ts:91`— so
the browser, not this server, hits the SSO server with its own session),
and session-cookie-bearing mutations (pin toggling, custom lines, train
tracking, ticket uploads). A service worker with a blanket "cache
`/api/*` responses" rule would risk: (a) serving stale live-status data
that contradicts the whole point of the 30s refresh loop, (b) potentially
caching a response carrying a `Set-Cookie` header or serving a cached
body for what should be a live auth-flow redirect. Any service worker
this app adds should treat everything under `/api/*` as explicitly
network-only, not merely "not specially handled" — the two aren't the
same guarantee under some caching strategies (e.g. a generic
"stale-while-revalidate for everything not matched by a static-asset
pattern" fallback rule would still be wrong here without an explicit
`/api/*` exclusion).

**Session cookie is a standard first-party cookie; nothing about it looks
PWA-standalone-mode-sensitive.** `crates/api/src/auth.rs:89-91` builds
`Set-Cookie` headers as `Path=/; HttpOnly; {Secure if https}; SameSite=Lax;
Max-Age=...`. `crates/api/src/routes/auth.rs:26-33`'s `cookie_secure()`
derives the `Secure` flag from whether the configured
`sso_redirect_url` starts with `https://` (confirmed live against local
dev per that comment: a hardcoded-`Secure` cookie previously couldn't be
set at all over plain `http://localhost:3000`). `crates/api/src/main.rs:42-49`
notes `SameSite=Lax` is a second, independent CSRF barrier alongside CORS
configuration. None of this is unusual for a same-origin app, and a PWA
installed to the home screen still runs in the same browser's cookie jar
against the same origin — an installed/standalone browsing context on iOS
(WebKit-backed, same as any Safari tab) and Android (Chrome's standalone
mode) both send and store first-party cookies identically to a normal tab.
Nothing found in this pass suggests special handling would be needed; this
is a low-risk area, not a silently-skipped one.

### 2. What "PWA support" concretely requires, by platform

**iOS Safari (the stated priority).** Current (per this research's web
search — see Method's caveat about SEO-blog sourcing quality; the most
concrete/checkable claims below are corroborated across multiple
independent posts) state:

- As of **iOS 17**, Safari surfaces "Add to Home Screen" more prominently
  when a page has a valid manifest with correct config; as of **iOS 26**,
  a site added to the home screen defaults to opening as a standalone web
  app *even with no manifest at all* — i.e. iOS's baseline "add an icon,
  launch without browser chrome" behaviour does **not strictly require** a
  manifest, though a manifest still controls `start_url`, `scope`,
  `display`, and icon selection precisely rather than leaving it to
  Safari's defaults.
- **No install-prompt API.** iOS Safari does **not** implement
  `beforeinstallprompt` — every iOS install is the user manually tapping
  Share → Add to Home Screen. There is no way to trigger or replace this
  with in-app UI; at most, an app can show its own instructional nudge.
- **The `apple-touch-icon` requirement is simple and already met.**
  Current guidance converges on a single 180×180 PNG, opaque (no
  transparency — iOS composites onto black if you give it alpha), no
  pre-rounded corners (iOS applies its own squircle mask). This app's
  existing `apple-icon.png` is already exactly 180×180 — confirmed by
  direct file read this session, not by these platform-guidance sources.
- **Push notifications exist but only for already-installed apps.** Since
  iOS 16.4 (March 2023), Web Push works — but *only* for a PWA already
  added to the home screen and launched in standalone mode; a normal
  Safari tab cannot receive push at all, on iOS, on any browser (all iOS
  browsers are WebKit-backed by Apple's platform requirement, so this
  isn't Safari-specific). Safari 18.4 added "Declarative Web Push," which
  doesn't require a service worker for the push-display path. This is
  flagged here only to be explicitly set aside — see "Explicitly out of
  scope" below.
- **No Background Sync API support**, and generally no background
  processing — consistent with this app's existing design (all data
  freshness already flows through explicit foreground polling, not
  background sync, so this isn't a capability gap relative to what the
  app does today).
- Net characterization: iOS's PWA support is real but manual-install-only,
  no-prompt, and narrower than Chrome's — exactly the "don't assume
  parity with Android" gap the task asked to characterize honestly, not
  something this research found reason to downplay.

**Android/Chrome.** The more standard, better-documented path:
`manifest.json`/`manifest.webmanifest` (or, in Next.js App Router, the
native `app/manifest.ts` convention — see §3) plus a real install-prompt
opportunity via `beforeinstallprompt`, gated by Chrome's own installability
checklist. Per Chrome/Lighthouse's documented installability criteria: a
`name`/`short_name`, a `start_url`, `display` set to `standalone` (or
`fullscreen`/`minimal-ui`), and an `icons` array containing at least one
192×192 and one 512×512 PNG (192 drives the launcher icon, 512 drives the
install-prompt/splash-screen art and is downscaled by Chrome for
intermediate sizes — no need for 256/384/1024 variants). A maskable icon
(`"purpose": "any maskable"`) is recommended, not required, to avoid a
white-circle clipping artifact on some Android launchers. Historically,
Chrome's installability check also wanted a registered service worker;
current guidance flags this as evolving/being phased toward not being a
hard requirement, but a manifest alone is **not guaranteed** to satisfy
Chrome's own install-prompt criteria the same way it satisfies iOS's
home-screen-add — this is the one place where "manifest-only" doesn't buy
100% of the Android win, only most of it (an icon + a real page open in
Chrome still works via the browser's manual "Install app" menu item even
without meeting every automatic-prompt criterion).

**A concrete manifest sketch, grounded in this app's real branding — not
placeholder values.** `frontend/lib/theme.ts:27` sets `primaryColor:
'grape'`; `frontend/app/globals.css:1-25` and `icon.svg:12-18`'s gradient
both use Mantine grape-6 (`#be4bdb`) as the app's brand-forward colour
(grape-6→grape-8 `#9c36b5` gradient in the icon; grape-7 `#ae3ec9` is used
only for link-text-contrast purposes, not as a brand colour).
`globals.css:755-761`'s body background is not solid grape — it's the
scheme-default white/dark background plus a very subtle
(`color-mix(... 6%, transparent)`) grape wash confined to the top 70vh —
so a manifest's `background_color` (the flash-of-white/splash-screen
colour, distinct from `theme_color`) should be plain white to match the
light-mode default, not grape:

```json
{
  "name": "Distant Signal",
  "short_name": "Distant Signal",
  "description": "A personal UK rail companion: live line status, train tracking, and ticket/Delay-Repay support.",
  "start_url": "/",
  "display": "standalone",
  "background_color": "#ffffff",
  "theme_color": "#be4bdb",
  "icons": [
    { "src": "/icon-192.png", "sizes": "192x192", "type": "image/png" },
    { "src": "/icon-512.png", "sizes": "512x512", "type": "image/png" },
    { "src": "/icon-512-maskable.png", "sizes": "512x512", "type": "image/png", "purpose": "maskable" }
  ]
}
```

(The `name`/`description` text above is this research's own paraphrase for
illustration only, not a proposal to change `layout.tsx:18-19`'s actual
existing metadata description — a real implementation would decide
whether to reuse that copy verbatim.) `theme_color` uses grape-6 to match
the icon's dominant colour and the app's single documented brand-colour
decision, not the near-white body background. This sketch is illustrative
only, consistent with this document's non-implementation scope.

### 3. Service worker strategy options

**Option A — manifest + icons only, no service worker at all.** iOS's
home-screen-install path (§2) does not require a service worker — it's a
Safari-native "Add to Home Screen" action driven by the page's own
manifest/meta tags. This option gets the full iOS win (proper icon,
`standalone` display, correct `start_url`/`scope`) with zero runtime code
added, and sidesteps the entire caching-vs-`AutoRefresh`/`api`-proxy
problem in §1 by simply not introducing a caching layer. On Android, it
still gets a real icon and standalone launch via Chrome's manual "Install
app" affordance, even if it doesn't guarantee the automatic
`beforeinstallprompt` banner (Chrome's install-prompt criteria have
historically preferred, and may still weight, a registered service
worker — unconfirmed how strictly current Chrome enforces this).

**Option B — a service worker scoped to static assets only, explicit
network-only for `/api/*`.** Caches JS/CSS/font/icon assets (Next's
hashed build output under `/_next/static/*`, plus the manifest icons
themselves) for faster repeat loads and closer-to-Chrome's-full
installability checklist, while explicitly excluding everything under
`/api/*` from any cache strategy — network-only, matching §1's finding
that this is the only correct behaviour given `AutoRefresh`'s 30s
`no-store` polling and the proxy's session-cookie/redirect handling. This
is more work than Option A (a service worker file, a registration call,
and — per the task's own framing — a genuinely separate testing surface,
see §4) for a benefit that's real but narrower than it sounds: this app's
static assets are already served with Next.js's own aggressive
content-hashed caching (browser HTTP cache), so a service worker's main
incremental win here is offline availability of the *app shell* (the page
would load with no network at all, then show a "can't reach the
server"-shaped empty state for live data) — not meaningfully faster
first-load performance over what the browser cache already gives for free
on a repeat visit.

**Option C — a fuller offline-capable service worker (cached fallback UI,
background sync, etc.).** Explicitly the most expensive option and, per
§1's live-data-freshness constraint, in direct tension with this app's
actual purpose: showing current train/line status. An offline cache that
serves yesterday's status as if current would be actively misleading for
a live-status app in a way it wouldn't be for, say, a documentation site
or a game. Not evaluated further here beyond flagging it as the
"un-recommended" end of the spectrum.

**Tooling: `next-pwa` is dead; `serwist` is its maintained App-Router-era
successor, if a service worker is pursued.** Web search confirms the
original `next-pwa` package's GitHub repo was archived by its owner in
2023 (read-only, pointing users to an active fork). Its actively
maintained lineage today is **Serwist** (`@serwist/next` + `serwist`,
built by the same maintainer who first forked `next-pwa` as
`@ducanh2912/next-pwa`), which integrates via `withSerwistInit` in
`next.config.mjs` and a `app/sw.ts` source file compiled to
`public/sw.js` — App-Router-compatible by design, not retrofitted. Given
this repo's existing dependency list (§1: nine total runtime dependencies,
all either Mantine's own suite, or single-purpose libraries like `dayjs`/
`recharts`/`isomorphic-dompurify` — no existing precedent for a
build-pipeline-integrating framework plugin of Serwist's shape) and this
project's evident preference for minimal, well-understood dependencies
over framework plugins elsewhere in this codebase, adding Serwist would
be a genuinely new category of dependency for this repo, not an
incremental one. If Option B is ever pursued, a hand-rolled ~30-40 line
service worker (a static-asset cache-first strategy plus an explicit
`/api/` network-only fetch handler, registered from a small client
component) is plausible without any new dependency at all, given how
narrow Option B's actual scope is — Serwist's main value-add is
precache-manifest injection at build time for a *larger* static asset
surface than this need obviously requires, though a hand-rolled worker
does mean this app owns maintaining the precache-invalidation logic
Serwist would otherwise handle.

**Manifest itself needs no new dependency either way.** Next.js's App
Router has a native `app/manifest.ts` file convention (a "special Route
Handler," auto-linked into `<head>` the same way `icon.svg`/
`apple-icon.png` already are today) that returns a typed
`MetadataRoute.Manifest` object — the same file-convention pattern this
app already uses for its favicon/apple-touch-icon, requiring no plugin.
One documented limitation: `manifest.ts` doesn't receive the incoming
request, so it can't vary by query string/auth state — irrelevant for
this app's static manifest sketch above, but worth knowing if a future
need arose.

### 4. Testing implications

This repo has a real, colocated Vitest suite throughout `frontend/`
(confirmed via `frontend/app/layout.test.tsx`, `frontend/app/globals.test.ts`,
and others found this session) but **no existing test touches `<head>`
metadata, viewport config, or icon files** — `layout.test.tsx` (read in
full, 59 lines) only exercises the two exported nav-item Server Components
(`TrackedTrainsNavItem`, `MyTicketsNavItem`) via `renderWithMantine`, not
`RootLayout`'s `metadata` export or anything manifest-shaped. This means
there's no existing pattern in this codebase to extend for a manifest, and
also very little that a manifest addition would obviously need: a
`manifest.ts` returning a static object is a plain data structure, not
logic — the only thing plausibly worth a unit test is a shape assertion
(icons array has the right sizes/types, `start_url`/`display` are
correct), which is a low-value test relative to just reading the file, and
this codebase's `globals.test.ts` precedent (a colocated test for a
non-component file) shows this pattern is at least stylistically
consistent with something the codebase already does elsewhere, if it were
judged worth doing at all.

A hand-rolled or Serwist-based **service worker**, if ever pursued
(Option B/C above), is a different and real testing problem: its
fetch-interception logic (the exact `/api/*`-is-always-network-only rule
this document's §1 finding depends on) is the kind of logic that
plausibly *should* be unit-testable — but Vitest's default `jsdom`
environment doesn't naturally host a `ServiceWorkerGlobalScope`/`fetch`
event listener the way it hosts React component trees, so testing a
service worker's caching decisions would need either a dedicated
service-worker-testing setup (a separate test environment/mocking layer)
or reliance on this repo's existing Playwright e2e suite
(`frontend/package.json:11`'s `test:e2e` script) to exercise it
behaviourally through a real browser context instead. This is a genuine
new testing surface, not a detail to wave away if a service worker is
ever added — flagged explicitly per the task's framing, not resolved
here.

## Explicitly out of scope

- **Push notifications.** A real PWA capability (and, per §2, iOS
  specifically requires the PWA already be installed before push can work
  at all), but a materially bigger, separate feature: it needs its own
  backend subscription/permission-storage model, a UI for opt-in, and a
  server-side trigger for *what* would even push a notification (a new
  incident? a tracked-train delay? — an open product question this
  research wasn't asked to answer). Named here explicitly so it isn't
  quietly folded into "PWA support" scope later.
- **Background Sync / offline data mutation queuing.** Not supported on
  iOS regardless (§2), and this app's write paths (pinning, tracking,
  ticket upload) are all synchronous, session-cookie-gated mutations
  through the `/api` proxy — queuing them for later replay while offline
  is a different feature with its own conflict-handling questions, not
  evaluated here.
- **A full offline-capable app shell / offline fallback page** (Option C,
  §3) — evaluated only enough to rule it out as disproportionate to what
  was asked for; not designed.
- **Changing `layout.tsx`'s existing `Metadata` description/title copy,
  or generating actual PNG icon assets.** This research's manifest sketch
  (§2) is illustrative; no file was created or modified as part of this
  task, per its own constraints.
- **Chrome's exact current enforcement of "service worker required for
  `beforeinstallprompt`."** Flagged as unconfirmed in §2/§3 — current
  guidance suggests this criterion is evolving away from a hard
  requirement, but this pass did not find an authoritative, dated
  confirmation either way. Doesn't change this document's recommendation
  (see below), since Android is explicitly secondary here.

## Recommendation

**Manifest + existing icons, no service worker, is the right scope for
what was actually asked — not "add a service worker because that's what
PWA usually means."** Ranked:

1. **Do first, if this is pursued at all: a `frontend/app/manifest.ts`
   (Next's native, dependency-free convention) plus two new raster icon
   exports (192×192, 512×512, generated from the same `icon.svg` source
   art already established) — Option A from §3.** This is genuinely
   close to the full iOS win the user actually asked for: iOS's
   home-screen-install path doesn't require a service worker, the
   existing `apple-icon.png` already meets the current 180×180
   `apple-touch-icon` requirement with no changes, and a correct manifest
   (`display: standalone`, `start_url: /`, real icons) is what upgrades
   Safari's "Add to Home Screen" from a generic bookmark-shortcut
   experience to a properly branded, chrome-less standalone launch. Zero
   new dependencies, zero interaction with the `AutoRefresh`/`/api`
   caching-correctness problem in §1 (there's no cache to get wrong),
   and a near-complete secondary win on Android too (icon + standalone
   launch via Chrome's manual install affordance, even without a
   guaranteed automatic install banner).
2. **Only if Android's install-prompt banner specifically becomes a
   stated goal (not implied by "secondary nice-to-have" as currently
   framed): a narrowly-scoped Option B service worker** — static assets
   only, explicit network-only for `/api/*`, either hand-rolled (~30-40
   lines, no new dependency, matching this repo's evident minimal-
   dependency taste) or via Serwist if the team decides the precache-
   manifest tooling is worth a new build-pipeline dependency. This is a
   real, bounded increment, not a rejection of service workers in
   principle — but it should be triggered by an actual Android-adoption
   goal, not added reflexively because "PWA" often implies one in general
   web-dev discourse. If pursued, budget for the testing gap in §4 (no
   existing pattern in this codebase covers service-worker fetch-logic
   testing) as real scope, not an afterthought.
3. **Not recommended at all, given this app's live-status purpose: any
   offline-caching strategy beyond static assets (Option C), and push
   notifications.** Both are real PWA capabilities in the abstract, and
   both are the wrong fit for an app whose entire value proposition is
   "the status shown is current" (offline caching of stale status data)
   or a materially separate feature with its own backend/product-design
   surface (push). Neither should be treated as an implied part of "PWA
   support" for this specific app without a separate, deliberate decision
   to take them on.

The honest finding, stated plainly per the task's own framing: **manifest
+ existing icons gets this app most of the iOS value asked for, at very
low implementation cost and with no interaction with this app's live-data
correctness constraints; a service worker adds real, non-trivial
complexity (a new testing surface, a caching-correctness risk that has to
be actively guarded against rather than being free) for a benefit that's
mostly about Android's automatic install-prompt criterion — a platform
this task explicitly framed as secondary.** If this moves to
implementation, the natural next step is a scoped design-spec pass for
Option A alone (icon asset generation, the exact `manifest.ts` content,
and whether/how to surface an iOS-specific "how to install" nudge in the
UI, since iOS has no native install prompt to lean on) — not a combined
manifest-plus-service-worker plan.

## Open questions

1. **Whether Chrome's current installability/`beforeinstallprompt`
   criteria still weight a registered service worker as strongly as
   historically documented**, or whether a manifest-only PWA now reliably
   gets the automatic install banner on Android. Unconfirmed by this
   pass; matters only if Android's install-prompt UX specifically becomes
   a stated goal (see Recommendation #2's trigger condition).
2. **Whether an iOS-specific "how to install" UI nudge is worth building**
   (since iOS has no `beforeinstallprompt`-equivalent to hook a native
   prompt off of) — a product/UX question, not a technical one, and out
   of this research's scope to answer, but worth naming as the natural
   follow-up decision if Recommendation #1 is acted on.
3. **The exact current strictness of Chrome's manifest-icon type/format
   requirements** (e.g. whether `purpose: "any maskable"` on a single
   icon entry vs. separate maskable-only entries matters in practice) —
   not resolved to field-level precision in this pass; a real
   implementation would want to re-check current `web.dev`/Chrome
   DevTools guidance at that time rather than rely on this snapshot.

## References

- iOS 26 default-standalone-launch behaviour, iOS 17 manifest-prominence
  change, iOS 16.4 Web Push origin, Background Sync gap:
  [MobiLoud — Do PWAs Work on iOS? 2026](https://www.mobiloud.com/blog/progressive-web-apps-ios/),
  [MagicBell — PWA iOS Limitations 2026](https://www.magicbell.com/blog/pwa-ios-limitations-safari-support-complete-guide),
  [CoderCops — PWAs in 2026](https://blog.codercops.com/blog/progressive-web-apps-2026)
- No `beforeinstallprompt` on iOS, manual Add-to-Home-Screen only:
  [tutorialpedia.org — Fix PWA Install No Prompt](https://www.tutorialpedia.org/blog/install-to-home-screen-on-ios-for-pwa-enabled-app/),
  cross-checked against MagicBell above
- `apple-touch-icon` 180×180 single-size current guidance, opaque/no-
  pre-rounding requirements:
  [realfavicongenerator.net — Apple touch icon](https://realfavicongenerator.net/blog/apple-touch-icon-the-good-the-bad-the-ugly),
  [appassetgenerator.com — Apple Touch Icon Size Guide](https://www.appassetgenerator.com/blog/apple-touch-icon-size-guide)
- Chrome/Lighthouse manifest installability criteria (192×192/512×512,
  `display`, `start_url`, maskable-icon recommendation):
  [Chrome for Developers — Web app manifest installability](https://developer.chrome.com/docs/lighthouse/pwa/installable-manifest),
  [web.dev — Add a web app manifest](https://web.dev/articles/add-manifest)
- `next-pwa` archived, Serwist as its maintained App-Router-era successor:
  [Rajesh Biswas — PWA in Next.js App Router with Serwist](https://rajesh-biswas.medium.com/how-i-set-up-a-pwa-in-next-js-app-router-typescript-with-serwist-50f55e698ad5),
  [next-pwa GitHub (archived)](https://github.com/ImBIOS/next-pwa)
- Next.js native `app/manifest.ts` file convention:
  [Next.js docs — Metadata Files: manifest.json](https://nextjs.org/docs/app/api-reference/file-conventions/metadata/manifest)
- EU DMA PWA-removal episode and its reversal (correcting several
  2026-dated blog posts' stale claim that it's still in effect):
  [9to5Mac — iOS 17.4 won't remove Home Screen web apps in the EU after all](https://9to5mac.com/2024/03/01/apple-home-screen-web-apps-ios-17-eu/),
  [TechCrunch — Apple reverses decision about blocking web apps on iPhones in the EU](https://techcrunch.com/2024/03/01/apple-reverses-decision-about-blocking-web-apps-on-iphones-in-the-eu/)
- This app's own code: `frontend/app/layout.tsx`,
  `frontend/app/layout.test.tsx`, `frontend/app/icon.svg`,
  `frontend/app/apple-icon.png`, `frontend/app/globals.css`,
  `frontend/lib/theme.ts`, `frontend/next.config.mjs`,
  `frontend/package.json`, `frontend/components/AutoRefresh.tsx`,
  `frontend/app/api/[...path]/route.ts`, `crates/api/src/auth.rs`,
  `crates/api/src/routes/auth.rs`, `crates/api/src/main.rs`
