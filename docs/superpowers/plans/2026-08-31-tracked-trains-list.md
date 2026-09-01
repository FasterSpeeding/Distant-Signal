# Tracked Trains List Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give a logged-in user a page listing every train they've ever tracked, most-recently-tracked first — closing a gap both prior train-tracking specs already flagged and deferred: `2026-08-29-train-tracking-frontend-design.md`'s own "Explicitly out of scope" lists *"A 'my tracked trains' list page ... The backend has no such read route today either ... not designed here"*. Today `crates/api/src/data/train_tracking.rs` has `create_pin` (every `tracked_trains` row has a real `user_id` from birth) and `tracked_train_owner` (a single-row ownership check used only by the ticket routes), but no query that lists every tracked train belonging to one user, and `frontend/app/train/` has only two single-train detail pages, both reachable only if the visitor already has a specific tracking id or (uid, date) pair. This plan builds the missing backend query/route and frontend page.

**Architecture:** One new backend query + route (`crates/api`), one new frontend page + nav item + supporting `lib/api.ts`/`lib/types.ts` additions (`frontend/`). No changes to any other crate or to the existing single-train detail pages.

```
frontend/app/track/mine/page.tsx   NEW -- async Server Comp, cookie-fwd
                                      GET /Train/mine, login nudge / empty
                                      state / list, per-row canonical link
frontend/app/layout.tsx            + TrackedTrainsNavItem (NEW, own
                                      Suspense, own guarded getSession())
frontend/lib/api.ts                + getMyTrackedTrains
frontend/lib/types.ts              + TrackedTrainListItem
        │ server-side fetch (read, cookie-fwd, no-store)
        ▼
crates/api/src/routes/train.rs     + GET /Train/mine (AuthenticatedUser-gated)
crates/api/src/data/train_tracking.rs
                                    + TrackedTrainListItem struct
                                    + list_tracked_trains_for_user query
                                    + MINE_LIST_LIMIT const
```

**Tech Stack:** Rust (axum, sqlx, `PgPool`) for the backend task; Next.js App Router + TypeScript + Mantine v9, Vitest + `@testing-library/react` + this repo's `renderWithMantine` helper (`frontend/test/render.tsx`) for the frontend tasks.

**Spec:** `docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md` — read in full before starting; this plan does not restate its research, only carries its decisions into concrete tasks. Cross-references below to "Decision N" / "Finding N" refer to that document.

**Status note:** every prerequisite this plan depends on is already live, not merely planned — confirmed by direct inspection while writing this plan (2026-08-31). `crates/api/src/routes/train.rs::router()` already mounts `/Train/track` as a literal segment above the dynamic `/Train/{tracking_id}` (Finding 5's precedent for adding `/Train/mine` the same way), and its own `router_builds_without_panicking` test already exists to catch a route-table conflict. `AuthenticatedUser` (`crates/api/src/auth.rs`) is already used by `post_track`/`post_ticket`/`get_tickets`/`get_delay_repay_estimate` in that same file. `tracked_trains_user_id` (`CREATE INDEX tracked_trains_user_id ON tracked_trains (user_id)`, `crates/api/migrations/20260828120000_train_tracking.sql` line 78) already exists and covers this plan's new query's `WHERE` clause — no new migration is needed. `frontend/app/layout.tsx` already has the `AuthNavItem`/`DataFreshnessNavItem` pattern (a separate async Server Component, its own `<Suspense>`, its own `getSession().catch(...)`-guarded call) this plan's new nav item follows exactly. `frontend/lib/api.ts` already has the cookie-forwarding, 401-tolerant read pattern (`getPreferences`, `getTicketsForTrackedTrain`) this plan's new `getMyTrackedTrains` follows.

## Global Constraints

- **No new database migration.** `tracked_trains_user_id` already exists and covers this plan's new query — do not add an index or a migration file.
- **Route shape is fixed, per Finding 5 / Decision 1:** `GET /Train/mine`, a literal segment mounted directly alongside the existing `/Train/track` literal in `crates/api/src/routes/train.rs::router()`, above `/Train/{tracking_id}` in the route list (matchit resolves a literal segment in preference to a same-position dynamic one — this ordering is not required for correctness but matches the existing file's own convention of listing `/Train/track` before `/Train/{tracking_id}`).
- **`TrackedTrainListItem` is a new struct, not a reuse or extension of `TrackedTrainState` or `TrackedTrainRef`.** Per Decision 1, its field list is fixed by the spec — do not add or drop fields. It is defined once, API-crate-side (`crates/api/src/data/train_tracking.rs`), with a hand-mirrored TypeScript counterpart in `frontend/lib/types.ts` — this repo does not generate frontend types from Rust source.
- **`pinScheduledDeparture` is a genuinely new field on the wire** (Finding 4) — no existing route selects `pin_scheduled_departure` today. Do not add it to `TrackedTrainState`, `TrackedTrainRef`, or any other existing struct as a side effect of this work; it is scoped to `TrackedTrainListItem` only.
- **Ordering is `tracked_at DESC`, not `pin_scheduled_departure`.** Per Decision 2, this is a deliberate choice, argued at length in the spec — do not resequence to scheduled-departure order.
- **No "active only" filter.** Per Finding 1 and Decision 2, `train_current_state.status` can never actually reach `'completed'` in the current codebase (a separate, already-flagged bug in `crates/trust-consumer/src/journey.rs`, not this plan's concern to fix). The list shows every tracked train the user has, unfiltered by status, capped at `MINE_LIST_LIMIT`. Do not add a status-based filter, "active" toggle, or any logic that tries to infer "still relevant" — the spec explicitly ruled this out and this plan does not re-litigate it.
- **`MINE_LIST_LIMIT` and pagination past the cap are open questions the spec deliberately left unresolved** (Open Questions 1–2), not something this plan or its tasks should resolve. Task 1 implements the cap as specified (a named constant, proposed value `100`, capping response size only) with no pagination/"load more" affordance anywhere in this plan — flagged here as inherited from the spec, not decided by this plan.
- **Route always returns `200` with a (possibly empty) array for any authenticated caller — never `404`.** Unlike the ticket routes' three-outcome shape (401 / 404-not-owner / 200), `GET /Train/mine` has only two outcomes: 401 (no session) or 200 (here's your list, however short). Do not add an ownership/404 branch — there is no id in the URL to be wrong about.
- **Frontend page path is fixed at `/track/mine`**, per Decision 3 — not a `/track` tab, not a home-page section. Do not touch `frontend/app/track/page.tsx` or `frontend/app/page.tsx` in this plan.
- **Nav item is hidden entirely when logged out, not shown-with-a-prompt.** Per Decision 4, this is a deliberate difference from `TicketPanel`'s in-page degrade pattern (a section of an already-public page) — a nav-bar entry point to an entirely-private page should not exist in the DOM at all for an anonymous visitor. Follow `AuthNavItem`/`DataFreshnessNavItem`'s exact shape: own async Server Component, own `<Suspense fallback={null}>`, own `getSession().catch(() => ({ authenticated: false, id: null, email: null, name: null }))`-guarded call — never an unguarded `getSession()` call in `layout.tsx` (the historical bug class this exact guard shape was already introduced to fix in `TicketPanel.tsx`, per the spec's Current relevant state).
- **No new refresh mechanism.** The new page is an ordinary `no-store` Server Component read, covered by the existing global `AutoRefresh` (`router.refresh()` every 30s, mounted once in `app/layout.tsx`) — no per-route opt-out, no manual "check now" button, per Decision 5.
- **Reads never go through the `/api/*` proxy.** `getMyTrackedTrains` is a server-only, cookie-forwarding read called from a Server Component, exactly like `getTrackedTrainById`/`getTicketsForTrackedTrain` — never called from a Client Component, never routed through `app/api/[...path]/route.ts`. This plan introduces no mutation and therefore needs no proxy change at all.
- **No backend changes outside `crates/api/src/data/train_tracking.rs` and `crates/api/src/routes/train.rs`.** No task may modify `crates/trust-consumer`, `crates/common`, or any migration file.
- **Out of scope, per the spec's own "Explicitly out of scope" section — no task may build any of these:** a "my tickets across all tracked trains" view (different backend gap, not this spec's), any retention/pruning job for `tracked_trains`, pagination/"load more" past `MINE_LIST_LIMIT`, filtering/search/sort controls on the list, fixing the `'completed'`-status gap in `crates/trust-consumer/src/journey.rs`, or any refresh mechanism faster than the existing global 30s `AutoRefresh`.
- **Testing convention:** Rust `#[cfg(test)]` modules colocated in the same file (matching `train_tracking.rs`'s and `train.rs`'s existing precedent — this repo's convention for this module is to unit-test pure logic without a database and leave query functions covered by integration tests elsewhere, if any exist; see Task 1's own testing note). Frontend: colocated `*.test.tsx`/`*.test.ts`, `@testing-library/react`, `renderWithMantine` (`frontend/test/render.tsx`), Vitest (`npm test` from `frontend/`). Every frontend task's verification step runs `npm test` and `npm run build` (both from `frontend/`) and requires both to pass with no new failures. Every backend task's verification step runs `cargo test -p api` and requires it to pass with no new failures.

---

### Task 1: Backend — `TrackedTrainListItem` struct and `list_tracked_trains_for_user` query

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs`

**Interfaces:**
- Produces: `pub struct TrackedTrainListItem` (public, `sqlx::FromRow` + `Serialize`, `camelCase` on the wire), `pub async fn list_tracked_trains_for_user(pool: &PgPool, user_id: &str) -> anyhow::Result<Vec<TrackedTrainListItem>>`, `const MINE_LIST_LIMIT: i64`.
- Consumed by: Task 2 (`crates/api/src/routes/train.rs`'s new `GET /Train/mine` handler).

Per Decision 1, the struct's field list is fixed by the spec — implement exactly these ten fields, in this order, with these types:

```rust
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedTrainListItem {
    pub id: i64,
    pub service_date: chrono::NaiveDate,
    pub pin_origin_crs: String,
    pub pin_destination_crs: Option<String>,
    pub pin_scheduled_departure: DateTime<Utc>,
    pub resolution_status: String,
    pub train_uid: Option<String>,
    pub status: Option<String>,
    pub delay_minutes: Option<i32>,
    pub tracked_at: DateTime<Utc>,
}
```

Deliberately excludes `train_id`, `last_reported_location`, `last_event_type`, `next_calling_point`, `eta_next`, `eta_source` — the full "where is it right now" detail stays on the single-train detail page (`TrackedTrainState`), not this list. `status`/`delay_minutes` are included (unlike `TrackedTrainRef`) because a list of a user's own trains is exactly where "is anything currently delayed" matters at a glance.

- [ ] **Step 1: Add `MINE_LIST_LIMIT` and the struct**

Add near the top of `crates/api/src/data/train_tracking.rs`, alongside the existing `MAX_PIN_AGE` constant:

```rust
/// Caps `list_tracked_trains_for_user`'s response size. No retention or
/// pruning job exists anywhere in this codebase for `tracked_trains`
/// (grepped for `DELETE FROM tracked_trains`/`prune`/`expire`/`retention`
/// -- only `ON DELETE CASCADE` foreign keys and unrelated matches turned
/// up), so this table grows without bound for as long as a user keeps
/// tracking trains, and this cap is the only bound on one HTTP response.
/// `100` is a reasonable-sounding round number, not a researched or
/// load-tested figure -- this codebase has no real-world data yet on how
/// many trains a typical user tracks over their account's lifetime. If
/// usage patterns show this is too low or unnecessarily high, revisit it
/// once real usage exists -- same posture `MAX_PIN_AGE` already took.
/// See docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md's
/// Open Questions 1-2 (also: no pagination/"load more" is designed for
/// what falls past this cap).
const MINE_LIST_LIMIT: i64 = 100;
```

Add the struct after `TrackedTrainState`/`TRACKED_TRAIN_STATE_SELECT` (i.e. after line ~298, before `get_by_tracking_id`), or any other reasonable location in the file that keeps it near its own query function — exact placement is an implementation-time judgment call, not load-bearing:

```rust
/// A user's own tracked-train list, lighter than `TrackedTrainState`
/// (Decision 1 of the design spec) -- excludes live movement detail
/// (`train_id`, `last_reported_location`, `last_event_type`,
/// `next_calling_point`, `eta_next`, `eta_source`), which belongs on the
/// single-train detail page, not a multi-row list. `pin_scheduled_departure`
/// is new here -- no other existing route selects it (Finding 4 of the
/// design spec). Lives API-crate-side only, same as `TrackedTrainState`/
/// `TrackedTrainTicket` -- never sent between Rust services, only
/// serialized to JSON for the frontend.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedTrainListItem {
    pub id: i64,
    pub service_date: chrono::NaiveDate,
    pub pin_origin_crs: String,
    pub pin_destination_crs: Option<String>,
    pub pin_scheduled_departure: DateTime<Utc>,
    pub resolution_status: String,
    pub train_uid: Option<String>,
    pub status: Option<String>,
    pub delay_minutes: Option<i32>,
    pub tracked_at: DateTime<Utc>,
}
```

- [ ] **Step 2: Add the query function**

```rust
/// A user's own tracked trains, most-recently-tracked first (`tracked_at
/// DESC`, deliberately NOT `pin_scheduled_departure` -- a train pinned a
/// month in advance would otherwise sit ahead of one pinned five minutes
/// ago for a service that's delayed right now, which is very likely the
/// one thing the caller actually wants to check on; see Decision 2 of the
/// design spec), capped at `MINE_LIST_LIMIT` rows. No status-based
/// filtering -- `train_current_state.status` can never actually reach
/// `'completed'` in this codebase today (a separate, already-flagged gap
/// in `crates/trust-consumer/src/journey.rs`, not fixed here), so an
/// "active only" filter would silently do almost nothing while implying
/// curation that isn't happening; this function intentionally does not
/// attempt one.
pub async fn list_tracked_trains_for_user(pool: &PgPool, user_id: &str) -> anyhow::Result<Vec<TrackedTrainListItem>> {
    let rows = sqlx::query_as::<_, TrackedTrainListItem>(
        "SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, \
                tt.pin_scheduled_departure, tt.resolution_status, tt.train_uid, \
                cs.status, cs.delay_minutes, tt.tracked_at \
         FROM tracked_trains tt \
         LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id \
         WHERE tt.user_id = $1 \
         ORDER BY tt.tracked_at DESC \
         LIMIT $2",
    )
    .bind(user_id)
    .bind(MINE_LIST_LIMIT)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

No new index needed — `tracked_trains_user_id` already covers the `WHERE tt.user_id = $1` clause.

- [ ] **Step 3: Testing note — no new unit test expected for this step**

Per the design spec's own Testing section and this file's existing convention: none of `create_pin`, `list_active_tracked_trains`, `get_by_tracking_id`, `get_by_uid_and_date`, or any other query function in this file has a unit test of its own (only `validate_pin`/`validate_ticket_entry`, the file's pure logic, are unit-tested without a database). `list_tracked_trains_for_user` follows that same precedent — it is pure SQL assembly with no branching logic to unit-test in isolation. Do not add a `#[sqlx::test]` or similar unless this repo already has an established integration-test harness for query functions elsewhere (confirm by grepping `crates/api` for `#[sqlx::test]` before deciding either way; if none exists, none is added here).

- [ ] **Step 4: Compile-check**

Run (from repo root): `cargo check -p api`
Expected: PASS, no warnings about unused struct/function (both are consumed in Task 2, which should be done in the same work session or immediately after — if done strictly separately, expect a transient `dead_code` warning until Task 2 lands).

- [ ] **Step 5: Run the crate's test suite**

Run (from repo root): `cargo test -p api`
Expected: PASS, including the existing `router_builds_without_panicking` and `validate_pin`/`validate_ticket_entry` tests (unaffected by this step).

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/train_tracking.rs
git commit -m "Add TrackedTrainListItem and list_tracked_trains_for_user query"
```

---

### Task 2: Backend — `GET /Train/mine` route

**Files:**
- Modify: `crates/api/src/routes/train.rs`

**Interfaces:**
- Produces: `.route("/Train/mine", axum::routing::get(get_my_tracked_trains))` mounted in `router()`; `async fn get_my_tracked_trains(...) -> Result<Json<Vec<train_tracking::TrackedTrainListItem>>, (StatusCode, String)>`.
- Consumes: `train_tracking::list_tracked_trains_for_user` (Task 1).
- Consumed by: Task 3 (`frontend/lib/api.ts`'s `getMyTrackedTrains`, functionally — at end-to-end runtime, not at compile time).

Depends on Task 1 being complete (imports `train_tracking::TrackedTrainListItem`/`list_tracked_trains_for_user`).

- [ ] **Step 1: Add the route to `router()`**

In `crates/api/src/routes/train.rs`, add `/Train/mine` as a literal segment. Per Finding 5, mount it alongside the existing `/Train/track` literal — matchit resolves literal segments in preference to same-position dynamic ones (`/Train/{tracking_id}`), so this is safe regardless of exact ordering, but for consistency with the existing file's own convention (literals listed before the dynamic segment), add it directly after `/Train/track`:

```rust
pub fn router() -> Router {
    Router::new()
        .route("/Train/track", axum::routing::post(post_track))
        .route("/Train/mine", axum::routing::get(get_my_tracked_trains))
        .route("/Train/{tracking_id}", axum::routing::get(get_by_tracking_id))
        // ... rest unchanged
```

- [ ] **Step 2: Add the handler**

Add near the other simple, single-purpose handlers (e.g. directly after `post_track`):

```rust
/// Always `200` with a (possibly empty) array for any authenticated
/// caller -- never `404`, unlike the ticket routes' "exists but not
/// yours -> 404" convention (Decision 1 of the design spec). There's no
/// id in the URL to be wrong about: the only two real outcomes are
/// "logged in, here's your list" and "not logged in, bare 401" (handled
/// by the `AuthenticatedUser` extractor itself, before this function
/// runs) -- matching `post_track`'s own two-outcome shape more closely
/// than the ticket routes' three-outcome one.
async fn get_my_tracked_trains(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<train_tracking::TrackedTrainListItem>>, (StatusCode, String)> {
    let trains = train_tracking::list_tracked_trains_for_user(&app.database, &user.id)
        .await
        .map_err(internal_error("list tracked trains"))?;
    Ok(Json(trains))
}
```

Reuses the existing `internal_error` helper already defined at the bottom of this file — no new error-mapping helper needed.

- [ ] **Step 3: Run the crate's test suite**

Run (from repo root): `cargo test -p api`
Expected: PASS. `router_builds_without_panicking` (unmodified, but now exercising the widened `router()`) is the actual regression check for Finding 5's literal-vs-dynamic-segment concern — a route-table conflict would panic this test at construction time, not silently misroute at runtime.

- [ ] **Step 4: Run the full backend build**

Run (from repo root): `cargo build --workspace`
Expected: PASS, no new warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/routes/train.rs
git commit -m "Add GET /Train/mine, session-gated via AuthenticatedUser"
```

---

### Task 3: Frontend — `lib/types.ts` and `lib/api.ts` additions

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/lib/api.ts`
- Modify: `frontend/lib/api.test.ts`

**Interfaces:**
- Produces: `TrackedTrainListItem` (type), `getMyTrackedTrains(): Promise<TrackedTrainListItem[] | null>`.
- Consumed by: Task 4 (`app/track/mine/page.tsx`), Task 5 (`TrackedTrainsNavItem` in `app/layout.tsx` uses `getSession`, already existing — this task does not touch the nav item itself, only the shared read function the page consumes).

`ResolutionStatus`/`JourneyStatus` are already defined in `frontend/lib/types.ts` (added by the train-tracking-frontend plan) — reused verbatim here, no new enum types needed.

- [ ] **Step 1: Add the type**

Add to `frontend/lib/types.ts`, after `TrackedTrainState`:

```ts
/** `GET /Train/mine`'s per-item response shape
 * (`crates/api/src/data/train_tracking.rs`'s `TrackedTrainListItem`,
 * camelCase). A deliberately lighter shape than `TrackedTrainState` --
 * excludes live movement detail (train id, last reported location, next
 * calling point, ETA), appropriate for one train's detail page, not a
 * multi-row list. `pinScheduledDeparture` is new: neither
 * `TrackedTrainState` nor any other existing route exposes it. */
export interface TrackedTrainListItem {
  id: number;
  serviceDate: string; // "YYYY-MM-DD"
  pinOriginCrs: string;
  pinDestinationCrs: string | null;
  pinScheduledDeparture: string; // RFC3339
  resolutionStatus: ResolutionStatus;
  trainUid: string | null;
  status: JourneyStatus | null;
  delayMinutes: number | null;
  trackedAt: string; // RFC3339 -- list ordering key
}
```

- [ ] **Step 2: Add `getMyTrackedTrains`**

Add `TrackedTrainListItem` to `frontend/lib/api.ts`'s existing `import type { ... } from './types';` list, then add after `getTrackedTrainByUidAndDate`:

```ts
/** `GET /Train/mine`. Returns `null` on `401` (not logged in) --
 * deliberately not `ApiNotFoundError`, matching `getTicketsForTrackedTrain`'s
 * precedent of treating "no session" as an expected outcome, not a
 * failure. Unlike that function, there is no second, distinct 404-shaped
 * outcome to also collapse into `null` here -- there's no id in this
 * route's path to be wrong about, so a 401 from this one call is the
 * complete, unambiguous signal. `app/track/mine/page.tsx` does NOT need a
 * separate `getSession()` call the way `TicketPanel` does. */
export async function getMyTrackedTrains(): Promise<TrackedTrainListItem[] | null> {
  const url = `${baseUrl()}/Train/mine`;
  const cookieHeader = (await cookies()).toString();
  const response = await fetch(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
  if (response.status === 401) {
    return null;
  }
  if (!response.ok) {
    throw errorForResponse(url, response);
  }
  return response.json() as Promise<TrackedTrainListItem[]>;
}
```

- [ ] **Step 3: Add tests**

Add `getMyTrackedTrains` to `frontend/lib/api.test.ts`'s existing import list from `./api`, then add:

```ts
it('getMyTrackedTrains fetches the correct URL, forwarding cookies, with no caching', async () => {
  incomingCookies.header = 'distant_signal_session=abc123';
  vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
  await getMyTrackedTrains();
  expect(fetch).toHaveBeenCalledWith(
    'http://test-api:8080/Train/mine',
    expect.objectContaining({
      cache: 'no-store',
      headers: { Cookie: 'distant_signal_session=abc123' },
    }),
  );
});

it('getMyTrackedTrains returns null on a 401 (not logged in)', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('no session', { status: 401 })));
  await expect(getMyTrackedTrains()).resolves.toBeNull();
});

it('getMyTrackedTrains resolves an empty array as logged-in-with-nothing-tracked, not null', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
  await expect(getMyTrackedTrains()).resolves.toEqual([]);
});

it('getMyTrackedTrains still throws on a non-401 failure', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('server error', { status: 500 })));
  await expect(getMyTrackedTrains()).rejects.toThrow(/500/);
});
```

(Match the exact helper names/shape `incomingCookies`/cookie-stubbing already use elsewhere in this file — copy the pattern from the adjacent `getTicketsForTrackedTrain` tests rather than reintroducing a new stubbing convention.)

- [ ] **Step 4: Run the test suite**

Run (from `frontend/`): `npm test -- api.test.ts`
Expected: all tests, including the four new ones, PASS.

- [ ] **Step 5: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/api.ts frontend/lib/api.test.ts
git commit -m "Add TrackedTrainListItem type and getMyTrackedTrains read function"
```

---

### Task 4: Frontend — `/track/mine` page

**Files:**
- Create: `frontend/app/track/mine/page.tsx`
- Create: `frontend/app/track/mine/page.test.tsx`

**Interfaces:**
- Consumes: `getMyTrackedTrains` (Task 3).
- Produces: default-exported async Server Component rendering the three outcomes — login nudge (`null`), empty state (`[]`), populated list.
- Consumed by: Task 5 (`TrackedTrainsNavItem` links here).

Depends on Task 3 being complete.

Per Decision 3, this page does **not** need a separate `getSession()` call the way `TicketPanel` does — `getMyTrackedTrains()`'s null-on-401 return is the complete signal, since there's no second party (owner-vs-not-owner) to disambiguate on a route with no id in its path.

Per the design's row-linking logic: `resolutionStatus === 'resolved' && trainUid` links to `/train/{trainUid}/{serviceDate}` (the canonical, shareable URL); otherwise (`pending`/`unresolved`, or a defensive `resolved`-with-null-`trainUid`) links to `/train/by-id/{id}`.

- [ ] **Step 1: Write the page**

Create `frontend/app/track/mine/page.tsx`:

```tsx
import { Badge, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import { getMyTrackedTrains } from '@/lib/api';
import { TextLink } from '@/components/TextLink';
import { formatDate, formatTime } from '@/lib/dateFormat';
import type { TrackedTrainListItem } from '@/lib/types';

/** `/track/mine` -- a logged-in user's own tracked trains, most-recently-
 * tracked first, per
 * docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md
 * Decision 3. `getMyTrackedTrains()` returning `null` on a `401` is the
 * COMPLETE "not logged in" signal for this page -- unlike
 * `components/TicketPanel.tsx`, no separate `getSession()` call is needed
 * here, since there's no second party (owner vs. not) to disambiguate on
 * a route with no id in its path (Decision 3's own note). */
export default async function MyTrackedTrainsPage() {
  const trains = await getMyTrackedTrains();

  if (trains === null) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>My Tracked Trains</Title>
        <TextLink href="/api/auth/login" underline="always">
          Log in to see the trains you&apos;re tracking
        </TextLink>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>My Tracked Trains</Title>
      {trains.length === 0 ? (
        <Text c="dimmed">
          You haven&apos;t tracked any trains yet. <Link href="/track">Track a train</Link> to get started.
        </Text>
      ) : (
        <Stack gap="xs">
          {trains.map((train) => (
            <TrackedTrainListRow key={train.id} train={train} />
          ))}
        </Stack>
      )}
    </Stack>
  );
}

function TrackedTrainListRow({ train }: { train: TrackedTrainListItem }) {
  // Canonical, shareable URL once resolved; the by-id detail route
  // otherwise -- matching the existing by-id page's own "canonical link
  // once resolved" logic rather than always sending the user through the
  // by-id redirect hop. The `resolved`-with-null-`trainUid` fallback is
  // defensive: the backend's own resolution invariant means this
  // shouldn't happen, but this component doesn't assume it.
  const href =
    train.resolutionStatus === 'resolved' && train.trainUid
      ? `/train/${train.trainUid}/${train.serviceDate}`
      : `/train/by-id/${train.id}`;

  const route = train.pinDestinationCrs
    ? `${train.pinOriginCrs} → ${train.pinDestinationCrs}`
    : train.pinOriginCrs;

  return (
    <Link href={href} style={{ textDecoration: 'none', color: 'inherit' }}>
      <Stack gap={4} p="sm" style={{ border: '1px solid var(--mantine-color-default-border)', borderRadius: 8 }}>
        <Group justify="space-between" wrap="nowrap">
          <Text fw={500}>{route}</Text>
          <RowStatusBadge train={train} />
        </Group>
        <Text size="sm" c="dimmed">
          {formatDate(train.serviceDate)} · {formatTime(train.pinScheduledDeparture)}
        </Text>
      </Stack>
    </Link>
  );
}

function RowStatusBadge({ train }: { train: TrackedTrainListItem }) {
  // `pending`/`unresolved` show the resolution status itself -- no
  // journey status exists yet for either. Once `resolved`, the journey
  // `status` plus a delay badge takes over, reusing the same "Xm
  // late"/"On time" treatment `TrainJourney.tsx`'s `JourneyDetails`
  // already uses. No "active only" filter and no attempt to distinguish
  // a genuinely-finished journey from one that's merely gone quiet -- per
  // Decision 2/Finding 1 of the design spec, the backend can't honestly
  // support that distinction today.
  if (train.resolutionStatus !== 'resolved') {
    return (
      <Badge color={train.resolutionStatus === 'unresolved' ? 'red' : 'gray'} variant="light">
        {train.resolutionStatus}
      </Badge>
    );
  }
  return (
    <Group gap={6} wrap="nowrap">
      {train.status && (
        <Badge color="gray" variant="light">
          {train.status}
        </Badge>
      )}
      {train.delayMinutes !== null && (
        <Badge color={train.delayMinutes > 0 ? 'orange' : 'green'} variant="light">
          {train.delayMinutes > 0 ? `${train.delayMinutes}m late` : 'On time'}
        </Badge>
      )}
    </Group>
  );
}
```

**Implementation-time verification note:** confirm `formatDate`/`formatTime` (`frontend/lib/dateFormat.ts`) exist with these exact signatures — both are already used by `TrainJourney.tsx`/`EtaBadge.tsx` per the train-tracking-frontend plan, so this should be a straight reuse, not new code; adjust the calls if the actual signatures differ. The row's inline border styling is one reasonable presentational choice, not a spec requirement — swap it for an existing `Card`/list styling convention if this codebase has one already established elsewhere that better fits (check `frontend/app/page.tsx`'s pinned-lines/pinned-stations list rendering for a precedent before inventing new inline styles).

- [ ] **Step 2: Write the tests**

Create `frontend/app/track/mine/page.test.tsx`, mocking `@/lib/api` and calling the async page function directly (the same "await the async Server Component, then render" technique `TicketPanel.test.tsx` established, per the journey-ticket-tracking-frontend plan):

```tsx
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import MyTrackedTrainsPage from './page';
import * as api from '@/lib/api';
import type { TrackedTrainListItem } from '@/lib/types';

vi.mock('@/lib/api');

function item(overrides: Partial<TrackedTrainListItem> = {}): TrackedTrainListItem {
  return {
    id: 1,
    serviceDate: '2026-08-31',
    pinOriginCrs: 'WAT',
    pinDestinationCrs: 'WOK',
    pinScheduledDeparture: '2026-08-31T18:32:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'C21373',
    status: 'en_route',
    delayMinutes: 4,
    trackedAt: '2026-08-31T12:00:00Z',
    ...overrides,
  };
}

describe('MyTrackedTrainsPage', () => {
  it('null (not logged in): shows a login nudge', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(
      screen.getByRole('link', { name: "Log in to see the trains you're tracking" }),
    ).toHaveAttribute('href', '/api/auth/login');
  });

  it('empty array: shows the empty state with a working link to /track', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText(/haven't tracked any trains yet/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Track a train' })).toHaveAttribute('href', '/track');
  });

  it('resolved train with a trainUid: links to the canonical /train/{uid}/{date} URL', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item()]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute(
      'href',
      '/train/C21373/2026-08-31',
    );
  });

  it('pending train: links to the by-id detail route', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      item({ resolutionStatus: 'pending', trainUid: null, status: null, delayMinutes: null }),
    ]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute('href', '/train/by-id/1');
  });

  it('resolved with a null trainUid (defensive case): falls back to the by-id route', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item({ trainUid: null })]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute('href', '/train/by-id/1');
  });

  it('renders a delay badge for a resolved, delayed train', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item({ delayMinutes: 12 })]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText('12m late')).toBeInTheDocument();
  });

  it('renders the resolutionStatus badge for a pending/unresolved train', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      item({ resolutionStatus: 'unresolved', trainUid: null, status: null, delayMinutes: null }),
    ]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText('unresolved')).toBeInTheDocument();
  });
});
```

- [ ] **Step 3: Run the tests**

Run (from `frontend/`): `npm test -- page.test.tsx`
Expected: all seven tests PASS. (If this glob matches other `page.test.tsx` files elsewhere in the tree, scope it further, e.g. `npm test -- track/mine/page.test.tsx`.)

- [ ] **Step 4: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/track/mine/page.tsx frontend/app/track/mine/page.test.tsx
git commit -m "Add /track/mine page listing a user's own tracked trains"
```

---

### Task 5: Frontend — session-gated nav item

**Files:**
- Modify: `frontend/app/layout.tsx`

**Interfaces:**
- Produces: `TrackedTrainsNavItem` (new async Server Component, local to `layout.tsx`, matching `AuthNavItem`/`DataFreshnessNavItem`'s existing shape), wrapped in `<Suspense fallback={null}>`, placed in the nav `Group` next to the existing `<TextLink href="/track">Track a Train</TextLink>`.

Depends on Task 4 being complete (links to `/track/mine`, which must exist for the link to be meaningful — though this task would still compile/test fine on its own if done first, since it's a plain `<TextLink>` with a hardcoded href).

Per Decision 4: hidden entirely (returns `null`) when logged out, not shown-with-a-login-prompt — a deliberate difference from `TicketPanel`'s in-page degrade pattern, because this is a full nav-bar entry point to a page whose entire content is private, not a section of an already-public page.

- [ ] **Step 1: Read the current `AuthNavItem`/`DataFreshnessNavItem` block to confirm the exact surrounding structure**

Re-read `frontend/app/layout.tsx` around lines 39-108 (already confirmed present as of 2026-08-31) to get the precise `Group`/`Suspense` nesting and the `getSession()` guard shape to mirror exactly.

- [ ] **Step 2: Add `TrackedTrainsNavItem`**

Add a new function alongside `AuthNavItem`/`DataFreshnessNavItem` (same file, same pattern):

```tsx
// Same rationale as AuthNavItem/DataFreshnessNavItem: a separate async
// Server Component so <Suspense> can stream the session check in without
// blocking the rest of the shell. Unlike those two, this one renders
// nothing at all when logged out (Decision 4 of
// docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md) --
// this is a full nav-bar entry point to a page whose entire content is
// private to the viewer, not a section of an already-public page (the
// TicketPanel pattern), so showing it to every visitor and having it
// always resolve to a login nudge would be dead weight for the common
// case of an anonymous visitor. Guarded with the same .catch() shape as
// AuthNavItem/DataFreshnessNavItem: a root layout has no route-level
// error.tsx, so an unguarded getSession() here could take down every
// page's nav bar on an auth glitch -- the same historical bug class
// already fixed in TicketPanel.tsx, not repeated here.
async function TrackedTrainsNavItem() {
  const session = await getSession().catch(() => ({
    authenticated: false,
    id: null,
    email: null,
    name: null,
  }));
  if (!session.authenticated) {
    return null;
  }
  return <TextLink href="/track/mine">My Tracked Trains</TextLink>;
}
```

- [ ] **Step 3: Mount it in the nav `Group`**

Add, next to the existing `<TextLink href="/track">Track a Train</TextLink>`:

```tsx
<Suspense fallback={null}>
  <TrackedTrainsNavItem />
</Suspense>
```

No skeleton fallback (unlike `DataFreshnessNavItem`'s icon or `AuthNavItem`'s "Log in" text) — there's no harmless placeholder for a link whose very presence depends on the still-pending fetch; render nothing until it resolves rather than flash a link that might immediately disappear, per Decision 4.

This duplicates the `getSession()` call `AuthNavItem` already makes on every page load. Accepted as harmless per Decision 4/Open Question 4: Next.js's per-request `fetch` deduplication means two calls to the same URL with the same options within one render pass share a single underlying network request. This plan does not add any new caching mechanism to avoid the duplicate call, per the spec's own explicit acceptance of this — flagged in the spec as asserted rather than benchmarked against this app's specific setup, worth a quick sanity check at implementation time if nav-bar latency ever becomes a concern, but not a blocker for this task.

- [ ] **Step 4: Write/extend a render test for session-conditional nav rendering**

Follow whatever existing test file already covers `layout.tsx`'s nav-conditional rendering (check for an `AuthStatus.test.tsx` or `layout.test.tsx`-shaped precedent per the design spec's own Testing section — *"mirroring `AuthStatus.test.tsx`'s existing shape for session-conditional nav rendering"*). If such a file exists, extend it with:

```tsx
it('hides "My Tracked Trains" when logged out', async () => {
  // ... mock getSession to return { authenticated: false, ... }
  // ... render the layout / relevant nav subtree
  expect(screen.queryByRole('link', { name: 'My Tracked Trains' })).not.toBeInTheDocument();
});

it('shows "My Tracked Trains", pointing at /track/mine, when logged in', async () => {
  // ... mock getSession to return { authenticated: true, ... }
  // ... render the layout / relevant nav subtree
  expect(screen.getByRole('link', { name: 'My Tracked Trains' })).toHaveAttribute('href', '/track/mine');
});
```

If no such file exists yet for `layout.tsx`'s nav items (confirm by searching for existing tests exercising `AuthNavItem`/`DataFreshnessNavItem` directly before assuming one does), this is new test surface — write a minimal `TrackedTrainsNavItem`-scoped test using the same "await the async function directly, then render" technique used elsewhere in this plan (Task 4) and in the ticket-tracking-frontend plan's `TicketPanel.test.tsx`, rather than skipping coverage.

- [ ] **Step 5: Run the tests**

Run (from `frontend/`): `npm test -- layout` (or whatever the actual test file is named — confirm the correct invocation once Step 4 locates/creates it)
Expected: all tests, including the new ones, PASS.

- [ ] **Step 6: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
git add frontend/app/layout.tsx
git commit -m "Add session-gated 'My Tracked Trains' nav item"
```

(Add the layout's test file to the `git add` list too if Step 4 modified or created one.)

---

## Sequencing notes

- Tasks 1 → 2 are backend-only and must run in that order (Task 2 imports Task 1's struct/function).
- Task 3 (frontend types/api) can start once Task 2's route shape is known — it does not require Task 1/2 to actually be merged to write against, since the wire contract is already fully specified in the design spec, but the honest end-to-end path (a working `GET /Train/mine` to hit) requires Tasks 1–2 done first if anyone wants to manually verify Task 3 against a live backend rather than mocked `fetch`.
- Task 4 depends on Task 3 (imports `getMyTrackedTrains`/`TrackedTrainListItem`).
- Task 5 depends on Task 4 only for the link target to be meaningful (`/track/mine` should exist); it has no import-level dependency on Task 4 and could be built in parallel if needed.
- Overall recommended order: 1, 2, 3, 4, 5 — matching the dependency chain above and the backend-then-frontend structure both precedent plans (`2026-08-29-train-tracking-frontend.md`, `2026-08-30-journey-ticket-tracking-frontend.md`) use, adapted here since (unlike those two) this feature's backend is not yet built and is in scope for this plan.

## Open questions carried forward from the spec (not resolved by this plan)

1. **`MINE_LIST_LIMIT`'s value (`100`, Task 1 Step 1) is the spec's own proposed round number, not researched or load-tested.** This plan implements it as specified rather than picking a different number or attempting to research one — revisit once real usage data exists, per the spec's Open Question 1.
2. **No pagination/"load more" is designed anywhere in this plan for what falls past the cap**, per the spec's Open Question 2 — a user with more than `MINE_LIST_LIMIT` tracked trains simply can't reach the older ones through `/track/mine`; their individual detail pages remain reachable if the URL/tracking id is already known.
3. **Whether `delayMinutes`/`status` per row is enough "at a glance" signal**, or a fuller ETA-per-row treatment is eventually wanted, is the spec's own Open Question 3 — Task 4 implements the narrower version (delay + status only) as specified, not a judgment call made by this plan.
