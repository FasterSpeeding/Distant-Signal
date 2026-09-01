# Design: Tracked Trains List

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md`
(the closest analog — a real frontend feature built on top of already-shipped
backend infrastructure, same team, same session, same conventions) and, one
level further back, `docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md`.
No implementation plan is included; that is a separate, later step in this
repo's process.

## Goal

Individual train tracking (`docs/superpowers/specs/2026-08-28-train-tracking-design.md`,
`docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md`) and
journey ticket tracking (`docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md`)
are both complete, working, end-to-end features — a user can pin a train,
watch its live progress, and attach a ticket with a Delay Repay estimate.
But there is no page listing a user's own tracked trains. Confirmed by
direct inspection: `frontend/app/train/` contains exactly two page files,
`[uid]/[date]/page.tsx` and `by-id/[trackingId]/page.tsx`, both single-train
detail views reachable only if the visitor already has a specific tracking
id or (uid, date) pair — from `/track`'s search-form redirect, a shared
link, or a bookmark. `crates/api/src/data/train_tracking.rs` has
`create_pin` (which stores a real `user_id` per pin, `NOT NULL` from birth)
and `tracked_train_owner` (a single-row ownership check used only by the
ticket routes), but **no query that lists every tracked train belonging to
one user** — confirmed by reading the file in full; the closest thing,
`list_active_tracked_trains`, is deliberately unscoped-by-user (it's
`trust-consumer`'s own polling reference-reload, covering every user's
active pins at once — see Current relevant state). A user who tracks a
train today has no way to come back next week and see "here's what I'm
tracking" without having saved the URL themselves. This spec designs the
missing backend query/route and frontend page that closes that gap.

Both frontend design docs above independently already flagged this as
future work rather than pretending it was solved:
`2026-08-29-train-tracking-frontend-design.md`'s own "Explicitly out of
scope" lists *"A 'my tracked trains' list page ... The backend has no such
read route today either ... not designed here"*, and
`2026-08-29-journey-ticket-tracking-frontend-design.md`'s own out-of-scope
section repeats the same gap for tickets specifically (*"A 'my tickets
across all tracked trains' view ... out of scope for a frontend-only
spec"*). This spec is that deferred follow-up, scoped to trains (not
tickets — see Explicitly out of scope).

## Corrections / findings from direct investigation

Following this repo's established "Corrections" precedent
(`2026-08-29-train-tracking-frontend-design.md`, `2026-08-29-journey-ticket-tracking-frontend-design.md`):
things the brief's framing left open that direct reading resolved,
materially shaping the design below.

1. **There is no reliable signal for "this tracked train is done, stop
   showing it as active."** `2026-08-29-train-tracking-frontend-design.md`'s
   own Decision 3 table and Open Question 2 already documented this for the
   single-train page, but it's worth restating because it's load-bearing
   for *this* spec's scoping decision too: `train_current_state.status`'s
   schema allows `'completed'`, but `crates/trust-consumer/src/journey.rs`'s
   `apply_movement` never emits it — every non-cancelled journey sits at
   `'en_route'` indefinitely, including ones that plainly finished hours or
   days ago. A list that tried to filter to "only active/current trains" by
   excluding `completed`/`cancelled` (the same predicate
   `list_active_tracked_trains` uses for its own, different purpose — see
   below) would therefore **never** hide a finished-but-not-cancelled
   journey; it would only ever hide `cancelled` ones. That predicate solves
   a real problem for `trust-consumer` (nothing further to *poll* for) but
   does not solve "what should a human see" — it can't, given what the data
   actually distinguishes today. This directly rules out "filter to
   active only" as a scoping strategy for the list — see Decision 2.
2. **`list_active_tracked_trains` and its `TrackedTrainRef` shape are a
   precedent to read, not a query to reuse or extend.** It answers "every
   pin any user has active," used only by `trust-consumer`'s reference
   reload, never scoped by `user_id`, and lives behind no auth check at all
   (it's an internal function called from a different binary via a shared
   `PgPool`, not exposed over HTTP). Its `WHERE tt.resolution_status !=
   'unresolved' AND (cs.status IS NULL OR cs.status NOT IN ('completed',
   'cancelled'))` filter is specifically about "does trust-consumer still
   have work to do here," not "should a user see this in their list" — per
   Finding 1, applying it to a user-facing list would silently and
   permanently hide unresolved pins (a real, useful thing for a user to
   see and re-check) and give a false sense that "no `cancelled`/`completed`
   in the list" means "still worth watching," when in practice it mostly
   means "hasn't been explicitly cancelled." This spec's own query and
   struct (Decision 1) mirror `TrackedTrainRow`/`TrackedTrainRef`'s
   *pattern* (a private `sqlx::FromRow` row struct, a thin mapping
   function) without reusing either the query's `WHERE` clause or its
   shape.
3. **No pruning, deletion, or retention job exists anywhere in this
   codebase for `tracked_trains` (or its child tables).** Grepped
   `crates/` for `DELETE FROM tracked_trains`, `prune`, `expire`,
   `retention`, `cleanup` — the only hits are `ON DELETE CASCADE` foreign
   keys (`train_movement_events`/`train_current_state`/
   `tracked_train_tickets` clean up if a `tracked_trains` row is ever
   deleted, but nothing ever deletes one) and unrelated matches
   (`trust-consumer`'s own module docs). `2026-08-29-train-tracking-frontend-design.md`'s
   own Open Question 3 already flagged this from the single-train-page
   side: *"no existing retention policy elsewhere in this repo... the
   90-day figure is a starting proposal, not a researched one"* (referring
   to the backend design doc's own unimplemented proposal). **This means
   `tracked_trains` grows without bound for as long as a user keeps
   tracking trains, and a "list everything" query has no natural database-
   side cutoff to lean on.** This directly shapes Decision 2 below (a
   capped, most-recent-first list, not a full history browser) — the cap
   is this spec's own choice to bound one response, not a statement that
   older rows get deleted.
4. **`TrackedTrainState` (the single-train read model) does not expose
   `pin_scheduled_departure`** — confirmed both in
   `crates/api/src/data/train_tracking.rs` (`TRACKED_TRAIN_STATE_SELECT`
   selects `service_date` but not `pin_scheduled_departure`) and in
   `frontend/lib/types.ts`'s own doc comment on `TrackedTrainState`
   (*"there is no `scheduledDeparture` field -- the backend's read query
   does not select `pin_scheduled_departure`, only `serviceDate`"*). A list
   view showing several pins needs the actual scheduled time to
   distinguish them (two trains pinned for the same `service_date` are
   common — an early one and a late one) and to sort meaningfully, so this
   spec's new list-item shape (Decision 1) selects and exposes
   `pin_scheduled_departure` for the first time on any public route — a
   genuinely new field on the wire, not a repackaging of an existing one.
5. **The `/Train/mine` route shape (a literal segment sitting alongside
   `/Train/{tracking_id}`, a dynamic one) already has a working precedent
   in this exact router.** `crates/api/src/routes/train.rs::router()`
   already mounts `.route("/Train/track", post(post_track))` directly
   above `.route("/Train/{tracking_id}", get(get_by_tracking_id))`, and its
   own test, `router_builds_without_panicking`, exists specifically to
   catch an axum/matchit route-table conflict at `cargo test` time. axum's
   underlying router (`matchit`) resolves a literal segment in preference
   to a same-position dynamic one, so `/Train/mine` can be added the same
   way `/Train/track` already was, with no risk of it being swallowed by
   `Path<i64>` on `/Train/{tracking_id}` (a non-numeric segment there would
   fail to parse anyway, but literal-priority means it never gets the
   chance to try).

## Current relevant state (verified 2026-08-31)

**Backend (`crates/api`)**, `/Train/...` mounted directly on the root
router in `main.rs` (`.merge(routes::train::router())`, alongside
`line_status::router()` — not nested under `/public`), per
`crates/api/src/routes/train.rs`'s own module doc:

- `create_pin(pool, pin, user_id)` — every `tracked_trains` row has a real,
  `NOT NULL` `user_id` from birth (`tracked_trains.user_id TEXT NOT NULL
  REFERENCES users(id)`, `crates/api/migrations/20260828120000_train_tracking.sql`),
  indexed (`tracked_trains_user_id`).
- `tracked_train_owner(pool, tracking_id)` — single-row ownership check,
  used only by the ticket routes (`POST`/`GET .../tickets`) to answer
  "does this tracked train exist and belong to the caller" before touching
  ticket data. Not a list; returns one `Option<String>` (the owner's
  `user_id` or `None`).
- `list_active_tracked_trains(pool)` — **no `user_id` parameter at all**,
  returns `Vec<TrackedTrainRef>` for every user's active pins combined.
  `trust-consumer`-facing only, called via a shared `PgPool` from a
  different binary, never exposed over HTTP. See Finding 2 above for why
  this is a pattern precedent, not a reusable query.
- `get_by_tracking_id` / `get_by_uid_and_date` — the two existing public,
  unauthenticated, unscoped single-train reads (`GET /Train/{trackingId}`,
  `GET /Train/by-uid/{uid}/{date}`), returning the full `TrackedTrainState`
  (14 fields, including live movement data). Neither leaks `user_id` — see
  that struct's own doc comment.
- **No query or route exists today that answers "list of tracked trains
  belonging to user X."**

**Auth**: `AuthenticatedUser` (`crates/api/src/auth.rs`), the extractor
every ownership-scoped route already uses (`post_track`, `post_ticket`,
`get_tickets`, `get_delay_repay_estimate`). Rejects with a plain-text `401`
(`(StatusCode::UNAUTHORIZED, "no session")` or `"session expired or
unknown"`) when there's no valid session cookie — no JSON body, matching
every other `AuthenticatedUser`-gated route in this file.

**Frontend**: no `/track/mine`-shaped (or equivalent) route exists in
`frontend/app/` today — confirmed, `frontend/app/train/` has only the two
detail pages, and `frontend/app/track/page.tsx` is the search-form entry
point only (`TrackTrainForm`, no listing of anything). `frontend/lib/api.ts`
has no function reading anything scoped to "my trains" (only
`getTrackedTrainById`/`getTrackedTrainByUidAndDate`, both public/unscoped).

**Established, directly-reusable frontend conventions** (verified in code):

- `frontend/lib/api.ts`'s cookie-forwarding pattern (`getSession`,
  `getPreferences`, `getTicketsForTrackedTrain`, `getDelayRepayEstimate`):
  a Server Component's own `fetch` does not inherit the incoming request's
  cookies, so a per-user read manually re-attaches
  `(await cookies()).toString()` as a `Cookie` header.
- `frontend/lib/api.ts`'s `getPreferences()`/`getTicketsForTrackedTrain()`
  401-tolerance pattern: `401` → return a documented sentinel (`{
  pinnedLines: [], pinnedStations: [] }` / `null`) rather than throwing,
  since "not logged in" is a normal, expected outcome for these reads, not
  a failure.
- `app/layout.tsx`'s `AuthNavItem`/`DataFreshnessNavItem` pattern: a
  separate `async` Server Component per piece of nav-bar data that needs
  its own fetch, each wrapped in its own `<Suspense>` so one slow/failing
  fetch can't block the rest of the shell, and each guarding its own
  `getSession()`/`getDataFreshness()` call with `.catch(() => <fallback
  shape>)` — a root layout has no route-level `error.tsx` of its own, so
  an unguarded call here would take down every page on an auth or freshness
  glitch. **This is the exact class of bug
  `2026-08-29-journey-ticket-tracking-frontend-design.md` found and fixed
  in `TicketPanel.tsx`** (an unguarded `getSession()` that could take down
  a whole page) — every new `getSession()` call this spec adds follows the
  already-guarded shape, not the historical unguarded one.
- `frontend/app/page.tsx`'s pinned-lines/pinned-stations empty-state
  pattern: `list.length === 0` → a `<Text c="dimmed">` sentence naming what
  to do next, with an inline `<Link>`/`<TextLink>` to the place that
  creates the first item — not a blank section.
- `frontend/components/TicketPanel.tsx`'s login-nudge pattern: a plain
  `<TextLink href="/api/auth/login" underline="always">` with copy that
  doesn't overpromise what logging in guarantees.

## Decisions

### 1. Backend: a new lightweight list-item shape and query, not a reuse of `TrackedTrainState` or `TrackedTrainRef`

**New struct, `TrackedTrainListItem`, in `crates/api/src/data/train_tracking.rs`**
(private row-mapping precedent: same pattern as `TrackedTrainRow`/
`TrackedTrainRef`, i.e. a `#[derive(sqlx::FromRow)]` struct built directly
as the public wire type here since, unlike `TrackedTrainRef`, this one has
no `crates/common` counterpart to satisfy — it's never sent between Rust
services, only serialized to JSON for the frontend, matching
`TrackedTrainState`/`TrackedTrainTicket`'s own precedent of living
API-crate-side only):

```rust
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedTrainListItem {
    pub id: i64,
    pub service_date: chrono::NaiveDate,
    pub pin_origin_crs: String,
    pub pin_destination_crs: Option<String>,
    pub pin_scheduled_departure: DateTime<Utc>,   // NEW on the wire -- see Finding 4
    pub resolution_status: String,
    pub train_uid: Option<String>,
    pub status: Option<String>,        // journey status, from train_current_state
    pub delay_minutes: Option<i32>,    // quick "how's it doing" glance per row
    pub tracked_at: DateTime<Utc>,     // when this pin was created -- see Decision 2
}
```

Deliberately excludes `train_id` (TRUST's internal daily identifier, never
rendered anywhere in the existing UI — `TrainJourney.tsx` never shows it),
`last_reported_location`/`last_event_type`/`next_calling_point`/`eta_next`/
`eta_source` (the full "where is it right now" detail, appropriate for one
train's detail page, not a multi-row list — a list row shows enough to
decide "which of these do I want to open," the detail page shows the rest,
exactly the same "lighter shape for a list, fuller shape for one item"
relationship `TrackedTrainRef` already has to `TrackedTrainState`, just
built for a different axis (per-user list vs. poller reference set)).
`status` and `delay_minutes` are included (unlike `TrackedTrainRef`, which
has neither) because a list of a user's *own* trains is exactly the place
"is anything currently delayed" matters at a glance — `TrackedTrainRef`
doesn't need this since `trust-consumer` doesn't render it to anyone.

**New query function**:

```rust
/// A user's own tracked trains, most-recently-tracked first, capped at
/// MINE_LIST_LIST_LIMIT rows -- see this function's own doc/Decision 2 for
/// why (no retention/pruning job exists anywhere in this codebase, per
/// this spec's Finding 3, so this cap is the only bound on an otherwise
/// unbounded-growth query).
pub async fn list_tracked_trains_for_user(
    pool: &PgPool,
    user_id: &str,
) -> anyhow::Result<Vec<TrackedTrainListItem>> {
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

No new index needed: `tracked_trains_user_id` (`CREATE INDEX
tracked_trains_user_id ON tracked_trains (user_id)`) already exists from
the original train-tracking migration — this query's `WHERE` clause is
covered.

**New route**: `GET /Train/mine`, session-gated via `AuthenticatedUser`
(same extractor, same bare-401-on-no-session shape as `POST /Train/track`
and the ticket routes — no ownership check needed beyond the extractor
itself, since "list *my* trains" has no second party whose ownership could
ever be in question, unlike the ticket routes' `tracking_id` path
parameter). Mounted in `router()` as a literal segment alongside the
existing `/Train/track` literal (see Finding 5 for why this is safe):

```rust
.route("/Train/mine", axum::routing::get(get_my_tracked_trains))
```

```rust
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

Always `200` with a (possibly empty) array for any authenticated caller —
never a `404`, unlike the ticket routes' "exists but not yours → 404"
convention: there's no id in the URL to be wrong about, so the only two
real outcomes are "logged in, here's your list" and "not logged in, bare
401," matching `POST /Train/track`'s own two-outcome shape more closely
than the ticket routes' three-outcome one.

### 2. Scope: everything a user has ever tracked, most-recently-tracked first, capped — not an "active only" filter

Two real options were considered for "what does this list show":

- **Filter to "still active/relevant" trains** (excluding
  `cancelled`/`completed`, mirroring `list_active_tracked_trains`'s own
  predicate). **Rejected** — per Finding 1, `'completed'` is never actually
  emitted by `trust-consumer` today, so this filter would only ever exclude
  `cancelled` trains; every journey that finished normally would stay
  "active" forever and never leave the list. This isn't a reasonable
  approximation of "still relevant," it's a filter that silently does
  almost nothing while implying it does something — worse than no filter
  at all, because it would read as intentional curation that isn't
  actually happening.
- **Show everything, most-recently-tracked first, capped at a fixed limit
  (`MINE_LIST_LIMIT`, proposed `100`).** **Chosen.** No status-based
  filtering (the data can't support it honestly, per above) — a `pending`
  pin from three days ago that never resolved is exactly as visible as one
  from ten minutes ago, and a `resolved`/`en_route` pin from a service that
  plainly finished last week stays visible too. This is honest about what
  the backend can actually tell the frontend, rather than pretending a
  distinction exists that the data doesn't support. The cap exists purely
  to bound one HTTP response's size against unbounded growth (Finding 3),
  not to imply "older than this doesn't matter" — see Open questions for
  what happens past the cap.

**Ordering: `tracked_at DESC` (most recently created pin first), not
`pin_scheduled_departure`.** Considered and rejected: sorting by scheduled
departure would put a train pinned a month in advance (an explicit,
supported use case — `validate_pin` allows an arbitrarily-far-future
`scheduled_departure`) ahead of one pinned five minutes ago for a service
that's delayed right now, which is very likely the one thing the user
actually opened this page to check on. `tracked_at DESC` has no such
inversion — it's always "what did I do most recently," a single,
unambiguous signal that needs no interpretation of `resolution_status`/
`status` (which, per Finding 1, can't reliably distinguish "needs
attention" from "long done" anyway). This does mean a train scheduled for
next week that was pinned first will sit below one pinned a minute ago for
a train departing later today — accepted as the simpler, more predictable
rule; not revisited further here.

### 3. Frontend: a new page at `/track/mine`, not a `/track` tab or a home-page section

Three real placements were considered:

- **A section on the home page (`/`)**, alongside the existing "Your
  Lines"/"Your Stations" pinned sections. **Rejected.** Those sections
  read `getPreferences()`, which is 401-tolerant *by design* because an
  anonymous visitor legitimately has "zero pinned lines/stations" — the
  home page renders identically in shape whether or not you're logged in,
  just with empty lists. Tracked trains are different: an anonymous
  visitor has no session to scope a query to at all, not just an empty
  result, so bolting a third section onto an otherwise anonymous-friendly
  page would either need to silently hide the section for anonymous
  visitors (fine) or show a login nudge inline (breaks that page's
  existing "renders the same shape for everyone" character). A dedicated
  page keeps that page's existing contract intact.
- **A tab/toggle inside `/track`** (alongside `TrackTrainForm`).
  **Rejected.** `/track` is explicitly the *anonymous-reachable* entry
  point today — nothing about it requires a session (only the final
  `POST /Train/track` submit does, gated inline via `TrackTrainForm`'s own
  `needsLogin` pattern). Folding a session-gated list into the same page
  would mean the page's own top-level identity becomes "sometimes just a
  form, sometimes a form plus a private list," which is a bigger
  behavioral change to an existing, already-shipped page than a new
  route needs to be.
- **A new page, `/track/mine`.** **Chosen.** Sits under the same `/track`
  segment as the existing search-form entry point (consistent grouping —
  both are about "trains I'm tracking," one for starting a new one, one
  for reviewing existing ones), without touching `/track/page.tsx` itself.
  Matches this app's existing lowercase URL convention
  (`/train/by-id/...`, `/train/[uid]/[date]`) rather than mirroring
  `/Train/mine` verbatim, same reasoning
  `2026-08-29-train-tracking-frontend-design.md`'s Decision 2 already gave
  for the two existing train pages.

**`frontend/app/track/mine/page.tsx`** (async Server Component):

```tsx
export default async function MyTrackedTrainsPage() {
  // getMyTrackedTrains() itself returns null on a 401 (see the api.ts
  // sketch below) -- unlike TicketPanel, this page does NOT need a
  // separate getSession() call to disambiguate "not logged in" from
  // "logged in but not the owner": there is no second party here at all
  // (the route has no id in its path to be wrong about), so a 401 from
  // this one call is the complete, unambiguous signal. This is a real,
  // deliberate simplification versus TicketPanel's two-call composition,
  // not an oversight -- see Decision 1's note on why GET /Train/mine has
  // only two outcomes where the ticket routes have three.
  const trains = await getMyTrackedTrains();

  if (trains === null) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>My Tracked Trains</Title>
        <TextLink href="/api/auth/login" underline="always">
          Log in to see the trains you're tracking
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
```

`TrackedTrainListRow` (new small presentational component, or inlined —
implementation-time detail) links each row to whichever URL is actually
correct for that pin's state, matching the existing by-id page's own
"canonical link once resolved" logic rather than always sending the user
through the by-id redirect hop:

- `resolutionStatus === 'resolved' && trainUid` → `/train/{trainUid}/{serviceDate}`
  (the canonical, shareable URL).
- otherwise (`pending`/`unresolved`, or `resolved` with a `trainUid` that's
  somehow still null — defensive) → `/train/by-id/{id}`.

Row content: origin → destination (or origin alone if no destination was
pinned), the pinned scheduled departure (now available — Finding 4) and
service date, and a compact status indicator — `resolutionStatus` badge
for `pending`/`unresolved`, or the journey `status` + a delay badge
(reusing the same "Xm late"/"On time" `Badge` treatment
`TrainJourney.tsx`'s `JourneyDetails` already uses) once `resolved`. Full
movement detail (last reported location, next calling point, ETA) is
deliberately not duplicated here — that's what clicking through to the
detail page is for, per Decision 1's shape rationale.

**`getSession().catch(...)` is not needed on this page at all** — only the
nav item (Decision 4) needs it, to decide whether to render the nav link
in the first place. This page's own defensive posture is entirely carried
by `getMyTrackedTrains()`'s null-on-401 return, mirroring
`getPreferences()`/`getTicketsForTrackedTrain()`'s existing shape, not a
new pattern.

### 4. Nav integration: a session-gated nav item, hidden entirely for a logged-out visitor

**Decision: add a new nav link, "My Tracked Trains," visible only when the
visitor has an active session** — not always-visible-but-login-gated (the
way `TicketPanel` degrades a *section of an already-public page*), because
this is a full nav-bar entry point to a page whose entire content is
private to the viewer; showing it to every visitor and having it always
resolve to a login nudge would be dead weight in the nav bar for the
(likely common) case of an anonymous visitor, the same reasoning
`AuthStatus` already applies by swapping its own rendering entirely on
`session.authenticated` rather than always showing a name/logout control
skeleton.

Implementation follows `app/layout.tsx`'s existing `AuthNavItem`/
`DataFreshnessNavItem` pattern exactly — a new async Server Component,
its own `<Suspense>` boundary, its own guarded `getSession()` call:

```tsx
async function TrackedTrainsNavItem() {
  // Same defensive fallback as AuthNavItem/DataFreshnessNavItem: a root
  // layout has no route-level error.tsx, so an uncaught rejection here
  // would take down every page's nav bar, not just this link. This is the
  // guarded shape -- the historical bug this spec's brief calls out (an
  // unguarded getSession() that could take down a whole page) was in
  // TicketPanel.tsx, already fixed there; this new call follows the
  // already-correct AuthNavItem precedent from the start.
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

...wrapped in `<Suspense fallback={null}>` (no skeleton needed — unlike
`DataFreshnessNavItem`'s icon or `AuthNavItem`'s "Log in" text, there's no
harmless placeholder to show for a link whose very presence depends on the
fetch that's still pending; better to render nothing until it resolves
than to flash a link that might immediately disappear), placed in the nav
`Group` next to the existing `<TextLink href="/track">Track a Train</TextLink>`.

This duplicates the `getSession()` call `AuthNavItem` (immediately below
it in the same `Group`) already makes on every page load — accepted as
harmless: Next.js's per-request `fetch` deduplication means two calls to
the same URL with the same options within one render pass share a single
underlying network request, so this is not a second round-trip to the
API, just a second `await` on the same in-flight response. No new caching
mechanism needs to be built for this.

### 5. Data refresh: reuse `AutoRefresh`, same as every other dynamic page

`getMyTrackedTrains()` is an ordinary `no-store` Server Component read,
refreshed automatically the same way every other dynamic section of this
app already is via the global `AutoRefresh` (mounted once in
`app/layout.tsx`, `router.refresh()` every 30s, no per-route opt-out
mechanism today). Same accepted tension both prior specs already noted for
their own pages (a page showing only finished/cancelled trains keeps
re-fetching every 30s even though nothing on it can change) — not
re-litigated here, this page inherits the same existing, already-accepted
posture.

## API/type contract

Hand-written, matching this repo's existing convention of not generating
types from the Rust source (`frontend/lib/types.ts`'s own established
pattern):

```ts
// frontend/lib/types.ts additions

/** `GET /Train/mine`'s per-item response shape
 * (`crates/api/src/data/train_tracking.rs`'s `TrackedTrainListItem`,
 * camelCase). A deliberately lighter shape than `TrackedTrainState` --
 * see the backend spec's Decision 1 for exactly what's included/excluded
 * and why. `pinScheduledDeparture` is new: neither `TrackedTrainState` nor
 * any other existing route exposes it. */
export interface TrackedTrainListItem {
  id: number;
  serviceDate: string;              // "YYYY-MM-DD"
  pinOriginCrs: string;
  pinDestinationCrs: string | null;
  pinScheduledDeparture: string;    // RFC3339
  resolutionStatus: ResolutionStatus;
  trainUid: string | null;
  status: JourneyStatus | null;
  delayMinutes: number | null;
  trackedAt: string;                // RFC3339 -- list ordering key
}
```

(`ResolutionStatus`/`JourneyStatus` are already defined in
`frontend/lib/types.ts`, reused verbatim — no new enum types needed.)

```ts
// frontend/lib/api.ts addition -- per-user, session-gated read, same
// cookie-forwarding pattern getSession()/getPreferences()/
// getTicketsForTrackedTrain() already use.

/** `GET /Train/mine`. Returns `null` on `401` (not logged in) --
 * deliberately not `ApiNotFoundError`, matching `getTicketsForTrackedTrain`'s
 * precedent of treating "no session" as an expected outcome, not a
 * failure. Unlike that function, there is no second, distinct 404-shaped
 * outcome to also collapse into `null` here -- see Decision 3's note on
 * why this page doesn't need a separate getSession() call the way
 * TicketPanel does. */
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

No proxy (`app/api/[...path]/route.ts`) changes needed: `GET /Train/mine`
is a server-side-only read (like every other `lib/api.ts` function), never
called from a Client Component, so it never goes through the browser-facing
proxy at all — same split `getTrackedTrainById`/`getTicketsForTrackedTrain`
already establish (server reads direct to `API_BASE_URL`, only
browser-initiated *mutations* go through `/api/*`). This route also needs
no allowlist widening on the proxy side even if that were relevant, since
it's a `GET` under the already-covered `/Train/...` root-mounted prefix.

## Architecture

```
┌────────────────────────────────────────────────────────────────────────┐
│ frontend/ (Next.js App Router)                                          │
│                                                                            │
│  app/track/mine/page.tsx        NEW -- async Server Comp, cookie-fwd     │
│                                    GET /Train/mine, login nudge / empty   │
│                                    state / list, per-row canonical link  │
│                                                                            │
│  app/layout.tsx                  + TrackedTrainsNavItem (NEW, own        │
│                                    Suspense, own guarded getSession())    │
│                                                                            │
│  lib/api.ts    + getMyTrackedTrains                                      │
│  lib/types.ts  + TrackedTrainListItem                                    │
└──────────────────────────┬────────────────────────────────────────────────┘
     server-side fetch     │
     (read, cookie-fwd,    │
     no-store)             ▼
                 ┌──────────────────────────────────────────────┐
                 │ api crate                                       │
                 │  GET /Train/mine   NEW -- AuthenticatedUser-gated│
                 │    -> train_tracking::list_tracked_trains_for_user│
                 │       (NEW query + TrackedTrainListItem, NEW)    │
                 └──────────────────────────────────────────────┘
```

## Error handling

- `getMyTrackedTrains()`'s `401` branch is not an error path — it's an
  expected, common, first-class outcome (an anonymous visitor, or one
  whose session lapsed since page load), rendered as the login nudge, same
  posture as `getTicketsForTrackedTrain`'s own `401`/`404` branches.
- Any other non-ok status (5xx, network failure) throws via the shared
  `errorForResponse`, falling through to the existing root `app/error.tsx`
  — no segment-specific `error.tsx` for this route, matching every other
  page with no bespoke error boundary today.
- `TrackedTrainsNavItem`'s `getSession()` call is guarded with `.catch()`,
  degrading to "link hidden" on any auth-check glitch rather than taking
  down the nav bar for every visitor — see Decision 4.
- No new upload/mutation surface is introduced by this spec at all — it is
  a pure read/list feature, so none of the multipart/file-upload error
  handling from the ticket-tracking spec is relevant here.

## Testing

Following this repo's existing convention (colocated `*.test.tsx`,
`renderWithMantine`, Vitest; Rust `#[cfg(test)]` modules colocated in the
same file per `train_tracking.rs`'s and `train.rs`'s existing precedent):

- `crates/api/src/data/train_tracking.rs`: this repo's existing convention
  for this module is to unit-test pure logic (`validate_pin`,
  `validate_ticket_entry`) without a database, and leave query functions
  themselves covered by integration tests elsewhere (none of the existing
  query functions in this file — `create_pin`, `list_active_tracked_trains`,
  `get_by_tracking_id`, etc. — have unit tests of their own in this file).
  `list_tracked_trains_for_user` follows that same precedent; no new unit
  test is expected here beyond what integration/e2e coverage (if any
  exists elsewhere in this repo for query functions) already provides.
- `crates/api/src/routes/train.rs`: extend the existing `#[cfg(test)]`
  module's `router_builds_without_panicking` coverage implicitly (adding
  the route and re-running that test is the actual regression check for
  Finding 5's literal-vs-dynamic-segment concern) — no new logic-bearing
  test needed for `get_my_tracked_trains` itself, since (unlike
  `build_delay_repay_response`) it contains no pure logic to unit-test in
  isolation, only a direct pass-through to the query function.
- `lib/api.ts`: unit test for `getMyTrackedTrains` returning `null` on
  `401` vs. resolving normally on `200`, mirroring
  `getTicketsForTrackedTrain`'s existing test shape.
- `app/track/mine/page.tsx`: render tests for the three real outcomes —
  `null` (login nudge), `[]` (empty state with a working link to
  `/track`), and a populated list (each row rendered, each linking to the
  correct URL variant depending on `resolutionStatus`/`trainUid`).
- `app/layout.tsx`'s `TrackedTrainsNavItem`: render test confirming the
  link is absent when `session.authenticated` is `false` and present,
  pointing at `/track/mine`, when `true` — mirroring `AuthStatus.test.tsx`'s
  existing shape for session-conditional nav rendering.

## Explicitly out of scope for this spec

- **A "my tickets across all tracked trains" view.** Different backend
  gap (`tracked_train_tickets` has no `list_tickets_for_user`-shaped query
  either — only `list_tickets_for_tracked_train`, scoped by
  `tracking_id`), already flagged as out of scope by
  `2026-08-29-journey-ticket-tracking-frontend-design.md`'s own
  "Explicitly out of scope" section. This spec is trains-only; tickets stay
  reachable only via each train's own detail page's `TicketPanel`.
- **Any retention/pruning job for `tracked_trains`.** Per Finding 3, none
  exists today, and this spec doesn't add one — the `MINE_LIST_LIMIT` cap
  (Decision 2) bounds one response, it does not delete or archive
  anything. If unbounded growth ever becomes a real operational problem,
  that's a separate, backend-storage-focused piece of work, not something
  a list-page frontend spec should decide unilaterally.
- **Pagination / "load more" / "view older trains" past the cap.** Not
  designed here — see Open questions. A user with more than
  `MINE_LIST_LIMIT` tracked trains ever simply can't reach the older ones
  through this page as specified.
- **Filtering/search/sort controls on the list** (e.g. "show only
  unresolved," "show only today"). Per Decision 2, the backend can't
  honestly support an "active" filter at all today, and no other filter
  axis was requested or found to be load-bearing for this pass — a plain,
  single-order list is the whole of v1.
- **Fixing the underlying `'completed'`-status gap** in
  `crates/trust-consumer/src/journey.rs` (Finding 1). This spec designs
  around that gap (by not attempting a filter the data can't support
  honestly); it doesn't fix it. `2026-08-29-train-tracking-frontend-design.md`'s
  own Open Question 2 already flags this as backend work belonging to
  `trust-consumer`, not a frontend concern.
- **Real-time updates faster than the existing global 30s `AutoRefresh`.**
  No new refresh mechanism designed here, per Decision 5.

## Open questions / risks

1. **What `MINE_LIST_LIMIT` should actually be.** `100` is proposed here as
   a reasonable-sounding round number, not a researched or load-tested
   figure — this codebase has no real-world data yet on how many trains a
   typical (or a power) user tracks over their account's lifetime, and per
   Finding 3, nothing bounds that number from the database side. If usage
   patterns turn out to make 100 too low (a user genuinely wants to browse
   further back) or unnecessarily high (most users track a handful, ever),
   this number should be revisited once real usage exists — the same
   posture `MAX_PIN_AGE`/the backend design doc's 90-day retention figure
   already took ("a starting proposal, not a researched one").
2. **No pagination/"load more" is designed for what falls past the cap.**
   A user who has tracked more than `MINE_LIST_LIMIT` trains simply loses
   access to the oldest ones via this page (their individual detail pages,
   if the URL/tracking id is still known, remain reachable — this only
   affects discoverability through the list). Whether that's an acceptable
   permanent limitation or needs real pagination is left open; not
   resolved here since it depends heavily on Open Question 1's answer.
3. **Whether `delayMinutes`/`status` on each row is enough "at a glance"
   signal, or whether a full `EtaBadge`-style ETA per row is also wanted,**
   is a real UI judgment call this spec makes narrowly (delay + status
   only, full detail on click-through) but doesn't claim is definitively
   right — flagged as a reasonable place to revisit once the page exists
   and gets real use, the same way
   `2026-08-29-journey-ticket-tracking-frontend-design.md`'s own Open
   Question 1 flagged its eager-fetch-per-ticket choice as "likely fine in
   practice, worth revisiting only if real usage shows otherwise."
4. **This spec assumes `TrackedTrainsNavItem`'s extra `getSession()` call
   is genuinely free via Next.js fetch deduplication** (Decision 4) rather
   than measuring it — this is standard, well-documented Next.js App
   Router behavior (request memoization for `fetch` within one render
   pass), not a novel claim, but it's asserted here rather than benchmarked
   against this specific app's setup; worth a quick sanity check at
   implementation time if nav-bar latency ever becomes a concern.
