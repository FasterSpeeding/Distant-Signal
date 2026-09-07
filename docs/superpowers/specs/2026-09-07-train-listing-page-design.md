# Design: A Filterable Train-Listing Page

**Status: design proposal, not approved.** Research and architecture only —
no migration, no Rust code, no frontend code in this pass. This document is
input to a human decision, not authorization to implement anything.

Product request, verbatim: *"Just a way to see a list of trains and how
they're doing and what they're scheduled for based on user input filters.
So you can click through to specific trains and track 'em directly or see
how your friend's train is doing from them sharing the link with you
etc."* — plus an explicit ask to consider whether this new page can
**replace** `/track` (`frontend/app/track/page.tsx` +
`frontend/components/TrackTrainForm.tsx`), not just sit beside it.

Required reading consumed in full before this document was written:
`frontend/components/TrackTrainForm.tsx`; `frontend/app/track/page.tsx`;
`frontend/app/train/[uid]/[date]/page.tsx`; `frontend/components/PinToggle.tsx`;
`frontend/components/useNeedsLogin.ts`; `crates/api/src/routes/train.rs`
(module doc and `post_track_by_uid`/`get_by_uid_and_date`/
`post_attach_ticket`/`enrich_shared_train`); `crates/api/src/data/trains.rs`;
`crates/api/src/routes/departures.rs`; `crates/api/src/data/queries.rs`
(the `schedule_network_departures` section); `crates/api/src/routes/reference.rs`;
`crates/api/src/routes/lines.rs` (the `/lines/{id}/schedule` route);
`crates/schedule-query/src/resolve.rs` (`departures_by_crs`,
`schedules_touching`); `crates/schedule-reference/src/main.rs`;
`docs/superpowers/specs/2026-09-03-trip-search-design.md`;
`docs/superpowers/specs/2026-09-04-whole-network-trip-search-research.md`;
`docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md`;
`docs/superpowers/specs/2026-09-06-shared-train-identity-design.md`;
`docs/superpowers/specs/2026-09-07-shared-train-status-write-race-design.md`
(for tone/structure precedent).

## 0. A load-bearing correction to the brief's own framing

The brief's required-reading list describes the 2026-09-04 whole-network
document as something that "deliberately ruled out" a cross-network
filterable list and asks this document to argue why those constraints do
or don't still apply. Reading the actual code (not just the design doc)
shows something more specific: **the whole-network CIF-derived fallback
described in that document is not a proposal any more — it already
shipped, unchanged from its own design.** Confirmed directly:

- `crates/schedule-reference/src/main.rs:273` — `const
  MAX_DEPARTURES_PER_STATION: usize = 10`, used at `main.rs:325`
  (`departures.truncate(MAX_DEPARTURES_PER_STATION)`) inside
  `publish_schedule_network_departures` (`main.rs:277`).
- `crates/api/migrations/20260904110000_schedule_network_departures.sql`
  and `crates/api/src/data/queries.rs:843` (`upsert_schedule_network_departures`),
  `queries.rs:888` (`latest_schedule_network_departures`, keyed
  `(crs, service_date)`).
- `crates/api/src/routes/departures.rs:61-89`
  (`get_station_schedule_departures`, `GET
  /public/stations/{crs}/schedule-departures`) — live, tested (`db_tests`
  in the same file), and already wired into `TrackTrainForm.tsx`'s 404
  fallback (`TrackTrainForm.tsx:207-232`).
- `crates/schedule-query/src/resolve.rs:174-211` — `departures_by_crs`,
  the grouping-pass function the design doc sketched, is real and tested
  (`resolve.rs:392-565`).

So the constraints this document has to engage with are not "should we
ever build the whole-network CIF fallback" (already answered: yes, it's
shipped) but the **narrower, still-genuinely-unresolved** ones the shipped
version explicitly declined to build, which are the actual candidates for
what this new listing feature would need to add on top:

1. No resident whole-network index anywhere (`ScheduleIndex` stays
   stack-local per `schedule-reference` cycle,
   `2026-09-04-whole-network-trip-search-design.md`'s "Explicitly out of
   scope").
2. No pagination, no window wider than 10 departures, `now`-forward, per
   station (`departures_by_crs`'s own `now`-forward filter,
   `resolve.rs:190-192`, plus the 10-row cap).
3. No synchronous HTTP call from `api` into `schedule-reference` (or any
   other batch poller) at request time.
4. No broadening of `poller-ldbws`'s ~286-station sampled set.
5. **Grouped by origin CRS only, not by destination, operator, or a
   named line** — `departures_by_crs`'s bucket key is the departure's own
   CRS (`resolve.rs:196-206`), with no equivalent grouping by destination
   or operator anywhere in `schedule-query` or the published
   `schedule_network_departures` table. `ScheduleDeparture`
   (`crates/schedule-query/src/records.rs`, referenced from
   `resolve.rs:180`) carries only `{uid, scheduled, destination_crs}` — no
   operator field exists on it at all (confirmed by the whole-network
   research doc's Part 1, and independently reconfirmed by `records.rs`
   having no `operator`/`BX`/headcode decoding anywhere).

Point 5 is the one the shipped feature never needed to solve (a
per-*origin-station* picker doesn't need to filter by destination server-side
— `TrackTrainForm.tsx`'s own `matchesDestination`/`matchesOperator`
client-side filters, `TrackTrainForm.tsx:18-43`, already handle that for a
single station's ≤10-row picker) and is the one a genuinely
destination-first or filter-first whole-network *list* would need. This is
the crux this document works through in the Approaches section.

## 1. Current state, grounded in code

### 1.1 Train discovery UI today: one component, two data sources, one station at a time

`TrackTrainForm` (`frontend/components/TrackTrainForm.tsx`) is the only
train-discovery UI in this app. Concretely, per its own extensive doc
comments and confirmed by reading the component in full:

- **Single origin station only.** The user types/picks one origin CRS
  (`Autocomplete`, `TrackTrainForm.tsx:543-557`, backed by
  `searchStations` against the `stations` table, ~2,500 rows,
  `docs/superpowers/specs/2026-07-11-operator-station-autocomplete-design.md:20`).
  There is no "search by destination" or "search by operator" entry point
  — Destination and Operator are optional **filters on that one station's
  own departure list**, not independent search axes (`matchesDestination`/
  `matchesOperator`, `TrackTrainForm.tsx:18-43`).
- **Two mutually-exclusive data sources, LDBWS-primary, CIF-fallback.**
  `GET /api/stations/{crs}/departures` (LDBWS live board,
  `crates/api/src/routes/departures.rs:31-56`, ~286 stations
  `poller-ldbws` samples) is tried first; only on a `404` does it fall back
  to `GET /api/stations/{crs}/schedule-departures` (CIF-derived,
  `departures.rs:61-89`, whole ~2,500-station network via `stanox_crs`)
  — `TrackTrainForm.tsx:207-232`. The two sources are never merged for one
  station; LDBWS wins outright whenever it has data at all.
- **Capped at ~10 rows, always "now-forward," always today.** Both sources
  independently cap at 10 (`poller-ldbws`'s `num_rows`,
  `crates/poller-ldbws/src/config.rs:46`; `MAX_DEPARTURES_PER_STATION`,
  `crates/schedule-reference/src/main.rs:273`) and both are "whatever's on
  the board/schedule right now," never a browsable future date or a wider
  window (`get_station_schedule_departures`'s own "always today,
  server-side" posture, `departures.rs:70-77`).
- **Selecting a row fills, never submits, a tracking form.**
  `pickDeparture`/`pickCifDeparture` (`TrackTrainForm.tsx:262-282`) set
  Destination/Operator/Scheduled-departure state; the user still has to
  press "Track this train" (`handleSubmit`,
  `TrackTrainForm.tsx:284-354`), which `POST`s a `TrackPinRequest` to
  `/Train/track` (legacy CRS+time pin creation,
  `crates/api/src/routes/train.rs` module doc lines ~20-38 and
  `post_track` — the "schedule-first" path this whole design predates).
- **The ticket-attach path is real and load-bearing.** When invoked with
  `attachTicketId` (from `TicketEntryForm`'s standalone-ticket flow,
  `TrackTrainForm.tsx:119-138`), a successful `POST /Train/track` triggers
  a best-effort follow-up `POST /Train/tickets/{attachTicketId}/attach`
  (`TrackTrainForm.tsx:319-335`) before navigating to the new pin. A
  failure there is swallowed — the tracked train still gets created, the
  ticket just stays standalone and reattachable later — but the *call
  itself* is not optional plumbing to drop; it is how a user who uploaded
  a ticket before knowing which train it was for ends up with the two
  linked in one flow.
- **401 handling protects typed input.** Unlike `PinToggle`'s "nothing was
  typed, safe to forget the click" posture, a 401 from `/Train/track`
  leaves all four fields exactly as typed and renders `LoginPromptModal`
  alongside them (`TrackTrainForm.tsx:339-342`, its own doc comment
  `:144-148`, Decision 4 of
  `docs/superpowers/specs/2026-09-04-track-a-train-picker-refactor-design.md`).
- **CIF rows link out to the public train page, LDBWS rows don't.** The
  CIF branch's "View live status" link
  (`TrackTrainForm.tsx:525-531`) goes to `/train/{uid}/{today}` because a
  CIF row genuinely carries a `uid` (a real CIF schedule UID); the LDBWS
  branch has nothing to link to that page with — `DepartureRow` carries a
  Darwin `serviceId`, "a different identifier scheme entirely," per the
  component's own comment (`TrackTrainForm.tsx:80-82`).

### 1.2 `/train/[uid]/[date]`: the existing public, shareable per-train page — read-only, no track CTA

`frontend/app/train/[uid]/[date]/page.tsx` renders one real train's shared
public status by `(train_uid, service_date)`, via
`getPublicTrainByUidAndDate` → `crates/api/src/routes/train.rs`'s
`get_by_uid_and_date`, which reads only the shared `trains`/
`train_current_state` tables — **no `AuthenticatedUser` extractor, no
ownership check** (its own doc comment, `train.rs` ~line 630 area: "anyone
can look up any known train... nothing in this response shape that COULD
leak another user's private per-subscription data"). It has a `ShareButton`
(`page.tsx:5,103`) and a closing sentence pointing back at `/track` to
"get updates" (`page.tsx:106-109`), but **renders no owner actions at
all** — no Rename/Delete/tickets, and critically for this document, **no
"Track this train" button**, even though `POST
/Train/by-uid/{train_uid}/{date}/track` already exists server-side and is
doc-commented as "the NR-primary tracking entry point"
(`train.rs`, `post_track_by_uid`'s own doc comment, ~line 673-687). This
route currently has zero frontend callers — confirmed by grepping the
frontend tree for `by-uid` and finding only the read-only `GET
/Train/by-uid/{uid}/{date}` call in `lib/api.ts`, never the `/track`
suffix.

`post_track_by_uid` (`train.rs`, `post_track_by_uid`): authenticated via
the normal `AuthenticatedUser` session extractor (same model as legacy
`post_track`, not the internal-OAuth service-token pattern); takes only
`train_uid` + `date` from the path, no request body; calls
`find_or_create_train` (`crates/api/src/data/trains.rs:17-31`, an
idempotent `ON CONFLICT ... DO UPDATE ... RETURNING id` upsert) to resolve
or create the shared `trains` row, then
`train_tracking::create_subscription_for_train` to link a new
`train_subscriptions` row to it — **no `pending`/`schedule_matched`
waypoint at all**, since identity is already known upfront. It then
best-effort-enriches the row via `enrich_shared_train`
(`train.rs`, same file) — a backlog replay attempt for schedule/history
data, logged-not-propagated on failure. It returns `{ trackingId }`
exactly like legacy `post_track`.

**`post_track_by_uid` has no ticket-attach equivalent today.** Nothing in
its handler or in the frontend calls `POST /Train/tickets/{id}/attach`
after it succeeds — a real, concrete parity gap this document has to
address directly (§4).

### 1.3 What backend query capability already exists vs. what would be new

Already exists and directly reusable, unchanged:

- **Per-origin-station departures**, both LDBWS-live (`GET
  /public/stations/{crs}/departures`) and CIF-derived-whole-network (`GET
  /public/stations/{crs}/schedule-departures`) — exactly what
  `TrackTrainForm` already uses.
- **Station/operator text autocomplete** (`GET /public/stations?q=`, `GET
  /public/tocs?q=`, `crates/api/src/routes/reference.rs:20-51`) — small
  reference tables (~2,500 stations, ~30 TOCs), already used for every
  free-text field in this app.
- **Per-train public status by identity**, `GET
  /Train/by-uid/{uid}/{date}` — the page this new list's rows would link
  to.
- **Per-line full stopping pattern for a rail day**, `GET
  /public/lines/{id}/schedule?date=` (`crates/api/src/routes/lines.rs:90-150`),
  reading `schedule_line_population` directly — but keyed by a catalogued
  `line_id` (~110 `lines/*.toml` entries, a real but partial subset of the
  network — the whole-network research doc's own count, "110 files under
  `lines/` collectively name only 267 distinct TIPLOCs"), **not** filterable
  by origin/destination/operator/time, and not something a "list of trains
  matching X" UI can drive directly (it answers "what does line L look
  like today," not "which trains match these filters").

Would be genuinely new backend query surface, not a reuse of anything
above:

- **Any query answering "list of trains from A to B" or "trains matching
  operator X, departing between T1 and T2" across the whole network.**
  Nothing today groups CIF-derived departures by destination or operator —
  `departures_by_crs` groups by origin CRS only (§0, point 5). Building
  this would mean either a second grouping pass in `schedule-query`
  (bucketing by `(origin_crs, destination_crs)` or similar) or a
  request-time filter over an already-published per-origin-station row —
  see Approaches, below.
- **Any query answering "list of trains matching filters, live-board
  primary, whole-network."** LDBWS/`station_samples` is fundamentally
  single-station-keyed (`crates/api/src/data/queries.rs`'s
  `latest_station_sample`, one row per CRS) and only covers ~286 stations.
  There is no live-board index by destination or operator anywhere, and
  building one would mean scanning all ~286 `station_samples` rows per
  request (small, but real, new server-side work with no existing
  precedent) or a new resident cache — see Approaches.
- **Operator filtering against CIF-derived data at all.** The CIF SCHEDULE
  feed's operator field (`BX`) is parsed-but-undecoded everywhere in this
  codebase (`crates/schedule-query`'s own module doc: "an optional `BX`
  line extends it... but not decoded"), confirmed independently by the
  whole-network research doc's Part 1. A listing page cannot filter
  CIF-derived rows by operator without new byte-offset-verified decoding
  work this codebase has twice now looked at and deferred (both trip-search
  design docs' "Explicitly out of scope").

## 2. Goal and scope

**Goal:** one page, `/trains` (name chosen for symmetry with `/track`; not
load-bearing), where a user enters filters — Origin, Destination
(optional), Date (default today), Time-of-day range (default "now
onward"), Operator (optional, LDBWS-sourced results only — see below) —
and sees a scrollable result list, each row showing:

- Scheduled departure time, origin → destination, operator (when known)
- A live-status summary where available (delay/cancelled badge, sourced
  exactly like `TrackTrainForm`'s LDBWS branch — never fabricated for
  CIF-only rows, per the honesty convention both trip-search docs already
  established)
- A link to `/train/{uid}/{date}` for any row that carries a real CIF
  `uid` (LDBWS-only rows, lacking a `uid`, get no such link — same
  limitation `TrackTrainForm`'s own LDBWS branch already accepts,
  `TrackTrainForm.tsx:80-82`)
- A "Track this train" action, gated behind the same anonymous-login-prompt
  pattern `PinToggle`/`TrackTrainForm` use (`useNeedsLogin.ts`,
  `LoginPromptModal`)

**What filters are realistic, not aspirational**, given §1.3's inventory:

| Filter | Realistic today? | Why |
|---|---|---|
| Origin station | Yes | Exactly what `TrackTrainForm` already resolves — required, same as today. |
| Date | Yes, narrowly | Both existing sources are "today only, server-side" (§1.1); a future/past date needs `schedule_line_population`'s own future/past-dates work (named, not solved, in this doc's required-reading sibling docs) — **out of scope for v1**, see §6. |
| Time-of-day range | Partially | The existing `now`-forward + 10-row cap already gives a de facto narrow window; a wider or arbitrary range needs the cap lifted — a real, scoped backend change (§3). |
| Destination | **New backend work required** | No existing grouping by destination anywhere (§1.3). Realistic as a v1 filter only if Approach B or C (below) is taken. |
| Operator | **Partial** — LDBWS rows only | CIF rows have no operator field at all (§1.3). An "Operator" filter that silently returns nothing for CIF-derived rows is the same honest-gap posture `TrackTrainForm` already uses for Operator on its CIF branch (`matchesOperator` is simply never called there, `TrackTrainForm.tsx:457-459`). |
| Line/route | Possible, narrower | `GET /public/lines/{id}/schedule` exists but requires the user to already know a `line_id` — not itself a listing UI, but a narrower Approach C could build on it. |

Result-row shape reuses `DepartureRow`/`ScheduleDepartureRow`'s existing
wire shapes (`TrackTrainForm.tsx:83-108`) wherever possible rather than
inventing a third one — see Approaches for exactly which shape each
option would need.

## 3. Approaches

### Approach A — Client-side, origin-still-required: turn the existing per-station picker into a general-purpose listing page, no new backend query surface

Reuse `GET /public/stations/{crs}/departures` and `GET
/public/stations/{crs}/schedule-departures` exactly as they exist today.
The new `/trains` page becomes, structurally, `TrackTrainForm`'s existing
picker lifted out of the tracking form and given its own page/route,
still requiring an Origin station up front, with Destination/Operator/Time
staying **client-side filters over that one station's ≤10-row result**
(reusing `matchesDestination`/`matchesOperator`/`matchesScheduledDeparture`
verbatim, `TrackTrainForm.tsx:18-70`).

**Tradeoffs.**
- Zero new backend work. Ships fast, reuses fully-tested, already-shipped
  code paths verbatim.
- **Does not actually satisfy the product ask.** "See a list of trains...
  based on filters" reads naturally as "trains from anywhere matching my
  filters," and the product owner's own phrasing ("see how your friend's
  train is doing from them sharing the link") implies discovering an
  *arbitrary* train, not one from a station the searcher happens to already
  know. Requiring Origin up front is exactly `TrackTrainForm`'s existing
  constraint, restyled — not a new capability.
- Destination-first search (a very plausible real use: "what's the next
  train to Edinburgh, from anywhere") is not answerable at all under this
  approach, since nothing groups departures by destination.

### Approach B — Destination/operator-first whole-network search: a new `schedule-query` grouping pass, published like `schedule_network_departures`'s own precedent

Add a second grouping function to `crates/schedule-query/src/resolve.rs`,
sibling to `departures_by_crs`, that buckets the same per-cycle
`ScheduleIndex` resolve pass by `(destination_crs)` (or `(origin_crs,
destination_crs)` pairs) instead of by origin CRS alone. Publish it from
`schedule-reference` on the same 30-minute cycle, into a new table —
copy-adjacent to `schedule_network_departures`'s own migration and
POST/GET route shape (`crates/api/migrations/20260904110000_schedule_network_departures.sql`,
`crates/api/src/routes/ingest.rs`'s `post_schedule_network_departures`,
`crates/api/src/routes/departures.rs`'s `get_station_schedule_departures`)
— then a new public route, `GET /public/trains/search?...`, that reads
this table server-side and applies the caller's Origin/Destination/
Time-range filters directly in SQL/JSONB, rather than shipping every row
to the client to filter, the way `TrackTrainForm`'s client-side filters do
today for a single station's ≤10 rows.

This is **not** a violation of the constraints named in §0: it reuses the
*same* already-shipped, transient, per-cycle `ScheduleIndex` build (no
second parse, no resident index — `departures_by_crs` and this new
sibling function would both run against the one `ScheduleIndex` already
built once per `schedule-reference` cycle, exactly as
`publish_cif_derived_products`'s sketch in the 2026-09-04 design doc
described "one pass over the feed producing two outputs"); it is a batch,
published-then-read pattern, not a synchronous cross-service call; and it
does not touch `poller-ldbws`'s sampled-station set at all. What it *does*
require, honestly:

- **A real, new grouping function and a real, new publish/read path** —
  bounded in the same way `schedule_network_departures` itself was bounded
  (a `now`-forward filter, a per-bucket row cap), but genuinely new code,
  not a reuse of anything that exists today.
- **A decision on the result cap's shape.** `departures_by_crs` caps *per
  origin station* at 10; a destination-keyed (or pair-keyed) bucket has a
  different, unmeasured cardinality profile — a popular destination like
  London Euston could have far more than 10 matching UIDs network-wide in
  a `now`-forward window. This needs its own sizing pass, not an assumed
  reuse of the existing `MAX_DEPARTURES_PER_STATION = 10` constant.
- **Operator stays unavailable for CIF-derived rows**, unchanged from
  today (§1.3) — this approach adds a new grouping key, not new decoded
  fields.
- **LDBWS coverage is not extended by this approach at all.** The
  ~286-station live board remains origin-keyed only
  (`station_samples`); a destination-first *live* search (as opposed to
  CIF-derived scheduled search) would need its own new work — scanning
  all `station_samples` rows and filtering by destination server-side, a
  smaller, bounded addition (at most ~286 rows to scan per request) but
  still new query surface with no existing precedent to point to.

**Tradeoffs.** This is the approach that actually delivers what the
product owner described — a real, filterable, whole-network list, not a
per-station picker with a new page wrapper. It is bounded, scoped work
building directly on a proven, already-shipped pattern (not new
architecture this codebase has "deliberately avoided" — §0 already showed
the risky, expensive part, whole-network CIF parsing at national scale, is
a sunk cost). But it is not zero-cost: a new grouping function, a new
table, two new routes, and an unmeasured cardinality/cap question that
needs a real sizing pass before shipping (mirroring the honest,
unresolved-cap posture the original `schedule_network_departures` design
itself flagged in its own Open Questions, item 5).

### Approach C — Staged: line-scoped filtering first, whole-network destination search deferred

Ship a narrower first slice: filtering *within* a line a user has already
picked (reusing `GET /public/lines/{id}/schedule?date=` directly,
`crates/api/src/routes/lines.rs:90-150`), rather than a true whole-network
destination-first search. A user picks a catalogued line (~110 of them,
`lines/*.toml`), then filters its already-published `LinePopulationEntry`
list by destination/time-of-day client-side (same posture as Approach A's
client-side filtering, just over a line's population instead of a
station's departures).

**Tradeoffs.** Smallest possible addition — literally zero new backend
routes, since `/public/lines/{id}/schedule` already exists and already
returns every UID's full calling pattern for a rail day
(`records.rs:147-160`'s `LinePopulationEntry`). But it inherits the
line catalogue's own real gaps: only ~110 lines, collectively naming only
267 distinct TIPLOCs per the whole-network research doc's own count — a
small, partial subset of the ~2,500-station `stanox_crs`/`stations`
coverage the CIF fallback already achieves per-origin-station. A user
has to already know which "line" (a concept this app defines for its own
internal catalogue purposes, not a concept most riders think in) their
journey belongs to before they can search at all — a materially worse
discovery UX than "type a destination," and arguably a step backward from
what `TrackTrainForm`'s CIF fallback already offers today (whole-network
coverage, origin-first).

### Recommendation

**Approach B**, not A or C. Approach A doesn't deliver the feature the
product owner actually asked for — it's a reskin of what exists, not a
new capability. Approach C trades away the one thing the already-shipped
CIF fallback got right (genuine whole-network coverage) for a smaller
build, for no real corresponding benefit — the line catalogue's ~267-TIPLOC
coverage is a strict regression from what `stanox_crs` already provides.
Approach B is bounded, scoped, and builds directly on a pattern
(`schedule-reference`'s transient per-cycle `ScheduleIndex` → grouped
publish → `api` passthrough) this codebase has now used twice
successfully (`schedule_line_population`, `schedule_network_departures`)
— a third application of the same shape, with its own new sizing question
flagged honestly rather than assumed away. Scope Approach B's v1 to
Destination-first search only (Origin remains what it is today —
optional, not required, unlike `TrackTrainForm`'s Origin-required form);
defer Operator filtering entirely (already unavailable for CIF rows, and
adding it for LDBWS-only rows in a mixed-source list needs its own
UX design for how a filter behaves when only some rows can honor it) and
defer live-board (LDBWS) destination-search to a later pass once
CIF-derived destination search has shipped and its cap/cardinality
questions are answered with real data — mirroring this codebase's own
repeated "ship the honest partial thing, revisit with real usage" pattern
(the LDBWS-primary/CIF-fallback split itself, the 53/286
per-station-stats split cited in the whole-network research doc, etc).

## 4. The `/track` replacement question

**Direct answer: no, the new listing page cannot fully replace
`TrackTrainForm`'s current job, and `/track`'s form-based role should stay
— but its relative importance shifts, and the ticket-attach flow needs a
deliberate migration if `POST /Train/by-uid/{uid}/{date}/track` is meant
to become the primary route through which tracking happens.**

Breaking `TrackTrainForm`'s three jobs apart, per the brief's own framing:

1. **Schedule discovery** (browsing departures to find the train you
   mean) — **yes, the new listing page can fully replace this job**, and
   arguably does it better (whole-network destination search vs.
   origin-only browsing). This part of `/track` becomes redundant once
   `/trains` ships with Approach B.
2. **Creating a tracked subscription** — **only partially.** For a row
   that carries a real CIF `uid` (Approach B's CIF-derived rows always
   will; LDBWS-derived rows, lacking a `uid`, cannot use the NR-primary
   path at all — same limitation `TrackTrainForm`'s own LDBWS branch
   already accepts, §1.1), the listing page's "Track this train" button
   can call `POST /Train/by-uid/{uid}/{date}/track` directly — no form,
   no manual entry, strictly better UX than today's "pick a row, then
   separately press Track." But `/Train/track`'s legacy CRS+time flow
   (`post_track`) is not made obsolete by this: it is precisely the
   fallback for **any train the listing/picker genuinely can't identify**
   — a station this feature's data sources don't cover, a departure that
   scrolled off a capped 10-row window, a user who knows a train is
   running but it isn't showing up anywhere yet. `/track`'s manual-entry
   fields (`TrackTrainForm.tsx:541-620`) are the honest fallback for
   exactly the gaps this document's own §1.3 catalogues (no future/past
   date, capped windows, no operator on CIF rows, LDBWS-only ~286
   stations) — removing the manual path would remove the one thing that
   currently absorbs every one of those gaps.
3. **The ticket-attach flow** — **cannot be dropped, and is not
   automatically carried by the NR-primary path today.** §1.2 already
   established `post_track_by_uid` has no equivalent to
   `post_attach_ticket`'s follow-up call. If the listing page's "Track
   this train" button is meant to be a real substitute for
   `/track?ticketId=...`'s flow (reached from `TicketEntryForm`'s "find or
   track the train this ticket is for" link,
   `frontend/app/track/page.tsx:14-20`), **the button needs the exact same
   `attachTicketId`-aware follow-up `TrackTrainForm.tsx:319-335` already
   performs**: on a successful `POST
   /Train/by-uid/{uid}/{date}/track`, best-effort-call `POST
   /Train/tickets/{attachTicketId}/attach` with the returned
   `trackingId`, swallowing a failure exactly as today (the ticket stays
   standalone and reattachable rather than blocking navigation). This is a
   **frontend-only** parity fix — `post_attach_ticket`
   (`train.rs:246-276`) already takes a bare `trackingId` in its body and
   has no opinion about which endpoint produced that id, so no backend
   change to `post_track_by_uid` or `post_attach_ticket` is required for
   parity, only a frontend call site that does the same two-step sequence
   `TrackTrainForm`'s `handleSubmit` already does.

**Recommendation on replacement: do not delete or hide `/track`.** Instead:

- Ship `/trains` as the **primary discovery surface**, replacing
  `TrackTrainForm`'s picker as the thing most users reach for first
  (whole-network, destination-first, richer than an origin-only picker).
- Keep `TrackTrainForm`/`/track` as the **explicit fallback**, reachable
  from `/trains` itself (e.g. "Can't find your train? Track it manually")
  exactly the way it's already reachable from `/stations/[crs]`'s "Track a
  train from here" link (`TrackTrainForm.tsx:126-127`'s own doc comment)
  and from `TicketEntryForm`'s standalone-ticket flow — both of which
  **must keep working unchanged**, since neither is this document's to
  redesign.
- Give the listing page's "Track this train" action full ticket-attach
  parity (above) so a user arriving at `/trains` via a
  `?ticketId=...`-style deep link (mirroring `/track`'s own
  `ticketId` query param, `app/track/page.tsx:7,19-20`) gets the same
  outcome as today's `/track` flow, not a degraded one.

This is not "it's possible to replace `/track`, technically" — it's a
direct "no" on full replacement, for one concrete, load-bearing reason
(manual entry is the honest fallback for real, enumerated data gaps this
feature does not close) plus one fixable gap (ticket-attach parity, a
frontend-only addition, not a backend blocker).

## 5. The `/train/[uid]/[date]` "Track this train" CTA

**In scope for this design, not deferred**, because the listing page's own
"Track this train" action (§4, point 2) is, functionally, the exact same
CTA this page has been missing. Recommendation: add it directly.

- A "Track this train" button, visible to every visitor (same "show the
  control to everyone, prompt on the real 401" posture `PinToggle`/
  `TrackTrainForm` already establish via `useNeedsLogin`/`LoginPromptModal`,
  `useNeedsLogin.ts:5-8`'s own doc comment naming this exact reusable
  pattern), calling `POST /Train/by-uid/{uid}/{date}/track` with no body
  (the route takes only the path params, `train.rs`'s `post_track_by_uid`
  signature — `Path((train_uid, date))`, no `Json<...>` extractor at all).
- On success (`{ trackingId }`), navigate to `/train/by-id/{trackingId}`
  — the same destination `TrackTrainForm.tsx:336` already navigates to
  after a successful legacy pin, for a consistent "you're now tracking
  this" landing experience regardless of which path got the user there.
- On 401, call `needsLoginState.markNeedsLogin()` and render
  `LoginPromptModal`, unchanged pattern.
- **No ticket-attach parameter needed on this specific page** — `/train/
  [uid]/[date]` has no existing `ticketId` query-param convention the way
  `/track` does, and inventing one here is out of scope for this design
  (a future pass can add it if a "find this train from a ticket, land on
  its public page, then track" flow is wanted — not asked for in this
  brief).
- **A logged-in visitor who already tracks this train** gets no special
  treatment in this design — `get_by_uid_and_date`'s response shape
  (`PublicTrainState`, `crates/api/src/data/trains.rs:220-251`) carries no
  "you already have a subscription" hint, by design (the shared-train-identity
  spec's own §4 calls this out as "optional, nice-to-have, not required"
  for the read side). Clicking "Track this train" a second time is safe
  regardless — `create_subscription_for_train` is presumably idempotent
  per-user per-train in the same way `find_or_create_train` is idempotent
  per-identity, but this document does not verify that claim against
  `train_tracking.rs`'s exact implementation, and flags it as an open
  question (§7) rather than asserting it.

## 6. Explicitly out of scope

- **Any change to `/Train/track`, `TrackPinRequest`, or `post_track`'s own
  matching logic.** This document only adds a new discovery surface and a
  new CTA; the legacy fallback path is untouched.
- **Backend decoding of `BX`/operator or headcode for CIF-derived data.**
  Already deferred twice by this codebase's own prior specs
  (§1.3); this document does not re-open that call.
- **Browsing a date other than today**, for either LDBWS or CIF-derived
  sources. Both remain "always now/today, server-side" — a future/past
  date filter on the new listing page needs `schedule_line_population`'s
  own future/past-dates work (named in this repo's
  `2026-09-06-schedule-line-population-{future,past}-dates-design.md`
  sibling documents), not designed here.
- **A resident, permanently-in-memory whole-network index anywhere.**
  Approach B's grouping pass stays transient and per-cycle, exactly like
  `departures_by_crs` today — no change to that posture.
- **A synchronous request-time call from `api` into `schedule-reference`
  or any other batch poller.** Approach B is publish-then-poll, same as
  every existing cross-service link in this app.
- **Broadening `poller-ldbws`'s ~286-station sampled set.** Unrelated,
  already-declined-elsewhere scope (per the LDBWS design doc's own
  Decision 1, restated by the whole-network design doc) — untouched here.
- **A true whole-network live-board (LDBWS) destination search.** Approach
  B's recommended v1 scope is CIF-derived only; extending destination
  search to the live ~286-station board is named as later work (§3), not
  designed here.
- **Merging LDBWS and CIF-derived rows for the same train/station.**
  Neither existing source does this today (`TrackTrainForm`'s own
  fallback-only posture, never blended) and this design doesn't introduce
  merging either — a listing-page result row is honestly tagged by its
  source, same as today's picker.
- **A `ticketId` deep-link convention for `/train/[uid]/[date]`.** Named
  in §5 as a possible future addition, not designed or built here.
- **Redesigning `notifier`'s fan-out for a train with many subscribers**,
  or anything else already named as a separate, unresolved dependency in
  `docs/superpowers/specs/2026-09-06-shared-train-identity-design.md`'s
  own §3/§7. This document assumes that foundation as given.
- **The exact visual/UX layout of the `/trains` page** (table vs. card
  list, column choices, mobile layout). This is an architecture document,
  not a UI mock — left to implementation/design-review time.

## 7. Open questions / risks

1. **Approach B's per-bucket cap and cardinality are unmeasured.**
   `departures_by_crs`'s existing `MAX_DEPARTURES_PER_STATION = 10` was
   chosen by precedent-matching against `poller-ldbws`'s own default, not
   by measuring destination-keyed cardinality
   (`2026-09-04-whole-network-trip-search-design.md`'s own Open Question
   5 already flags this for the *origin*-keyed case; a destination-keyed
   or pair-keyed bucket is a materially different, wholly unmeasured
   distribution — a popular terminus could have far more matching UIDs in
   a `now`-forward window than a typical origin station does). This needs
   a real check against `timetable_full.zip` before an implementation plan
   commits to a specific cap.
2. **Is `train_tracking::create_subscription_for_train` idempotent for a
   second click by the same user on the same train?** Referenced in §5 as
   an assumption this document does not verify. If it is not — if a
   second click creates a second `train_subscriptions` row for the same
   user+train — the CTA in §5 needs either a client-side "already
   tracking" state (which the current `PublicTrainState` response can't
   supply, per §5) or a backend uniqueness constraint this document has
   not confirmed exists.
3. **What does a "mixed-source" result row look like when a station
   appears in both the LDBWS live board and (once Approach B ships) the
   CIF-derived destination index for the same train?** This document
   recommends never merging (§6), but doesn't design the UI treatment for
   a user filtering by destination who might see what looks like the
   "same" train twice from two sources with different freshness — a real,
   deferred UX question, not resolved here.
4. **Whether Approach B's CIF-derived destination-search table should be
   retained/pruned the same way `schedule_network_departures` presumably
   is** (this document did not verify `schedule_network_departures`'s own
   retention/pruning story, if any, before proposing a sibling table —
   worth confirming during implementation planning rather than assuming
   parity).
5. **Whether `/trains` should accept a shareable filter state in its own
   URL** (e.g. `?destination=EDB&date=2026-09-08`), mirroring `/track`'s
   `?origin=`/`?ticketId=` query-param convention — a natural, low-risk
   addition given the product owner's own "share the link with a friend"
   framing, but not designed in detail here since it depends on Approach
   B's exact filter set being finalized first.
6. **Same-day VSTP-style urgent CIF amendments** — inherited, still
   unresolved, from both prior trip-search documents' own Open Questions.
   Directly relevant here too: a destination-search result list is
   exactly the kind of surface where a same-day schedule change (if this
   pipeline even carries them, which remains unconfirmed) would need to be
   reflected promptly to avoid showing a stale, already-changed schedule.
