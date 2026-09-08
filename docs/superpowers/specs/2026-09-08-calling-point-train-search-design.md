# Design: Calling-Point Train Search (generalizing destination-first search)

**Status: design proposal, approved for implementation by the requesting
session (no separate human sign-off step in this pipeline).** This document
turns the just-shipped destination-first `/trains` search
(`docs/superpowers/specs/2026-09-07-train-listing-page-design.md`, as revised
by `docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md`)
into a calling-point-first search: primary key becomes "any station the
train calls at," with "departing from" and "terminating at" demoted to
optional filters layered on top.

Product request, verbatim intent: the current `/trains` search requires a
destination CRS and treats origin as an optional filter on top. Search
should instead find trains by **any station they call at along their
route** (any calling point — departing-from and terminating-at included),
with origin and destination becoming **optional filters layered on top of
that calling-point lookup**. Example: *"find trains that call at READING
today, optionally further filtered to ones originating at X and/or
terminating at Y."*

Required reading consumed in full before this document was written:
`crates/schedule-query/src/resolve.rs` (all of it, including every test);
`crates/schedule-query/src/records.rs`; `crates/api/migrations/20260907130000_schedule_destination_departures.sql`;
`crates/api/src/data/queries.rs` (the whole `schedule_destination_departures`
section, `:900-1189`); `crates/api/src/routes/trains.rs` (whole file,
including `db_tests`); `crates/api/src/render.rs` (`:130-211`);
`crates/api/src/routes/ingest.rs` (the `schedule-destination-departures`
route and its `db_tests`); `crates/schedule-reference/src/main.rs`
(`:200-471`); `frontend/components/TrainSearchForm.tsx`;
`frontend/app/trains/page.tsx`; the two predecessor design docs named above,
for tone/structure precedent.

## 0. Settling the data-model question — this is the load-bearing part of
this document

Two prior, secondhand characterizations of `schedule_destination_departures`
disagreed about its granularity. Reading the real code settles it
definitively, **and then reveals a second, narrower ambiguity the prompt's
own "picture 2" framing did not fully resolve**, which is the actual reason
this document needed a real design pass rather than a one-line "add an
index" patch.

### 0.1 The table is already calling-point-granular (confirmed "picture 2")

`crates/schedule-query/src/resolve.rs:260-305`,
`departures_by_destination_crs`, does NOT emit one row per (train,
true-origin, true-destination). It emits **one row per departure-bearing
calling point** of every non-cancelled resolved schedule:

```rust
for cp in &resolved.calling_points {
    let Some(departure) = cp.booked_departure else { continue; };
    if departure < now { continue; }
    let Some(origin_crs) = tiploc_to_crs.get(normalize_tiploc(&cp.tiploc)) else { continue; };
    by_destination.entry(destination_crs.clone()).or_default().push(DestinationDeparture {
        uid: resolved.uid.clone(),
        origin_crs: origin_crs.clone(),   // <- cp's OWN crs, not the schedule's first station
        scheduled: departure,
    });
}
```

`destination_crs` is computed **once per schedule**, via
`resolved.calling_points.last()` (`resolve.rs:276-282`), and is identical for
every row that schedule contributes. `origin_crs` is computed **once per
calling point** inside the loop, and is genuinely different across a
schedule's own rows.

This is independently confirmed three more ways, all already in the
codebase before this document:

- `crates/schedule-query/src/records.rs:190-209`, `DestinationDeparture`'s
  own doc comment, states plainly: *"`origin_crs` means 'the station this
  train departs FROM', which is the calling point's own CRS... It is NOT
  necessarily the schedule's own first station, and is deliberately not the
  same concept as `trains.origin_crs` in `crates/api`, which always is."*
- The migration's own header
  (`crates/api/migrations/20260907130000_schedule_destination_departures.sql:1-2`):
  *"ONE ROW PER DEPARTURE, not one row per destination bucket."*
- `resolve.rs`'s own test,
  `departures_by_destination_crs_buckets_every_departure_bearing_calling_point_under_one_destination`
  (`resolve.rs:692-726`): a train EUSTON→CREWE(intermediate)→MNCRPIC(terminate)
  produces **two** rows in the *same* MAN bucket — `origin_crs: "EUS"` and
  `origin_crs: "CRE"` — proving one row per calling point, not one per train.

This matches the live production query quoted in the brief exactly: 9 rows
for `train_uid='L78659'`, all `destination_crs=WAT`, 9 distinct `origin_crs`
values and 9 distinct `scheduled` times — one row per calling point along the
route to Waterloo.

**Conclusion: picture 2 is correct.** No new table is needed, and the
grouping/resolve logic in `schedule-query` needs no structural change — it
already produces calling-point-level rows.

### 0.2 The refinement the "just add an index" framing misses

The brief's suggested minimal fix was: add an index on
`(service_date, origin_crs, scheduled, train_uid)`, and a new query function
that filters on `origin_crs` (renamed conceptually to "calling point")
instead of `destination_crs`, using the existing `destination_crs` column as
the optional destination filter. That correctly identifies `origin_crs` as
already being calling-point-granular — exactly what the new required
"station" search key needs. **But it does not account for what happens to
the "origin" filter once that repurposing happens.**

Once `origin_crs` becomes the primary equality filter (`station = S`), every
row of a response has `origin_crs = S` by construction. There is no room
left in that column for a *second*, independent meaning — "trains
originating at X" cannot also be answered by comparing `origin_crs`, because
`origin_crs` is now pinned to whatever the caller searched for. The brief's
own example is explicit that X is generally a **different** station than the
one being searched ("calling at READING... originating at X") — i.e. it is
asking about the schedule's *true, first* calling point, not the row's own
calling point.

This is not a new problem the codebase doesn't already have a name for: the
existing `records.rs:197-204` doc comment already draws exactly this
distinction — `origin_crs` ("departs from here") vs. `trains.origin_crs`
("the schedule's actual first station", `crates/api`'s notion, always true).
`schedule_destination_departures` currently stores only the former, per row.
The latter — the schedule's one, true, first calling point — is not stored
anywhere on this table today. `destination_crs` gets this right (computed
once via `.last()`, stored as a real column, already a legitimate filter);
there is no `.first()`-equivalent column.

**Consequence: this needs one new nullable column**, not just an index.
Call it `true_origin_crs` — the schedule's real origin CRS, computed once
per schedule via `resolved.calling_points.first()` (the exact mirror of how
`destination_crs` is already computed via `.last()`), stored identically on
every row that schedule contributes, and used **only** as the optional
"originating at" filter, never as the primary search key (which is
`origin_crs`, unchanged in meaning, retained as the "which calling point"
column).

This is a small addition — one nullable `TEXT` column, one small resolve.rs
change — not a new table, not a new grouping function, not a change to the
publish cadence or the "no cap, no resident index" constraints the
predecessor docs establish. It is a genuine, real requirement the "just add
an index" framing did not surface, found by tracing what the "origin"
filter would have to mean once "calling point" takes over the primary-key
role it used to fill.

### 0.3 Nullability policy for the new column, and why it differs from `destination_crs`

`departures_by_destination_crs` already has a precedent for "TIPLOC doesn't
resolve to a CRS" in two different flavors, and they are deliberately
different (`resolve.rs:236-243`):

- `destination_crs` is the **bucket key** in `departures_by_destination_crs`
  — if it can't be resolved, the whole schedule is dropped (there is "no
  honest bucket to file it under").
- A calling point's own CRS, when it's just a **field** (as in
  `departures_by_crs`'s `destination_crs: Option<String>`), degrades to
  `None` rather than dropping the row.

`true_origin_crs` is a **filter field**, not a key, in exactly the same
sense `departures_by_crs`'s own `destination_crs` field is — so it follows
the *softer* precedent: if the schedule's first calling point's TIPLOC
doesn't resolve, the row is still emitted, with `true_origin_crs: None`. An
"originating at X" filter then simply never matches that row (correct: we
cannot honestly confirm or deny it), rather than silently losing the row
from every other search that doesn't ask about origin at all.

## 1. Goal and scope

Change `GET /public/trains/search` (and its one caller, `TrainSearchForm`)
so the required search key is **any calling point** (a station the train
calls at, boarding or alighting, anywhere on its route), with "departing
from" (the schedule's true origin) and "terminating at" (the schedule's true
destination, unchanged from today) as independent, optional filters on top.

**Explicitly not changed:** the "no operator filter," "today only, no date
param," "no resident whole-network index," "publish once per CIF delivery,
`now`-forward applied at read time," "keyset pagination, not offset," and
"404 means no publish for today, `200 []` means published-but-unmatched"
decisions from the two predecessor design docs. None of those are
reopened by generalizing the search key.

## 2. Schema change

One new migration,
`crates/api/migrations/20260908120000_schedule_destination_departures_calling_point_search.sql`:

```sql
-- Generalizes the destination-first search
-- (2026-09-07-train-listing-destination-search-sizing-design.md) into a
-- calling-point-first one
-- (2026-09-08-calling-point-train-search-design.md). See that document's
-- §0 for why this is an additive column + index, not a new table: the
-- table is already one-row-per-departure-bearing-calling-point (proven in
-- §0.1); only the QUERY's leading equality column changes, from
-- `destination_crs` to `origin_crs` (already exactly "the calling point of
-- this row" -- see `schedule_query::DestinationDeparture`'s own doc
-- comment), plus one new column to carry the schedule's TRUE origin
-- independently of which calling point a row represents (§0.2).

ALTER TABLE schedule_destination_departures ADD COLUMN true_origin_crs TEXT;

-- New leading index for the new primary query shape: equality on
-- (service_date, origin_crs) -- "calls at this station" -- then a range
-- scan on scheduled, with train_uid as the keyset cursor's tiebreaker.
-- Does NOT replace the existing primary key, which remains required for
-- upsert idempotency (ON CONFLICT DO NOTHING targets it) and still serves
-- destination_crs as a real, if no-longer-leading, filter column.
CREATE INDEX schedule_destination_departures_calling_point_idx
    ON schedule_destination_departures (service_date, origin_crs, scheduled, train_uid);
```

No change to the primary key, no change to retention/pruning
(`crates/aggregator/src/queries.rs::prune_schedule_destination_departures`,
`:544`, is `service_date`-scoped and needs no change), no change to publish
cadence.

## 3. `schedule-query` crate changes

`crates/schedule-query/src/records.rs`, `DestinationDeparture`: add

```rust
pub true_origin_crs: Option<String>,
```

with a doc comment update explaining the distinction from `origin_crs`
(existing field, "the calling point this row represents" — unchanged) per
§0.2/§0.3 above.

`crates/schedule-query/src/resolve.rs`, `departures_by_destination_crs`:
compute `true_origin_crs` once per schedule, the same way `destination_crs`
is already computed once per schedule —

```rust
let true_origin_crs = resolved
    .calling_points
    .first()
    .and_then(|first| tiploc_to_crs.get(normalize_tiploc(&first.tiploc)))
    .cloned();
```

— and attach it, unchanged, to every `DestinationDeparture` the schedule
contributes (mirroring exactly how `destination_crs` is already shared
across all of a schedule's entries). New/updated tests:

- A schedule with 3+ calling points: every emitted row (origin's own,
  every intermediate's) carries the SAME `true_origin_crs`, which for the
  origin's own row happens to equal that row's `origin_crs`, and for every
  other row does not — the discriminating case that proves `true_origin_crs`
  is schedule-scoped, not calling-point-scoped.
- An unresolved first-calling-point TIPLOC: the schedule's rows are still
  emitted (contrast with the existing
  `departures_by_destination_crs_drops_a_schedule_whose_destination_tiploc_is_unresolved`
  test, which drops the WHOLE schedule for the analogous destination case),
  each with `true_origin_crs: None`.
- Existing seven tests for this function keep passing with the new field
  simply present and asserted where relevant; no existing assertion needs to
  change in a way that alters its meaning.

`departures_by_crs` (the origin-keyed sibling backing
`schedule_network_departures`) is untouched.

## 4. `crates/api` data layer (`crates/api/src/data/queries.rs`)

- `ScheduleDestinationDeparturesRow`: add `pub true_origin_crs: Option<String>`.
- `upsert_schedule_destination_departures`: bind a sixth parallel `Vec` for
  `true_origin_crs` in the `UNNEST` insert; column list grows to six.
- New cursor type, replacing `DestinationDepartureCursor`:

  ```rust
  pub struct CallingPointDepartureCursor {
      pub scheduled: chrono::NaiveTime,
      pub train_uid: String,
  }
  ```

  Two components, not three: the old cursor's third component
  (`origin_crs`) existed only because `origin_crs` used to vary within one
  response (it was the field being paginated over per destination). Under
  the new query shape `origin_crs` is the fixed equality filter — constant
  across every row of one response — so it carries no ordering information
  and would be redundant in the cursor. `train_uid` alone is a sufficient
  tiebreaker on `scheduled` because `train_uid` is unique per
  `(service_date, origin_crs)` (a schedule visits a given CRS-mapped calling
  point at most once — see the "same station twice" edge case noted in §7).

- New page type `CallingPointDeparturePage { departures: Vec<Value>, next_cursor: Option<CallingPointDepartureCursor> }`.
- New function replacing `search_schedule_destination_departures`:

  ```rust
  pub async fn search_schedule_calling_point_departures(
      pool: &PgPool,
      station_crs: &str,
      service_date: chrono::NaiveDate,
      scheduled_from: chrono::NaiveTime,
      true_origin_crs: Option<&str>,
      destination_crs: Option<&str>,
      to_time: Option<chrono::NaiveTime>,
      after: Option<&CallingPointDepartureCursor>,
      limit: i64,
  ) -> Result<Option<CallingPointDeparturePage>>
  ```

  ```sql
  SELECT train_uid, destination_crs, true_origin_crs, scheduled
  FROM schedule_destination_departures
  WHERE service_date = $1
    AND origin_crs = $2
    AND scheduled >= $3
    AND ($4::text IS NULL OR true_origin_crs = $4)
    AND ($5::text IS NULL OR destination_crs = $5)
    AND ($6::time IS NULL OR scheduled <= $6)
    AND ($7::time IS NULL OR (scheduled, train_uid) > ($7, $8))
  ORDER BY scheduled, train_uid
  LIMIT $9
  ```

  Rides the new `schedule_destination_departures_calling_point_idx` for
  equality + range + cursor, exactly the way the old query rode the primary
  key — same "LIMIT + 1 index entries touched" cost profile regardless of
  how busy the searched station is.

  The day-scoped existence probe
  (`schedule_destination_departures_published_for`) is reused unchanged —
  it was already day-scoped, not destination-scoped, so nothing about it
  needs to know about the new leading column.

- The old `search_schedule_destination_departures`,
  `DestinationDepartureCursor`, and `DestinationDeparturePage` are removed,
  not kept alongside the new ones — `GET /public/trains/search` is being
  changed in place (per the brief), and this route has exactly one caller
  in this repository (`TrainSearchForm.tsx`, changed in lockstep by this
  same plan), so there is no external consumer to preserve backward
  compatibility for. Their existing tests are replaced by equivalents
  exercising the new function (see §8 for the specific fixture shape that
  discriminates `origin_crs` from `true_origin_crs`).

## 5. `crates/api/src/render.rs`

Replace `destination_departure_json` with `calling_point_departure_json`:

```rust
pub(crate) fn calling_point_departure_json(d: &Value, station_crs: &str) -> Value {
    let scheduled = d.get("scheduled").and_then(Value::as_str).map(|s| s.chars().take(5).collect::<String>());
    json!({
        "uid": d.get("uid").cloned().unwrap_or(Value::Null),
        "scheduled": scheduled,
        "stationCrs": station_crs,
        "originCrs": d.get("true_origin_crs").cloned().unwrap_or(Value::Null),
        "destinationCrs": d.get("destination_crs").cloned().unwrap_or(Value::Null),
    })
}
```

Two differences from the old function, both deliberate and both breaking
(§6 explains why that's acceptable here):

- `stationCrs` is new — the caller-supplied, normalized required search
  parameter, echoed onto every row exactly the way `destinationCrs` used to
  be (it's constant for the whole response, so it's attached client-side by
  the render function rather than re-selected from every row).
- `originCrs` now means the schedule's TRUE origin (nullable — `None` when
  unresolved, per §0.3), not "the calling point of this row" — that role
  moves to `stationCrs`. `destinationCrs` keeps its name and its meaning
  (the schedule's true destination) but is no longer caller-supplied and
  fixed; it now varies per row and is read out of `d`, same shape switch
  `schedule_departure_json` already uses for its own destination field.

## 6. `crates/api/src/routes/trains.rs`

- `TrainSearchParams`: `station: String` (required, was `destination`),
  `origin: Option<String>` (now means "originating at" — a breaking
  semantic change, see below), `destination: Option<String>` (new,
  optional — "terminating at," unchanged meaning from today, just no
  longer required), `from`/`to`/`limit`/`after` unchanged.
- `normalize_crs("station", ...)` required exactly where `destination` used
  to be validated; `origin`/`destination` both become the same
  optional-CRS-when-present pattern the OLD `origin` param already used.
- `encode_cursor`/`decode_cursor`: two parts (`"HH:MM:SS|train_uid"`), not
  three — matches `CallingPointDepartureCursor`'s new shape.
- Calls `queries::search_schedule_calling_point_departures(...)`, and
  renders with `calling_point_departure_json(row, &station)`.
- **This is a breaking change to `GET /public/trains/search`'s query
  contract**: `destination` goes from required to optional, a new required
  `station` param is added, and `origin`'s meaning changes from "the
  calling point a train departs from" to "the schedule's true origin."
  This is deliberate and is not a compatibility concern: the route has
  exactly one consumer in this codebase (`TrainSearchForm.tsx`), updated by
  this same plan, and there is no versioned or external API contract to
  preserve — same posture the sizing addendum already took when it changed
  this route's response envelope additively (its own §5, Task 7 row).
- Module doc comment, `MAX_SEARCH_LIMIT`/`DEFAULT_SEARCH_LIMIT` doc
  comments referencing "destination-first": reworded to "calling-point"
  framing where they describe the search semantics; the actual behavior
  those comments defend (page size ceiling reasoning, clamp-vs-400 posture,
  `now`-forward boundary ownership) is unchanged and stays as-is.
- `db_tests`: `seed_today` gains a `true_origin_crs` column in its insert
  and its signature; a properly discriminating fixture needs at least one
  schedule where `origin_crs` (the calling point/search key) and
  `true_origin_crs` (the origin filter) are DIFFERENT values, to prove the
  two are no longer conflated. Every existing test that used `?destination=`
  as the required param moves to `?station=`; every existing test that used
  `?origin=` to mean "departs from this calling point" is retargeted to
  `?origin=` meaning "true schedule origin," against the new fixture.

## 7. `crates/schedule-reference`

`schedule_destination_departures_rows` (`main.rs:376-397`): add
`"true_origin_crs": d.true_origin_crs` to the emitted JSON object per row.
Its own doc comment's per-entry byte budget note ("~80 bytes, not ~55")
grows slightly (~10 more bytes/entry for a `TEXT`-or-null field); at
~377,000 rows that's roughly +3.5MB on top of the already-budgeted ~30MB,
still comfortably inside `DefaultBodyLimit::max(100 * 1024 * 1024)`
(`crates/api/src/routes/mod.rs:86`) — not large enough to reopen the
chunking question the sizing addendum already resolved.

`publish_schedule_destination_departures`'s `now = NaiveTime::MIN` and every
other constraint in that function (whole day, uncapped, publish-then-poll,
same per-cycle `ScheduleIndex`) is unchanged — this document adds a field
to an existing row, not a new publish path.

`crates/api/src/routes/ingest.rs::post_schedule_destination_departures`:
unchanged route/method/auth; its body type follows `ScheduleDestinationDeparturesRow`'s
new field automatically. Its `db_tests` fixture bodies gain the new
optional field (can be omitted from a JSON fixture and `serde` will need
the field to be `Option`-typed and thus tolerant of absence, OR every test
fixture is updated to include it explicitly — pick the latter for
consistency with this codebase's preference for explicit fixtures over
relying on `#[serde(default)]`, unless a task finds a reason to prefer the
former).

## 8. Frontend (`frontend/components/TrainSearchForm.tsx`, `frontend/app/trains/page.tsx`)

- New required field: **Station** (label copy TBD at implementation time,
  e.g. "Station" with description "Any station this train calls at,
  including where it starts or ends" — matches §0's "any calling point"
  framing). New `stationCrs` state, its own `Autocomplete` backed by
  `searchStations`, same CRS-format validation as the existing fields.
- Existing **Destination** field is relabeled "Terminating at (optional)"
  and becomes optional (unchanged CRS validation, now gated like `origin`
  already is today).
- Existing **Origin** field is relabeled "Departing from (optional)" and
  its underlying semantics change to match §0.2/§6 — it now filters to the
  schedule's true origin, not "any calling point along the route," which is
  a real behavior change worth a copy update (e.g. its description changes
  from "Any station on the train's route, not just where it started" to
  something like "Where the journey actually begins").
- `canSearch`: `stationValid && originValid && destinationValid && fromValid && toValid && !searching`.
- `searchParams()`: `station` always set (required); `origin`/`destination`
  set only when non-empty, same pattern as today's optional fields.
- `TrainSearchRow` wire type: `{ uid: string; scheduled: string; stationCrs: string; originCrs: string | null; destinationCrs: string | null }`
  — both `originCrs` and `destinationCrs` become nullable on the wire
  (§0.3's `None` policy for origin; destination stays non-null in practice
  since it's the bucket key from `departures_by_destination_crs` and is
  never `None` there, but the type should still reflect it can be filtered
  optionally without being guaranteed present in the same way `originCrs`
  now explicitly can be absent — a task should double check whether making
  it non-nullable in the TS type but keeping the Rust `Option` unwrap
  pattern in render.rs, which is already `unwrap_or(Value::Null)`
  defensively, is the more honest choice; lean toward matching whatever
  render.rs actually emits rather than asserting more than the backend
  guarantees).
- Row rendering: needs to show the station being searched plus whichever of
  origin/destination are known, e.g.
  `{scheduled} · calls at {stationCrs} · {originCrs ?? '?'} → {destinationCrs ?? '?'}`
  — exact copy is an implementation-time call, not fixed here (matches the
  predecessor doc's own "visual treatment is implementation/design-review"
  posture).
- Error copy at `resultsContent()`'s `!stationValid` branch (was
  `!destinationValid`): reworded, e.g. *"Enter a station above to search for
  trains that call there."*
- `frontend/app/trains/page.tsx`: `searchParams` type gains `station?: string | string[]`;
  destructure and pass `initialStation` to `TrainSearchForm` the same way
  `initialDestination`/`initialOrigin` already work. Existing
  `?destination=`/`?origin=` prefill behavior is kept (now prefilling the
  now-optional filters), and `?station=` is added as the new primary
  shareable-link param. Descriptive copy ("Search the whole network by
  where a train is going...") updated to reflect calling-point-first
  framing.
- `TrainSearchForm.test.tsx`: every existing test exercising "destination
  required" moves to "station required"; a new discriminating test proves
  origin/destination are independent optional filters layered on a fixed
  station, not the primary key.
- `TrackThisTrainButton`, the `/train/{uid}/{date}` link, and the
  ticket-attach flow (`attachTicketId`) are all untouched — this document
  changes only the search/filter surface, not the row-to-detail-page or
  row-to-tracking wiring, both already correct per the brief's explicit
  scope boundary.

## 9. Explicitly out of scope

- **A pure-terminus-only calling point is not found by a `station=` search
  for the specific service where it's only ever a terminus.** The table has
  no row for a `Terminate` calling point (it has no `booked_departure`,
  `resolve.rs:284-286`'s `let Some(departure) = cp.booked_departure else { continue; }`
  drops it before any row is built), so a schedule where CRS `Y` is the
  final stop only shows up under `station=Y` via some OTHER schedule that
  also calls there with a departure (e.g. a return working) — never via its
  own terminus leg. This existed identically before this change (the OLD
  `destination=Y` search already only worked because `destination_crs` is
  a real, separately-computed column, not because Y ever got a departure
  row of its own) and is not made worse by it; it is named here because the
  NEW `station=` search surfaces the gap in a case the old
  `destination=`-required search didn't need to care about. Fixing it
  would require storing arrival times, which this table does not have and
  this document does not add — a real, honest, un-fixed gap, matching this
  codebase's existing "drop/degrade, never fabricate" convention rather
  than papering over it.
- **Any change to operator filtering, date filtering, LDBWS/live-board
  search, or the "no resident index / no synchronous cross-service call /
  no broadened `poller-ldbws`" constraints.** All untouched, all still
  binding, per both predecessor docs' own out-of-scope sections.
- **Any change to `crates/api/src/routes/train.rs`'s `get_by_uid_and_date`,
  `is_known_scheduled_train`, `post_track_by_uid`, or `find_or_create_train`**
  — the just-merged `f65c8f8` fix and the CTA wiring are untouched.
- **A `date`/`operator` field on `TrainSearchForm`** — still out of scope,
  per the form's own existing comment this document does not revisit.
- **Retention/pruning changes** — `prune_schedule_destination_departures`
  needs no change; the new column doesn't affect what gets deleted or when.
- **Renaming the `schedule_destination_departures` table.** It is no
  longer purely "destination departures" in spirit (it now backs a
  calling-point-first search with destination as one of two optional
  filters), but renaming a live table with a retained primary key and an
  established migration history is a larger, separate, unforced move; the
  table keeps its name.

## 10. Open questions / risks

1. **The pure-terminus gap (§9, first bullet)** is real and not solved
   here. If it turns out to matter in practice (a station that is
   overwhelmingly a terminus and rarely an onward-departure point), the fix
   would need arrival-time data this table doesn't carry — a separate design
   pass, not assumed away here.
2. **Whether `train_uid` alone is always a sufficient cursor tiebreaker**
   for a fixed `(service_date, origin_crs)` (§4's `CallingPointDepartureCursor`
   design). This assumes a schedule visits a given CRS-mapped calling point
   at most once per day. A real CIF schedule that loops through the same
   station twice (rare, but not provably impossible — e.g. a reversing
   service) would produce two rows with the same `train_uid` and different
   `scheduled` times at that `origin_crs`, which the cursor already handles
   correctly (they're different `scheduled` values); the only failure mode
   would be the SAME `scheduled` time twice for the same `train_uid` at the
   same calling point, which would require a real timetable anomaly this
   document has not tried to rule out. Flagged, not solved — matches the
   old cursor's own unexamined assumption (it also relied on `origin_crs`
   plus `train_uid` plus `scheduled` being unique, an equally-unverified
   claim the old code shipped with).
3. **Copy/labels for the frontend fields** are sketched, not final — left
   to implementation time, consistent with both predecessor docs' "visual
   treatment is implementation/design review" posture.
4. **Whether the JSON fixture bodies in `routes/ingest.rs`'s `db_tests`
   should omit `true_origin_crs` (relying on `Option` deserialization) or
   include it explicitly everywhere.** Named in §7, left as an
   implementation-time call.
