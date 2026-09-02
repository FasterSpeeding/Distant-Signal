# Frontend Disconnect/Reconnect UX — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.
>
> **Task 0 is a blocking spike and must run first.** Every other task's
> design depends on its finding (does a React Context update actually reach
> `app/error.tsx` inside a tripped Next error boundary?). If Task 0's answer
> is "no", stop and report — Task 6 needs redesigning and nothing after it
> should land on a guess. Open question 4 of the spec named this exact risk.
>
> **After Task 0: Tasks 1, 2 and 3 are independent of each other** and can
> land in any order, one commit each. **Task 4 depends on Task 3**
> (`ConnectivityMonitor` provides the context `app/error.tsx` reads).
> **Task 5 depends on Task 2** (`withStaleFallback` must exist before call
> sites use it). Task 6 is final verification and runs last.
>
> **This plan collides with the unimplemented
> `docs/superpowers/plans/2026-09-02-frontend-ux-review-fixes.md`.** That
> plan's Task 4 rewrites `frontend/app/error.tsx` (to stop printing
> `error.message` verbatim — finding F5b). This plan's Task 4 also rewrites
> that file. Neither has landed as of this plan's HEAD (`app/error.tsx`
> still renders `<Text c="dimmed">{error.message}</Text>`). **Whichever goes
> second rebases rather than re-derives**; the two changes are compatible
> (one changes *what copy* is shown, the other adds *connectivity
> awareness + auto-reset*), they simply touch the same 25-line file. See
> "Interaction with the ux-review-fixes plan" below.

**Goal:** implement
`docs/superpowers/specs/2026-09-02-frontend-disconnect-reconnect-ux-design.md`
— when the Rust `api` service is unreachable from the Next.js frontend
server, keep the last-known data on screen with a non-blocking
"Reconnecting…" indicator, retry on the cadence the app already has, and
recover automatically, instead of replacing every page's content with
`app/error.tsx`'s dead-end error card.

**Architecture:** frontend-only. No new npm packages, no backend changes,
no migrations, no new API routes. Two new files, five modified files, one
Helm values comment.

| | File | Change |
|---|---|---|
| NEW | `frontend/lib/liveDataCache.ts` | `withStaleFallback()` + its TTL/cap constants |
| NEW | `frontend/components/ConnectivityMonitor.tsx` | context + detection + banner |
| MOD | `frontend/app/layout.tsx` | thread `backendReachable`, mount the monitor |
| MOD | `frontend/app/error.tsx` | connectivity-aware copy + auto-`reset()` |
| MOD | `frontend/components/AutoRefresh.tsx` | visibility-based pause/resume |
| MOD | `frontend/app/page.tsx`, `app/lines/page.tsx`, `app/lines/[id]/page.tsx`, `app/stations/[crs]/page.tsx` | opt into `withStaleFallback` + degrade the sibling fetches |
| MOD | `charts/distant-signal/values.yaml` | document the single-replica assumption |

**Tech stack:** Next.js 16 App Router + TypeScript (strict) + Mantine
9.5.2 (pinned exact), Vitest 2 + `@testing-library/react` via
`frontend/test/render.tsx`'s `renderWithMantine`, Playwright for e2e.

**Specs:**
- `docs/superpowers/specs/2026-09-02-frontend-disconnect-reconnect-ux-design.md`
  — the design this implements. Its Decisions 1–10 are authoritative for
  *what* to build. This plan departs from it in four places, each recorded
  under "Where code investigation corrected the spec".
- `docs/superpowers/specs/2026-09-02-pwa-service-worker-design.md` — the
  adjacent, complementary failure domain (browser ↔ frontend server). Not
  touched here.
- `docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md` — the
  shared-client-state precedent the spec's Decision 3 follows.

---

## Verified facts (ground truth for this plan — do not re-derive)

Everything below was read out of the working tree while planning, at
commit `c69bae7`. File and line numbers are as of that HEAD.

### The error-boundary mechanism

- `frontend/node_modules/next/dist/client/components/error-boundary.js`'s
  `ErrorBoundaryHandler.getDerivedStateFromProps` clears a tripped error
  only when `props.pathname !== state.previousPathname`. `router.refresh()`
  never changes the pathname. **Confirmed** — this is why the existing 30s
  loop can never self-heal a tripped error page, and it is the entire
  reason Task 4 exists.
- `frontend/app/error.tsx` is 25 lines: `'use client'`,
  `Stack`/`Title order={1} size="h2"`/`Text c="dimmed"{error.message}`/
  `Button onClick={reset}`. **The `order={1} size="h2"` and its comment
  block landed in `43595ce` (accessibility fixes) and must be preserved
  verbatim by Task 4** — do not drop the heading-level fix while rewriting
  around it.
- There is no `frontend/app/error.test.tsx`. Task 4 creates the first one.

### The layout and its fetches

- `frontend/app/layout.tsx:58-70` — `DataFreshnessNavItem()` already does
  `await getDataFreshness().catch(() => ({ stations: null, tocs: null,
  incidents: null, tfl: null }))`, with a comment explaining a root layout
  has no `error.tsx` boundary. This is the connectivity oracle.
- `layout.tsx:119` `RootLayout` mounts, inside `<AppMantineProvider>`:
  `<AutoRefresh />` (`:127`), `<ColorSchemeMeta />` (`:128`),
  `<ServiceWorkerRegister loadedAt={new Date().toISOString()} />` (`:135`),
  then the nav `<Box component="nav">` (`:144`), then
  `<Container component="main" size="lg" px={0}>{children}</Container>`
  (`:194-196`), then `<OpenDataAttribution />` (`:197`).
- `DataFreshnessNavItem` is rendered inside a `<Suspense>` at `:172-174`.
  **This matters:** the freshness fetch is *streamed*, so its result is not
  available to the synchronously-rendered `RootLayout` body. Task 1 has to
  restructure this — see Correction 1.
- `frontend/app/layout.test.tsx` (68 lines) does **not** mock `@/lib/api`
  and has **no** existing `DataFreshnessNavItem` coverage. It tests
  `TrackedTrainsNavItem`, `viewport`, `metadata`, and one `readFileSync`
  source assertion. The spec's Testing section assumed a `vi.mock('@/lib/api')`
  shape that does not exist in this file. Task 1 adds the mock.

### `AutoRefresh`

- `frontend/components/AutoRefresh.tsx` is 24 lines:
  `useInterval(() => router.refresh(), 30_000, { autoInvoke: true })`.
- `frontend/components/AutoRefresh.test.tsx` (54 lines) already covers
  "renders nothing", "calls on a 30s interval", "stops once unmounted",
  using `vi.useFakeTimers({ shouldAdvanceTime: true })` and a
  `vi.mock('next/navigation', () => ({ useRouter: () => ({ refresh: refreshMock }) }))`.
  Task 3 extends this file; those three cases must still pass unchanged.
- `@mantine/hooks@9.5.2` ships `useNetwork()`, `useDocumentVisibility()`
  and `useMounted()`. `useNetwork`/`useDocumentVisibility` have zero
  usages in `frontend/` today. No new package needed. Confirmed.

### The four target pages and their *other* fetches

This is the part the spec under-scoped. Wrapping only the status fetcher
per page does **not** stop the page blanking, because each page has
sibling fetches in the same `await`/`Promise.all` that also throw:

| Page | Status fetch (wrap) | Sibling fetches that ALSO throw today |
|---|---|---|
| `app/page.tsx:95-101` | `getLineStatusForMode(DISPLAYED_MODES_PARAM)` | `getPreferences()`, `getMyTrackedTrains()`; plus `getStopPointDisruption(crs)` inside `pinnedStationEntries` (`:176-186`) |
| `app/lines/page.tsx:10-15` | `getLineStatusForMode(...)`, `getAllLines()` | `getPreferences()`, `getAllTocs()` |
| `app/lines/[id]/page.tsx:28-36` | `getLineStatus([id], true)` | `getAllLines()` (`:44`), `getCustomLine`/`getLineDefinition` (already caught) |
| `app/stations/[crs]/page.tsx:54` | `getStopPointDisruption(crs)` | `getPreferences()`; `getStationName` already `.catch()`-guarded (`:33-35`) |

- `getSession()` is **already** `.catch()`-guarded in `app/page.tsx:78-83`
  (degrades to anonymous). No change needed there.
- `getPreferences()` (`lib/api.ts:200-215`) tolerates a **401** only
  (returns `{ pinnedLines: [], pinnedStations: [] }`); every other non-ok
  status, and a rejected `fetch()`, throws. Unguarded at all three call
  sites above.
- `getMyTrackedTrains()` (`lib/api.ts:350`) returns `null` on 401 but
  throws on 5xx/network.
- `getAllTocs()` is `next: { revalidate: 3600 }` reference data;
  `getStationName()` likewise (`lib/api.ts:115-121`).

### Per-user variance of the "public" status endpoints — the security-relevant fact

**`/Line/Mode/{mode}/Status` and `/Line/{ids}/Status` are NOT
user-independent, despite being unauthenticated routes.**
`crates/api/src/routes/line_status.rs:116-141` filters private custom
lines by owner (`owners_for_ids`, then
`(Some(caller), Some(owner_id)) => &caller.id == owner_id`), and `:312-316`
does the same for the single-line route. There is a dedicated DB-backed
test at `:1057-1093` that seeds two sessions and asserts the owner sees
the custom row while a non-owner does not. `lib/api.ts`'s
`getLineStatusForMode`/`getLineStatus` both forward the session cookie
(`cookieForwardInit()`, `lib/api.ts:76-79`).

**Consequence:** a process-local cache keyed only on
`` `lineStatusForMode:${mode}` `` would, during an outage, serve one
visitor's private custom-line status to a different visitor, or to an
anonymous one. This is a cross-user data leak, not a staleness
inconvenience. See Correction 2 — it changes `withStaleFallback`'s
signature contract.

- The session cookie is `distant_signal_session`
  (`crates/api/src/routes/line_status.rs:1026`).
- `getStopPointDisruption` (`lib/api.ts:99-103`) forwards **no** cookies —
  genuinely public. It is still scoped the same way, for one uniform rule.

### Tooling and verification surface

- `frontend/package.json` has **no `lint` script and there is no
  `eslint.config.*`**. `.github/workflows/ci.yml:219-256` documents this
  explicitly: the frontend job is `npm ci` → `npx tsc --noEmit` →
  `npm test` → `npm run build`. **"lint" for this repo's frontend means
  `npx tsc --noEmit`.** Do not add ESLint as part of this plan.
- `frontend/vitest.config.ts` restricts `include` to
  `**/*.test.{ts,tsx,js,jsx}`, keeping Vitest out of `frontend/e2e/`.
- **`frontend/e2e/` is no longer empty** (the spec's Testing section said
  it was — that is now stale): `accessibility.spec.ts`, `chat.spec.ts`,
  `push-notifications.spec.ts`, `service-worker.spec.ts` all exist.
  `ci.yml`'s comment about e2e not existing is likewise stale, but fixing
  CI is out of scope here.
- No test in `frontend/` currently uses `vi.resetModules()`. Task 2 needs
  its own module-state reset — it exports one (Correction 3).
- `charts/distant-signal/values.yaml:1083` is `frontend.replicaCount: 1`
  with no comment. Contrast `:72` and `:563`, which carry explicit
  "intentionally no `replicaCount`" rationale comments — that is this
  repo's established way of recording a single-instance constraint.

---

## Where code investigation corrected the spec

Four departures. Each is a decision this plan makes, not an oversight.

### Correction 1 — `backendReachable` cannot come from the streamed `DataFreshnessNavItem`; hoist the fetch into `RootLayout`

The spec's Decision 1 says to capture success/failure "at that call site"
and "pass that boolean down as a prop". But `DataFreshnessNavItem` is an
async child inside `<Suspense>` (`layout.tsx:172-174`) — it resolves
*after* `RootLayout`'s own JSX has been returned, so `RootLayout` cannot
read its outcome to pass to a sibling.

**Decision:** move the `getDataFreshness()` call up into `RootLayout`
itself (making `RootLayout` `async`), compute
`{ freshness, backendReachable }` once there, pass `freshness` down to
`DataFreshnessNavItem` as a prop, and pass `backendReachable` to
`ConnectivityMonitor`.

The cost is real and must be stated in the code comment: this removes the
streaming behaviour for the freshness tooltip — `RootLayout` now awaits
that fetch before emitting any HTML, adding its latency to every route's
TTFB. That is acceptable and arguably correct here: the fetch is against
the same in-cluster `api` service every page already awaits for its own
content, so in the healthy case it adds no meaningful wall-clock time
(it runs before, not instead of, the page's own fetches, but against a
service on the same network); and in the *unhealthy* case — the case this
whole plan exists for — its failure is the signal we need before first
paint, which streaming it explicitly denies us.

`AuthNavItem`'s own `<Suspense>` (`:177-179`) is **left exactly as it is**.
It is not a connectivity oracle and must keep streaming.

**Rejected alternative:** keep the Suspense boundary and have
`DataFreshnessNavItem` render a hidden `<ConnectivityMonitor>` of its own.
This puts the banner's provider *inside* a Suspense boundary below
`{children}`'s position in the tree, so `app/error.tsx` could never be a
descendant of it — Task 4 would be impossible. Rejected on those grounds.

### Correction 2 — `withStaleFallback` must be session-scoped, or it leaks private custom lines between visitors

Per Verified facts, `/Line/Mode/…/Status` and `/Line/…/Status` vary by
caller. The spec's Decision 5 sketch (`` `lineStatusForMode:${mode}` ``)
would share one cache entry across all visitors.

**Decision:** `lib/liveDataCache.ts` derives the real map key itself, as
`sha256(sessionCookieValue ?? '') + ' ' + key`, reading the cookie
via `next/headers`' `cookies()` — the same mechanism `lib/api.ts` already
uses. Call sites keep passing the plain logical key; they cannot forget to
scope it, because they never construct the real one.

Anonymous visitors all hash to the same bucket, which is correct: they
receive byte-identical public responses. Hashing (rather than using the
raw token as a key) keeps session tokens out of a long-lived in-process
map.

This makes the module server-only (it imports `next/headers`), which it
already effectively was.

**Consequence for tests:** `liveDataCache.test.ts` must
`vi.mock('next/headers')`. `lib/api.test.ts` already establishes that
pattern in this repo — follow it.

### Correction 3 — the cache needs an entry cap, not just a TTL

Session-scoping (Correction 2) multiplies entries by distinct visitors. A
long-running Next process with a plain unbounded `Map` would retain one
entry per (session × key) forever, since nothing evicts on TTL expiry —
the TTL only decides whether an entry is *usable*, not whether it is
*retained*.

**Decision:** cap the map at `STALE_CACHE_MAX_ENTRIES = 500` with
insertion-ordered eviction (JS `Map` iterates in insertion order; on
overflow, delete the first key). Also delete entries found to be past
their TTL on read. 500 × a line-status payload is a bounded, small
memory ceiling for a personal-scale deployment.

Export `__resetStaleCacheForTests()` for Task 2's test file — no
`vi.resetModules()` pattern exists in this repo to follow, and an
explicit exported reset is clearer than dynamic re-imports.

### Correction 4 — wrapping the status fetch alone does not stop the page blanking

Per the Verified facts table, every one of the four target pages has at
least one *other* unguarded fetch in the same critical path. Wrapping only
the status fetcher would leave `getPreferences()` (a 5xx or a rejected
`fetch` during the very same outage) still throwing to `app/error.tsx` —
the feature would appear to do nothing.

**Decision:** Task 5 does both, per page:
1. `withStaleFallback(...)` around the read-only status fetch (the spec's
   Decision 5 allowlist: `getLineStatusForMode`, `getLineStatus`,
   `getStopPointDisruption`, `getAllLines`).
2. A plain `.catch(fallback)` on each remaining per-user fetch, degrading
   in the **safe** direction — `getPreferences()` → `{ pinnedLines: [],
   pinnedStations: [] }` (the exact shape it already returns for a 401),
   `getMyTrackedTrains()` → `null` (its own established "not logged in"
   value), `getAllTocs()` → `[]`.

   These deliberately do **not** go through `withStaleFallback`. That is
   the spec's Decision 5 exclusion holding: per-user, write-adjacent data
   must fail closed (show fewer pins) rather than be stale-served. A
   logged-in visitor loses their pinned sections for the duration of an
   outage but keeps the whole page and the live status content —
   materially better than today's blank error card, and it never shows
   anyone data that is not theirs.

---

## Resolved open questions

The spec left six open. All six are decided here.

1. **`frontend.replicaCount` and the process-local cache (spec Q1).**
   **Decision: document, do not guard.** Task 5 Step 6 adds a rationale
   comment at `charts/distant-signal/values.yaml:1083`, matching the
   established style of `:72` and `:563`. No Helm validation/`fail`
   template is added: unlike `postgresql.replicaCount` (whose constraint
   is a genuine correctness one — migration advisory locks), scaling the
   frontend out is **not incorrect** here. Each pod holds its own
   internally-consistent cache; the only effect is that during an outage a
   visitor may get a stale-served page from one pod and Task 4's
   auto-retrying error page from another. Blocking a legitimate scale-out
   with a hard failure over a degraded-but-safe outage behaviour would be
   the wrong trade. The comment says exactly that, so an operator raising
   it is making an informed choice.

2. **Stale-data TTL (spec Q2). Decision: 10 minutes**,
   `STALE_DATA_TTL_MS = 10 * 60 * 1000`, a single exported constant.
   Justification: (a) it is 20 `AutoRefresh` cycles — long past any
   plausible transient blip, unambiguously "a real outage"; (b) the app's
   only other cache window is `getStationName`'s 1 hour, explicitly for
   data that "changes on the order of years" (`lib/api.ts:111-114`) —
   live status needs something an order of magnitude shorter; (c) rail
   disruption meaningfully evolves on a 5–15 minute scale, so 10 minutes
   is at the edge of "still worth showing" without crossing into
   confidently-wrong; (d) past the TTL the honest answer is Task 4's
   auto-retrying error page, which — unlike today — actually recovers on
   its own, so the TTL expiring is no longer a dead end. One constant,
   one line to tune.

3. **~60–90s worst-case detection latency (spec Q3). Decision: accept, do
   not add a second timer.** The latency is *cosmetic only* once Task 5
   lands: the visitor keeps seeing real (stale) content throughout that
   window, so the banner is telling them why the timestamps stopped
   moving, not gating anything. A dedicated faster probe would double
   steady-state backend load permanently to shave a delay that costs
   nothing. The spec's own documented escalation path (Decision 1's
   rejected alternative — a dedicated client-side probe) stays on record
   as the next step if real use disagrees. Task 6 Step 4 measures the
   actual observed latency against a running stack so the number is
   recorded rather than assumed.

4. **Does `reset()` / a Context update actually reach `app/error.tsx`
   (spec Q4)? Decision: verify before building on it — this is Task 0**,
   a blocking spike against a real dev server. The reasoning says yes
   (React's context propagation is not blocked by an intervening class
   component such as `ErrorBoundaryHandler`, and the provider will be an
   ancestor of the boundary), but the spec was right to refuse to commit
   to it unverified. Task 0 proves or disproves it in a scratch branch
   before Task 4 is written.

5. **Rollout scope (spec Q5). Decision: the four pages the spec named,
   confirmed, and that is sufficient for v1** — `/` (`app/page.tsx`),
   `/lines`, `/lines/[id]`, `/stations/[crs]`. These are every route
   reachable from the nav bar that renders live status, i.e. the entire
   surface a visitor hits by browsing rather than by deep link. But it is
   only sufficient **because of Correction 4** — the four pages get their
   *sibling* fetches degraded too, not just the status one. Deferred and
   named: `/incidents/[id]`, the history/trends pages, `/track/*`, `/chat`.
   Those are deep-link or task-flow pages where Task 4's now-self-healing
   error page is an acceptable outage experience, and three of them are
   per-user write-adjacent data the spec's Decision 5 excludes from
   stale-serving on correctness grounds anyway.

6. **One unified banner message for both signals (spec Q6). Decision:
   accept the simplification**, with one wording change from the spec's
   draft copy. The spec's body text ("Distant Signal can't reach the
   server right now") reads as wrong when the visitor's own device is
   offline. Task 3 uses connectivity-neutral copy that is true in both
   cases: title "Reconnecting…", body "Can't reach live data right now —
   showing the last update." No branching, no second message.

---

## Global constraints

- **Flat Mantine named exports only, never dot-notation.** `Notification`,
  `Stack`, `Title`, `Text`, `Button` — not `Mantine.Notification`. This
  repo has already been broken twice by dot-notation across the
  Server/Client boundary (`ae0bd22`, "use flat ListItem export, not
  List.Item").
- **Anything reading browser-only state is `useMounted()`-gated** and
  renders the deterministic server value until mounted. Precedents:
  `PrideToggle`, `ThemeToggle`, `ServiceWorkerRegister`, `ColorSchemeMeta`,
  `LastUpdated`.
- **No new npm dependencies.** `@mantine/hooks`' `useNetwork`,
  `useDocumentVisibility`, `useMounted`, `useInterval` and
  `@mantine/core`'s `Notification` are all already installed.
- **No changes to `lib/api.ts`'s existing exports or signatures.** Only
  new call-site usage.
- **No changes to `frontend/app/api/[...path]/route.ts`,
  `frontend/public/sw.js`, or `offline.html`.**
- Tests are colocated `*.test.tsx`/`*.test.ts` and render through
  `renderWithMantine` from `@/test/render`.
- Verification per task: `npx tsc --noEmit && npm test` from `frontend/`.
  Task 6 adds `npm run build` and live verification.
- Work in a dedicated git worktree off `main` (this repo's established
  pattern — see `.claude/worktrees/`), one commit per task.

## Interaction with the ux-review-fixes plan

`docs/superpowers/plans/2026-09-02-frontend-ux-review-fixes.md` Task 4
also rewrites `frontend/app/error.tsx` (finding F5b: stop rendering
`error.message` verbatim to users). Neither plan has landed.

- If **this plan lands first**: that plan's Task 4 rebases onto the
  connectivity-aware `error.tsx`. Its change is to the `<Text c="dimmed">`
  line only, which this plan preserves as the non-connectivity branch.
- If **that plan lands first**: this plan's Task 4 keeps its new copy and
  wraps it in the `disconnected` conditional.

Either order works. The one thing neither may do is drop
`<Title order={1} size="h2">` or its comment block (landed in `43595ce`).

---

## Tasks

### Task 0: Spike — prove a Context update reaches `app/error.tsx` inside a tripped error boundary

**Files:** none committed. This is a throwaway experiment.

Resolves spec open question 4. **Blocking: do not start Task 4 until this
answers yes.**

- [ ] **Step 1: Stand up a dev server**

```bash
cd frontend && npm ci && npm run dev
```

If `API_BASE_URL` is unset the app will fail everywhere, which is fine —
that *is* the failure being simulated. Prefer pointing it at the repo's
`docker compose` stack so you can stop and start `api` deliberately.

- [ ] **Step 2: Add a throwaway context + a deliberately-throwing page**

Temporarily, in the working tree (to be reverted):

- In `app/layout.tsx`, create a context with a value that flips on a
  timer, and wrap `{children}`'s `<Container component="main">` in its
  provider from a small `'use client'` component.
- In `app/error.tsx`, add `useEffect(() => { console.log('[spike] ctx
  ->', value); }, [value])` and a `console.log` on render.
- Make a page throw unconditionally (e.g. add `throw new Error('spike')`
  at the top of `app/lines/page.tsx`'s component body).

- [ ] **Step 3: Observe**

Navigate to the throwing page so the boundary trips. Watch the browser
console.

**Expected (the assumption under test):** `[spike] ctx -> …` logs again
each time the provider's value changes, *while the error card is on
screen*. If it does, the assumption holds and Task 4 proceeds as written.

- [ ] **Step 4: Also confirm `reset()` re-renders the segment**

From the tripped state, with the throw removed (edit the file so the
route now renders fine — Fast Refresh will not clear the boundary on its
own), click "Try again". Expect the real page to appear. This confirms
`reset()` is sufficient once the underlying cause is gone, which is what
Task 4's auto-call relies on.

- [ ] **Step 5: Revert everything and record the finding**

```bash
cd /path/to/worktree && git checkout -- frontend/
```

**If Step 3 fails** (no re-log while the boundary is tripped): **stop and
report a blocking issue.** Do not proceed to Task 4. The documented
fallback is for `ConnectivityMonitor` to own a `key` on the `<Container
component="main">` wrapper that changes on reconnect (forcing React to
remount the whole subtree, including the boundary), but that is a
different, heavier design that needs its own review — not something to
improvise here.

- [ ] **Step 6: No commit.** Record the outcome in the Task 6 verification
  notes and in the Task 4 commit message.

---

### Task 1: Hoist the freshness fetch into `RootLayout` and thread `backendReachable`

**Files:**
- Modify: `frontend/app/layout.tsx`, `frontend/app/layout.test.tsx`

Implements spec Decision 1, as amended by Correction 1. Independent of
Tasks 2 and 3. `ConnectivityMonitor` does not exist yet, so this task
computes the boolean and holds it; Task 3 consumes it.

- [ ] **Step 1: Replace `DataFreshnessNavItem`'s fetch with a prop**

`app/layout.tsx:54-70` — replace the async component with a synchronous
one taking `freshness` as a prop. Keep the existing comment's substance,
updated to explain why it no longer streams:

```tsx
// Takes `freshness` as a prop rather than fetching it itself. The fetch
// moved up into RootLayout (below) because its *success or failure* is
// this app's backend-reachability signal -- and a fetch inside a
// <Suspense> boundary resolves after RootLayout has already returned its
// JSX, so RootLayout could never read the outcome to pass to a sibling.
// See docs/superpowers/specs/2026-09-02-frontend-disconnect-reconnect-ux-design.md
// Decision 1 and its implementation plan's Correction 1.
//
// The cost, stated plainly: this nav-bar tooltip no longer streams in --
// RootLayout awaits it before emitting any HTML. Acceptable because the
// call is against the same in-cluster `api` service every page already
// awaits for its own content, and because in the failure case (the one
// this whole design exists for) we specifically need the outcome before
// first paint. AuthNavItem below deliberately keeps its own <Suspense>:
// it is not a connectivity oracle.
function DataFreshnessNavItem({ freshness }: { freshness: DataFreshness }) {
  return <DataFreshnessInfo freshness={freshness} />;
}
```

Import `DataFreshness` from `@/lib/types`.

- [ ] **Step 2: Make `RootLayout` async and compute both values**

`app/layout.tsx:119` — change to:

```tsx
const UNAVAILABLE_FRESHNESS: DataFreshness = {
  stations: null,
  tocs: null,
  incidents: null,
  tfl: null,
};

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  // A root layout has no route-level `error.tsx` boundary (that only
  // catches errors in child segments), so an uncaught fetch failure here
  // would take down every page rather than just one -- fall back to an
  // all-"never fetched" state instead. Unchanged in substance from the
  // previous `.catch()` on this same call; the only addition is that we
  // now also record *whether* it fell back, which is the
  // backend-reachability signal ConnectivityMonitor debounces.
  let freshness: DataFreshness;
  let backendReachable: boolean;
  try {
    freshness = await getDataFreshness();
    backendReachable = true;
  } catch {
    freshness = UNAVAILABLE_FRESHNESS;
    backendReachable = false;
  }
```

- [ ] **Step 3: Drop the freshness `<Suspense>`, pass the prop**

`app/layout.tsx:172-174` — replace

```tsx
                  <Suspense fallback={<ActionIcon variant="subtle" aria-label="Data freshness" disabled loading />}>
                    <DataFreshnessNavItem />
                  </Suspense>
```

with

```tsx
                  <DataFreshnessNavItem freshness={freshness} />
```

Leave `AuthNavItem`'s `<Suspense>` at `:177-179` untouched. `Suspense` and
`ActionIcon` are both still used elsewhere in the file — verify before
touching the import list at `:2-3` (`ActionIcon` is used only by the
removed fallback; if so, drop it from the `@mantine/core` import and let
`tsc --noEmit` confirm).

- [ ] **Step 4: Hold `backendReachable` until Task 3**

To keep this task independently committable and typecheck-clean, mark the
variable as intentionally not yet consumed with a comment naming Task 3.
Do **not** add an `eslint-disable` (there is no ESLint here) and do not
prefix it with `_`. If `tsc --noEmit` complains about an unused local
(it will not — `noUnusedLocals` is not enabled in
`frontend/tsconfig.json`), then merge Tasks 1 and 3 into one commit
instead of working around it.

- [ ] **Step 5: Tests**

Append to `frontend/app/layout.test.tsx`. This file has no `vi.mock` for
`@/lib/api` yet — add one at the top, alongside the existing imports:

```tsx
vi.mock('@/lib/api', () => ({
  getDataFreshness: vi.fn(),
  getSession: vi.fn(),
}));
```

Then:

```tsx
describe('DataFreshnessNavItem', () => {
  it('renders the freshness it is given, without fetching', () => {
    renderWithMantine(
      <DataFreshnessNavItem freshness={{ stations: null, tocs: null, incidents: null, tfl: null }} />,
    );
    expect(screen.getByRole('button', { name: 'Data freshness' })).toBeInTheDocument();
  });
});

describe('backend reachability threading', () => {
  // RootLayout renders <html>/<body> and cannot be mounted by
  // @testing-library/react (same constraint the <main> landmark test
  // above documents), so this asserts on the source -- the established
  // tactic in this file and in app/globals.test.ts. The behavioural
  // coverage lives in ConnectivityMonitor.test.tsx and e2e.
  it('passes a backendReachable boolean derived from the freshness fetch', () => {
    const source = readFileSync('app/layout.tsx', 'utf8');
    expect(source).toMatch(/backendReachable = true/);
    expect(source).toMatch(/backendReachable = false/);
  });
});
```

Export `DataFreshnessNavItem` from `layout.tsx` (it is currently
module-private) so the test can import it — `TrackedTrainsNavItem` is
already exported for exactly this reason (`:115`).

Adjust the `aria-label`/role assertion to whatever `DataFreshnessInfo`
actually renders; check `components/DataFreshnessInfo.tsx` and its
existing test rather than guessing.

- [ ] **Step 6: Verify**

```bash
cd frontend && npx tsc --noEmit && npm test
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add frontend/app/layout.tsx frontend/app/layout.test.tsx
git commit -m "Hoist the freshness fetch into RootLayout as a backend-reachability signal"
```

---

### Task 2: `lib/liveDataCache.ts` — session-scoped, TTL'd, capped stale fallback

**Files:**
- Create: `frontend/lib/liveDataCache.ts`, `frontend/lib/liveDataCache.test.ts`

Implements spec Decision 5's mechanism, with Corrections 2 and 3.
Independent of Tasks 1 and 3. Task 5 depends on this.

- [ ] **Step 1: Write the module**

`frontend/lib/liveDataCache.ts`:

```ts
import { createHash } from 'node:crypto';
import { cookies } from 'next/headers';
import { ApiForbiddenError, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';

/** 10 minutes. Justification (this plan's Resolved open questions #2):
 * 20 AutoRefresh cycles, so unambiguously past any transient blip;
 * an order of magnitude shorter than getStationName's 1-hour window,
 * which is for data that "changes on the order of years"; and inside the
 * 5-15 minute scale on which rail disruption meaningfully evolves, so a
 * value served from here is still worth showing rather than confidently
 * wrong. Past this, app/error.tsx's auto-retrying state is the honest
 * answer instead. */
export const STALE_DATA_TTL_MS = 10 * 60 * 1000;

/** Entries are scoped per session (see `scopedKey`), so the map's size is
 * distinct-visitors x distinct-keys and would otherwise grow without
 * bound in a long-lived server process -- the TTL governs whether an
 * entry is *usable*, not whether it is *retained*. JS Maps iterate in
 * insertion order, so evicting the first key is an oldest-first eviction. */
export const STALE_CACHE_MAX_ENTRIES = 500;

const cache = new Map<string, { data: unknown; at: number }>();

/** The cached line-status endpoints are NOT user-independent, despite
 * being unauthenticated routes: crates/api/src/routes/line_status.rs
 * filters private custom lines by owner, and lib/api.ts forwards the
 * session cookie to them. Keying on the logical key alone would serve one
 * visitor's private custom line to another during an outage. Call sites
 * pass only the logical key and can't forget this, because they never
 * build the real one. The cookie value is hashed rather than used
 * directly so raw session tokens never sit in a long-lived map key. */
async function scopedKey(key: string): Promise<string> {
  const session = (await cookies()).get('distant_signal_session')?.value ?? '';
  return `${createHash('sha256').update(session).digest('hex')} ${key}`;
}

/** Opt-in, per-call-site stale-data fallback for read-only live status
 * fetches. On success, caches and returns fresh data. On a connectivity-
 * shaped failure, returns a cached value if one exists and is younger
 * than STALE_DATA_TTL_MS; otherwise rethrows, leaving the call site's own
 * error handling (ultimately app/error.tsx, which is connectivity-aware
 * and self-healing) to deal with it.
 *
 * ApiNotFoundError / ApiUnauthorizedError / ApiForbiddenError are always
 * rethrown and never stale-served: those are meaningful application
 * states (a deleted line, a logged-out session, a revoked permission),
 * not connectivity failures, and papering over them with old data would
 * be wrong rather than merely stale. Only the generic Error case --
 * network failure, 5xx, timeout -- triggers the substitution.
 *
 * Deliberately NOT applied to session-, mutation- or write-adjacent data.
 * See the design spec's Decision 5. */
export async function withStaleFallback<T>(key: string, fetcher: () => Promise<T>): Promise<T> {
  const mapKey = await scopedKey(key);
  try {
    const data = await fetcher();
    cache.set(mapKey, { data, at: Date.now() });
    if (cache.size > STALE_CACHE_MAX_ENTRIES) {
      const oldest = cache.keys().next();
      if (!oldest.done) cache.delete(oldest.value);
    }
    return data;
  } catch (err) {
    if (
      err instanceof ApiNotFoundError ||
      err instanceof ApiUnauthorizedError ||
      err instanceof ApiForbiddenError
    ) {
      throw err;
    }
    const entry = cache.get(mapKey);
    if (entry === undefined) throw err;
    if (Date.now() - entry.at > STALE_DATA_TTL_MS) {
      cache.delete(mapKey);
      throw err;
    }
    return entry.data as T;
  }
}

/** Test-only. No vi.resetModules() pattern exists in this repo to follow,
 * and an explicit reset is clearer than dynamic re-imports per case. */
export function __resetStaleCacheForTests(): void {
  cache.clear();
}
```

- [ ] **Step 2: Write the tests**

`frontend/lib/liveDataCache.test.ts`. Mock `next/headers` — follow
whatever shape `lib/api.test.ts` already uses (read it first; do not
invent a second convention):

Cases:
1. A throwing fetcher with nothing cached **rethrows** the original error.
2. A succeeding fetcher returns fresh data and populates the cache.
3. After (2), a throwing fetcher for the same key returns the cached value
   silently.
4. After (2), with `vi.useFakeTimers()` advanced past `STALE_DATA_TTL_MS`,
   a throwing fetcher **rethrows** instead of serving the stale entry.
5. `ApiNotFoundError` is rethrown even when a fresh entry exists for that
   key (same for `ApiUnauthorizedError`, `ApiForbiddenError`).
6. **Session scoping:** populate the cache with the mocked cookie set to
   session A, then change the mock to session B and call with a throwing
   fetcher for the *same logical key* — it must rethrow, not return
   A's data. This is the regression test for Correction 2 and is the most
   important case in the file.
7. Eviction: writing `STALE_CACHE_MAX_ENTRIES + 1` distinct keys leaves
   the map at the cap and the first key no longer served.

`beforeEach(() => __resetStaleCacheForTests())`.

- [ ] **Step 3: Verify**

```bash
cd frontend && npx tsc --noEmit && npm test
```

- [ ] **Step 4: Commit**

```bash
git add frontend/lib/liveDataCache.ts frontend/lib/liveDataCache.test.ts
git commit -m "Add withStaleFallback: session-scoped, TTL'd stale-data fallback for live status fetches"
```

---

### Task 3: `ConnectivityMonitor` + the context, and `AutoRefresh` visibility pausing

**Files:**
- Create: `frontend/components/ConnectivityMonitor.tsx`,
  `frontend/components/ConnectivityMonitor.test.tsx`
- Modify: `frontend/components/AutoRefresh.tsx`,
  `frontend/components/AutoRefresh.test.tsx`, `frontend/app/layout.tsx`

Implements spec Decisions 2, 3, 4, 8, 9. Depends on Task 1 for the
`backendReachable` value.

- [ ] **Step 1: Write `ConnectivityMonitor.tsx`**

Shape (`'use client'`), following `ServiceWorkerRegister`'s doc-comment
density:

- Exports `ConnectivityContext` (`createContext<{ disconnected: boolean }>({
  disconnected: false })` — a non-throwing default, per the spec's Error
  handling section) and `useConnectivity()`.
- `export function ConnectivityMonitor({ backendReachable, children }: {
  backendReachable: boolean; children: React.ReactNode })` — **a wrapper,
  not a sibling.** It must be an ancestor of `{children}` for Task 4's
  `app/error.tsx` to read the context (see Task 0). The spec's Decision 3
  described it as mounted "alongside" `AutoRefresh`; this is the concrete
  form that satisfies its own Decision 6.
- `const mounted = useMounted();` — render `{children}` and nothing else
  until mounted (spec Decision 9). The context still provides
  `{ disconnected: false }` pre-mount, which is the only honest server
  value.
- `const { online } = useNetwork();`
- Two-strikes counter: a `useState<number>` incremented in a `useEffect`
  keyed on `backendReachable` — increment when `false`, reset to `0` when
  `true`. `const backendDown = failures >= 2;` (spec Decision 2:
  two consecutive failures to trip, one success to clear).
- `const disconnected = mounted && (!online || backendDown);` — note
  `useNetwork()` gets **no** debounce (spec Decision 2).
- Renders:

```tsx
<ConnectivityContext.Provider value={{ disconnected }}>
  {children}
  {disconnected && (
    <div style={{ position: 'fixed', bottom: 16, left: '50%', transform: 'translateX(-50%)', zIndex: 300 }}>
      <Notification loading withCloseButton={false} title="Reconnecting…" role="status" aria-live="polite">
        Can&apos;t reach live data right now — showing the last update.
      </Notification>
    </div>
  )}
</ConnectivityContext.Provider>
```

Copy is connectivity-neutral per Resolved open question 6 — true whether
the device is offline or the backend is down. `zIndex: 300` sits below
`DataFreshnessInfo`'s tooltip (400) and below Mantine's `Modal`, per spec
Decision 8. `Notification` is imported flat from `@mantine/core`.

- [ ] **Step 2: Mount it in `RootLayout`**

`app/layout.tsx` — wrap the nav/main/footer block (everything inside
`<AppMantineProvider>`) in
`<ConnectivityMonitor backendReachable={backendReachable}>`. Wrapping the
whole block rather than only `<Container component="main">` means the
banner's fixed positioning is not constrained by the content container,
and `AutoRefresh` sits inside the provider should it ever need the value.

Remove the Task 1 Step 4 "not yet consumed" comment.

- [ ] **Step 3: Add visibility pausing to `AutoRefresh`**

Spec Decision 4. Keep `REFRESH_INTERVAL_MS = 30_000` exactly as-is, keep
the "renders nothing" contract, and keep the existing doc comment —
append to it rather than replacing it.

```tsx
export function AutoRefresh() {
  const router = useRouter();
  const visibility = useDocumentVisibility();
  const interval = useInterval(() => router.refresh(), REFRESH_INTERVAL_MS);

  useEffect(() => {
    if (visibility === 'visible') {
      // Refresh once immediately on becoming visible rather than making a
      // returning visitor wait up to 30s: whatever is on screen is by
      // definition as stale as the time they spent away.
      router.refresh();
      interval.start();
      return interval.stop;
    }
    interval.stop();
    // No cleanup needed in the hidden branch -- the interval is already
    // stopped, and returning nothing keeps the two branches' intent
    // obvious.
    return undefined;
  }, [visibility, router, interval]);

  return null;
}
```

Note `{ autoInvoke: true }` is dropped: the effect above now owns
start/stop, and leaving `autoInvoke` on would start a second, ungoverned
timer. Verify `useInterval`'s returned object identity is stable across
renders before listing `interval` in the dependency array — read
`node_modules/@mantine/hooks/esm/use-interval/use-interval.mjs`. If it is
**not** stable, the effect will re-run every render and re-`refresh()`
in a loop; in that case depend on `[visibility, router]` only and
reference the interval through a ref, and record the finding in the
commit message. **This is a real trap — check it, do not assume.**

- [ ] **Step 4: Test `ConnectivityMonitor`**

`frontend/components/ConnectivityMonitor.test.tsx`:

- Mock `@mantine/hooks`' `useNetwork` (`vi.mock` with
  `importOriginal`, so `useMounted`/`useInterval` keep their real
  implementations).
- Renders no banner before mount / with `backendReachable={true}` and
  `online: true`.
- **One** `backendReachable={false}` rerender shows **no** banner; a
  **second** consecutive one does. (The two-strikes regression test.)
- A subsequent `backendReachable={true}` clears it immediately.
- `useNetwork()` reporting `online: false` shows the banner immediately,
  with no two-strikes delay, even while `backendReachable` is `true`.
- The banner has `role="status"` and `aria-live="polite"`.
- Children are always rendered, banner or not.

- [ ] **Step 5: Extend `AutoRefresh.test.tsx`**

Keep the three existing cases passing unchanged (they run with the
default jsdom `document.visibilityState`, which is `'visible'`) — **note
that the "calls router.refresh() on a 30s interval" test will now see an
extra immediate call on mount** from Step 3's effect. Update its
assertions to account for that (assert the *interval-driven* calls
relative to the mount call) rather than deleting the test.

Add:
- No `router.refresh()` calls accumulate while visibility is mocked
  `'hidden'` and timers advance 60s.
- An immediate `router.refresh()` on transitioning `'hidden'` →
  `'visible'`.

Mock `useDocumentVisibility` the same way Step 4 mocks `useNetwork`.

- [ ] **Step 6: Verify**

```bash
cd frontend && npx tsc --noEmit && npm test
```

- [ ] **Step 7: Commit**

```bash
git add frontend/components/ConnectivityMonitor.tsx frontend/components/ConnectivityMonitor.test.tsx \
        frontend/components/AutoRefresh.tsx frontend/components/AutoRefresh.test.tsx frontend/app/layout.tsx
git commit -m "Add ConnectivityMonitor: debounced disconnect detection, reconnecting banner, visibility-paused refresh"
```

---

### Task 4: Make `app/error.tsx` connectivity-aware and self-healing

**Files:**
- Modify: `frontend/app/error.tsx`
- Create: `frontend/app/error.test.tsx`

Implements spec Decision 6. **Depends on Task 0's finding and on Task 3.**

- [ ] **Step 1: Rewrite `error.tsx`**

Preserve `<Title order={1} size="h2">` and its full comment block
verbatim (landed in `43595ce`). Add:

```tsx
const { disconnected } = useConnectivity();

// Next's own ErrorBoundaryHandler only clears a tripped error when the
// *pathname* changes (getDerivedStateFromProps in
// node_modules/next/dist/client/components/error-boundary.js), and
// AutoRefresh's router.refresh() never changes the pathname -- so before
// this effect existed, a visitor who hit this page during a backend
// outage stayed on it until they manually clicked "Try again" or
// navigated away, no matter how long ago the backend came back. `reset`
// is a prop only this component receives; no sibling elsewhere in the
// tree can call it, which is why this fix can only live here. See
// docs/superpowers/specs/2026-09-02-frontend-disconnect-reconnect-ux-design.md
// Decision 6.
useEffect(() => {
  if (!disconnected) reset();
}, [disconnected, reset]);
```

Careful with the guard: on a *non*-connectivity error (a genuine
Server Component bug while `disconnected` is `false`), this effect would
fire `reset()` on mount, the page would throw again, and the boundary
would loop. **Track the previous value and only call `reset()` on a
`true` → `false` transition**, via a `useRef<boolean>` seeded from the
first observed value. Do not call `reset()` on the initial render.

Copy, per spec Decision 6: while `disconnected`, show "Trying to
reconnect…" framing instead of the raw error, and keep the "Try again"
button visible in both states.

- [ ] **Step 2: Test**

`frontend/app/error.test.tsx` (the first test for this file):

- The "Try again" button still calls `reset`.
- Connectivity-aware copy appears when the component is rendered inside
  `<ConnectivityContext.Provider value={{ disconnected: true }}>`.
- **`reset` is auto-called when the context flips `true` → `false`
  across a rerender.**
- **`reset` is NOT called on an initial render with `disconnected: false`.**
  (The infinite-loop regression test — this is the important one.)

- [ ] **Step 3: Verify**

```bash
cd frontend && npx tsc --noEmit && npm test
```

- [ ] **Step 4: Commit**

Record Task 0's finding in the message body.

```bash
git add frontend/app/error.tsx frontend/app/error.test.tsx
git commit -m "Make app/error.tsx connectivity-aware and auto-reset on reconnect"
```

---

### Task 5: Roll `withStaleFallback` out to the four live-status pages, and degrade their sibling fetches

**Files:**
- Modify: `frontend/app/page.tsx`, `frontend/app/lines/page.tsx`,
  `frontend/app/lines/[id]/page.tsx`,
  `frontend/app/stations/[crs]/page.tsx`, plus each file's colocated test
- Modify: `charts/distant-signal/values.yaml`

Implements spec Decision 5's scope, with Correction 4. Depends on Task 2.

- [ ] **Step 1: `app/page.tsx`**

At `:95-101`:

```tsx
const [preferences, allReports, myTrackedTrains] = await Promise.all([
  // Fails closed to "nothing pinned" -- the exact shape getPreferences
  // already returns for a 401 -- rather than being stale-served: this is
  // per-user data, and the design spec's Decision 5 excludes per-user
  // state from the stale cache on correctness grounds. Losing the pinned
  // sections for the duration of an outage is materially better than
  // losing the whole page, which is what an unguarded throw here did.
  getPreferences().catch(() => ({ pinnedLines: [], pinnedStations: [] })),
  withStaleFallback(
    `lineStatusForMode:${DISPLAYED_MODES_PARAM}`,
    () => getLineStatusForMode(DISPLAYED_MODES_PARAM),
  ),
  // null is getMyTrackedTrains()'s own established "not logged in" value,
  // and the call site below already collapses it to []. Same fail-closed
  // rationale as preferences above.
  getMyTrackedTrains().catch(() => null),
]);
```

Also guard `getStopPointDisruption(crs)` inside `pinnedStationEntries`
(`:176-186`) — `.catch(() => [])`, matching the `getStationName` guard
already on the line above it.

- [ ] **Step 2: `app/lines/page.tsx`**

Wrap `getLineStatusForMode` and `getAllLines` in `withStaleFallback`
(keys `` `lineStatusForMode:${DISPLAYED_MODES_PARAM}` `` — the *same* key
as `app/page.tsx`, deliberately: it is the same request, so the two pages
should share one entry — and `'allLines'`). `.catch()` `getPreferences()`
and `getAllTocs()` (`→ []`).

- [ ] **Step 3: `app/lines/[id]/page.tsx`**

Compose with the existing `ApiNotFoundError` catch (`:28-36`) rather than
replacing it — `withStaleFallback` rethrows `ApiNotFoundError`
unconditionally, so the `notFound()` branch keeps working:

```tsx
let reports;
try {
  reports = await withStaleFallback(`lineStatus:${id}`, () => getLineStatus([id], true));
} catch (err) {
  if (err instanceof ApiNotFoundError) {
    notFound();
  }
  throw err;
}
```

Also wrap `getAllLines()` at `:44` in `withStaleFallback('allLines', …)`
— same key as Step 2.

- [ ] **Step 4: `app/stations/[crs]/page.tsx`**

At `:54`: `withStaleFallback(`stopPointDisruption:${crs}`, () =>
getStopPointDisruption(crs))`, and `.catch()` `getPreferences()`.
`getStationName` is already guarded (`:33-35`) — leave it.

- [ ] **Step 5: Tests**

Each of the four pages has a colocated `page.test.tsx` that
`vi.mock('@/lib/api')`. Add one case per page: **when the mocked status
fetcher rejects with a generic `Error`, the page still renders its
content** (from a prior successful call in the same test, populating the
cache) **rather than throwing**. Add one case per page: when
`getPreferences` rejects, the page renders with nothing pinned rather
than throwing.

Note these tests exercise `withStaleFallback`'s real module state — call
`__resetStaleCacheForTests()` in `beforeEach` and mock `next/headers` the
same way `lib/api.test.ts` does. If a page test file does not already
mock `next/headers`, adding it is required, not optional.

- [ ] **Step 6: Document the single-replica assumption in the chart**

`charts/distant-signal/values.yaml:1083` — add above `replicaCount: 1`,
matching the comment style at `:72` and `:563`:

```yaml
  # -- Safe to raise, with one documented caveat. frontend/lib/liveDataCache.ts
  # keeps a process-local stale-data cache so a backend outage shows the
  # last-known line status instead of an error page
  # (docs/superpowers/specs/2026-09-02-frontend-disconnect-reconnect-ux-design.md).
  # That cache is per-pod: with more than one replica, during an outage one
  # visitor may get stale-but-useful content from a warm pod while another
  # gets the auto-retrying error page from a cold one. Each pod stays
  # internally consistent and no stale data crosses users (entries are
  # session-scoped), so this is a degraded-experience caveat, not a
  # correctness one -- deliberately documented rather than blocked, unlike
  # postgresql.replicaCount above.
  replicaCount: 1
```

- [ ] **Step 7: Verify**

```bash
cd frontend && npx tsc --noEmit && npm test
cd .. && helm lint charts/distant-signal
```

- [ ] **Step 8: Commit**

```bash
git add frontend/app charts/distant-signal/values.yaml
git commit -m "Keep stale live-status data on screen during a backend outage instead of blanking the page"
```

---

### Task 6: Final verification — suite, typecheck, build, and a real simulated outage

**Files:** none (or an e2e spec if Step 5 is taken).

- [ ] **Step 1: Full frontend gate**

```bash
cd frontend && npx tsc --noEmit && npm test && npm run build
```

All three must PASS. `npm run build` is the one that catches
Server/Client boundary violations, which this plan risks in two places
(`ConnectivityMonitor` wrapping RSC `children`; `liveDataCache` importing
`next/headers`). **Do not claim completion without this output.**

- [ ] **Step 2: Run the existing e2e suite**

```bash
cd frontend && npx playwright test
```

`accessibility.spec.ts` in particular — the new fixed-position
`Notification` must not introduce an axe violation, and the layout
restructuring in Task 1 must not break the `<main>` landmark check.

- [ ] **Step 3: Live verification — healthy state**

Bring up the stack (`docker compose up`, per the repo's compose file) and
`npm run dev` (or the compose frontend). Load `/`, `/lines`,
`/lines/[id]`, `/stations/[crs]`. Expect: no banner, content as before,
freshness tooltip still populated.

- [ ] **Step 4: Live verification — simulated outage (the load-bearing check)**

With a page open and settled:

```bash
docker compose stop api
```

Then, **timing each observation**:
1. The page must **keep showing its content** across the next several
   `AutoRefresh` cycles — not blank, not the error card.
2. The "Reconnecting…" banner must appear bottom-centre within roughly
   60–90s (two failed cycles). **Record the actual observed time** — this
   is the measurement Resolved open question 3 asks for.
3. Navigate to a page with nothing cached (e.g. a `/stations/[crs]` you
   have not visited). Expect `app/error.tsx` with the reconnecting copy,
   not the dead-end card.

```bash
docker compose start api
```

4. The banner must clear within ~30s without any interaction.
5. The error page from (3), **left untouched**, must recover to real
   content on its own. This is the Task 0/Task 4 payoff and the single
   most important thing to confirm.

- [ ] **Step 5: Live verification — browser-offline**

With the stack healthy, toggle DevTools' Network "Offline". The banner
must appear immediately (no two-strikes delay). Toggle back; it must
clear.

- [ ] **Step 6: Report**

Record, in the merge/PR description: the Task 0 finding, the observed
detection latency from Step 4.2, the observed recovery time from Step
4.4, and confirmation of Step 4.5. If any of Steps 3–5 fails, **stop and
report a blocking issue** rather than merging.

---

## Explicitly out of scope

- **Applying `withStaleFallback` beyond the four pages.**
  `/incidents/[id]`, the history/trends pages, `/track/*` and `/chat` are
  deferred — see Resolved open question 5 for why that is sufficient for
  v1.
- **Any change to `getSession()`, ticket, or tracked-train error
  handling** beyond the fail-closed `.catch()`es Correction 4 specifies.
  No per-user data is ever stale-served.
- **A dedicated backend health endpoint.** Spec Decision 1 reuses
  `getDataFreshness()`; that stands.
- **Changing `AutoRefresh`'s 30s cadence**, in either direction, connected
  or disconnected. Only visibility pausing is added.
- **`sw.js` / `offline.html`.** Different failure domain entirely.
- **Adding ESLint to `frontend/`.** There is none today and CI's
  "lint" step is `tsc --noEmit` (`ci.yml:219-256`). Not this plan's job.
- **Fixing `ci.yml`'s now-stale "frontend/e2e does not exist yet" comment
  and adding an e2e CI job.** Real, but a separate change.
- **Differentiated copy for offline-vs-backend-down.** One neutral
  message, per Resolved open question 6.
