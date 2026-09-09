# Design: Multi-Day `GET /public/trains/search`

**Status: design proposal, approved for implementation by the requesting
session (no separate human sign-off step in this pipeline) — same posture
`2026-09-08-calling-point-train-search-design.md` and
`2026-09-08-destination-arrival-time-filter-design.md` already took for this
same route.**

Product ask, verbatim intent: `GET /public/trains/search` is
CIF-SCHEDULE-derived and meant to search over the whole published timetable,
not just "today" — but the route's own module doc comment
(`crates/api/src/routes/trains.rs:34-35`) says plainly: "There is
deliberately NO date parameter... always today, server-side." Add the
ability to search a different day, forward and backward from today.

Required reading consumed in full before this document was written:
`docs/superpowers/specs/2026-09-06-schedule-line-population-future-dates-design.md`;
`docs/superpowers/specs/2026-09-06-schedule-line-population-past-dates-design.md`;
`docs/superpowers/specs/2026-09-07-train-listing-page-design.md`;
`docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md`;
`docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md`;
`docs/superpowers/specs/2026-09-08-destination-arrival-time-filter-design.md`;
`crates/api/src/routes/trains.rs` (whole file, including `db_tests`);
`crates/api/src/data/queries.rs` (the whole `schedule_destination_departures`
section); `crates/api/src/render.rs` (`calling_point_departure_json`);
`crates/api/migrations/20260907130000_schedule_destination_departures.sql`,
`20260908120000_..._calling_point_search.sql`,
`20260908130000_..._destination_arrival.sql`; `crates/aggregator/src/queries.rs`
(`prune_schedule_destination_departures`); `crates/aggregator/src/config.rs`;
`crates/schedule-reference/src/main.rs` (`publish_cif_derived_products`
through `publish_schedule_destination_departures`); `crates/schedule-query/src/resolve.rs`
(`resolve_for_date`, `schedules_touching`, `schedule_for_uid`,
`departures_by_destination_crs` — all already date-parametric);
`frontend/components/TrainSearchForm.tsx`, `frontend/app/trains/page.tsx`;
git history `baa4e75`, `8250a9a` (the two UTC/London-skew fixes on this exact
route).

## 0. The one decision that resolves most of the rest

The brief poses six open questions. Reading the code shows that picking the
right **wire shape** (§5) collapses several of them, so that decision is
made first and the rest follow from it.

**`from`/`to`/`destination_from`/`destination_to` stay bare `"HH:MM"`
times-of-day. One new, optional query parameter, `date` (`"YYYY-MM-DD"`),
selects which single calendar `service_date` the whole search runs against
— defaulting to today when absent.** Not full ISO 8601 datetimes on every
time field; not a `from`/`to` pair that spans multiple calendar days.

Why: a CIF schedule already belongs to exactly one `service_date` — that is
what the column is called, what `resolve_for_date` takes, and what every
sibling table (`schedule_line_population`, `schedule_network_departures`)
is keyed by. `destination_from`/`destination_to` already exists as a second,
independent `"HH:MM"` pair layered on top of `from`/`to`; a single `date`
param composes with both pairs at once, where turning four separate time
fields into four independent datetimes would not — each would need its own
day, and nothing would enforce they agree. Making `from`/`to` into
datetimes would also reintroduce exactly the ambiguity `destination_from`/
`destination_to`'s own design doc was careful to avoid: "when does the train
reach `destination_crs`" already has to stay a same-day, bare-time question,
because the table has no arrival *date*, only an arrival *time* (destination-arrival
design doc's Open Question 2, an inherited, pre-existing limitation this
document does not reopen or worsen).

This single choice is what lets §4 (index shape) and part of §5 (cursor
shape) stay **unchanged** — see those sections for why.

## 1. How far forward and back, and the resulting numbers

### 1.1 The two directions have completely different cost profiles

**Backward is close to free.** `upsert_schedule_destination_departures`
already does `DELETE FROM schedule_destination_departures WHERE service_date
= ANY(...)` scoped to *only the dates in the batch being published* — it
never touches other days' rows. So every past day's rows, once published,
sit untouched until `prune_schedule_destination_departures` deletes them at
`service_date < CURRENT_DATE - retention_days` (currently `retention_days =
2`, `crates/aggregator/src/config.rs:131`). **Extending how far back a
search can reach requires zero publish-side code — only raising
`schedule_destination_departures_retention_days`'s default.** And because
every past day's data was published *on that day*, using *that day's own*
live CIF extract, this is Regime A from the past-dates sibling design
(`2026-09-06-schedule-line-population-past-dates-design.md`, Decision 3):
"exactly as accurate as present-day resolution already is — there is no new
correctness risk to document." This document deliberately never reaches for
Regime B (reconstructing a day's data after the fact from today's extract):
a date older than the retention window simply 404s, exactly like an
unpublished day does today. No STP-overlay-dropped-from-a-later-extract risk
exists anywhere in this design, because nothing is ever reconstructed after
the fact.

**Forward requires real new work.** Nothing publishes a row for any date
other than "today" today. `schedules_touching`/`resolve_for_date`/
`departures_by_destination_crs` are already fully date-parametric — confirmed
directly: `publish_schedule_destination_departures`
(`crates/schedule-reference/src/main.rs:442-475`) already takes an arbitrary
`today: chrono::NaiveDate` parameter, not a hardcoded value; its only caller,
`publish_cif_derived_products`, is the one place that currently pins it to
`chrono::Utc::now().date_naive()`. So supporting a forward window is a
**small, mechanical extension**: call the existing, unmodified
`publish_schedule_destination_departures` once per date in
`today..=today+N`, instead of once for `today` alone — the same "Approach A:
fixed N-day rolling window, recomputed in full every cycle" shape the
schedule-line-population future-dates document already worked out and
recommended for its own sibling table, and for the same reason: it is the
only shape that gets automatic pickup of a later VSTP/overlay correction for
a still-in-window future date, for free, with no separate invalidation
logic — every date in the window is unconditionally re-derived from the
latest `ScheduleIndex` every time a new delivery is processed (§3 below).

### 1.2 Chosen window: 7 days forward, 7 days back

**Forward: `N = 7`** (today, today+1, …, today+7 — 8 calendar days
published per cycle). **Backward: the search window is 7 days
(today−1, …, today−7), but `schedule_destination_departures_retention_days`
is raised from `2` to `8`, one day more than the window needs** — the same
"+1 day of safety margin around the rail-day/midnight boundary and a late
CIF delivery" reasoning this config field's own existing doc comment already
gives for why today's default is 2 rather than 1 (`crates/aggregator/src/config.rs:115-121`).
Without that margin, a date exactly 7 days back could be pruned by
`aggregator`'s own cycle moments before a request for it lands, turning a
documented-as-supported date into a flaky, timing-dependent 404. Together,
16 distinct calendar days resident at steady state (today + 7 forward + 8
backward-retained, today counted once).

Reasoning:

- This is a *browsing/search* feature, not the tracked-train pin-matching
  correctness problem the line-population future-dates document was solving
  for — that document reached for 14-30 days specifically because an
  arbitrarily-far-future *pin* can already be created today (no
  `MAX_PIN_FUTURE` exists) and needs *some* chance of resolving. Nothing
  here creates an unbounded-future obligation: a search UI's realistic
  horizon is "this week" / "next week" trip planning, not six months out,
  and CIF's own long validity windows (the cited real `2026-05-17` to
  `2026-12-06` example) do not obligate this *search* feature to expose that
  whole span — only `schedules_touching`/`resolve_for_date`'s own
  already-correct per-date resolution needs to keep working at whatever
  date this route asks for, which it already does.
- A week each way is large enough to answer the product ask ("search for
  trains on a DIFFERENT day, not just today") for the realistic case —
  tomorrow morning's commute, a trip next weekend, checking how a friend's
  journey went a few days ago — without adopting a genuinely unbounded
  window whose cost this document cannot bound.
- Symmetric, easy to state and to validate against in one place (§5).

### 1.3 The arithmetic, stated plainly

Per-day row count is the anchor number from the sizing design doc,
independently reconfirmed in the current code's own comments
(`schedule_destination_departures_rows`'s doc comment,
`crates/schedule-reference/src/main.rs:374-380`, budgets "~100 bytes per
entry... not ~55 or ~80" now that `true_origin_crs` and `destination_arrival`
have both been added since the original sizing pass):

- **Rows per service date: ~377,000** (sizing design doc §1.2 — derived from
  25,305 non-cancelled schedules × ~14.9 mean departure-bearing calling
  points per schedule for the measured day, taken as a conservative,
  above-network-average stand-in).
- **Publish payload per date: ~30MB** — already the code's own stated
  number for a single day's flat, uncapped publish
  (`crates/schedule-reference/src/main.rs:435`), comfortably under
  `DefaultBodyLimit::max(100 * 1024 * 1024)` per POST.
- **Table+index footprint per resident day: the sizing design doc's own
  estimate was "~40-60MB of table+index per service date"**, for the
  *original* 5-column, single-PK-index schema. Two nullable columns
  (`true_origin_crs TEXT`, `destination_arrival TIME`) and a second index
  (`schedule_destination_departures_calling_point_idx`, 4 columns) have been
  added since that estimate. This document does not have a fresh
  measurement of the current per-day footprint; extending that estimate by
  a second, smaller index and two mostly-small columns, a working figure of
  **~75MB/day table+index** is used below, clearly flagged as an
  extrapolation, not a re-measurement (§7 names getting a real number as a
  prerequisite task, mirroring the sizing doc's own "measure before
  committing" posture).

**Storage, forward direction (genuinely new data — nothing today publishes
these dates at all):** 7 new forward days × ~75MB/day ≈ **~525MB of new
data**, published fresh every cycle (once per CIF delivery, so this is
steady-state accrual, not a one-time cost).

**Storage, backward direction (retained longer, not new data):** today's
default (`retention_days = 2`) already keeps ~3 calendar days resident
(today, today−1, today−2) ≈ 3 × ~75MB ≈ ~225MB baseline. Raising retention
to 8 keeps ~9 calendar days (today through today−8) ≈ 9 × ~75MB ≈ ~675MB —
**~450MB of net *new* storage**, at **zero** additional publish or compute
cost (§1.1).

**Total steady-state footprint under this design: 16 days × ~75MB/day ≈
~1.2GB**, versus today's ~225MB baseline — roughly a 5x increase in this
one table's resident size. This is the number a human sign-off should see
stated plainly, per the brief's own instruction, rather than left implicit.

**Publish/compute cost:** the resolve+group+serialize+POST pass
(`departures_by_destination_crs` → `schedule_destination_departures_rows` →
`post_batch`) already runs once per CIF delivery (roughly daily, not every
30-minute poll tick — `poll_once`'s `last_processed_delivery` early return).
Under this design it runs **8 times per cycle instead of once** (today plus
7 forward dates), each against the **same**, already-built, per-cycle
`ScheduleIndex` — no second parse of the ~707MB MCA file, matching every
existing constraint this route's predecessor docs already establish. Per
the line-population future-dates document's own Dimension 2 finding
(re-applied here, not re-derived from scratch), this class of repetition —
N-times over an already-cheap, already-bounded resolve pass sharing one
index — is not expected to be a real bottleneck at N=7, but it is not
independently benchmarked in this pass either. `schedule-reference` already
emits a per-cycle duration metric; a real before/after comparison after
shipping should gate whether 7 stays the right number (named as a task in
§7, not skipped).

## 2. Publish-side extension: loop, don't reinvent

`publish_cif_derived_products` (`crates/schedule-reference/src/main.rs:176-226`)
changes from one call to `publish_schedule_destination_departures` for
`today` to a loop:

```rust
const FORWARD_WINDOW_DAYS: i64 = 7;

for offset in 0..=FORWARD_WINDOW_DAYS {
    let date = today + chrono::Duration::days(offset);
    publish_schedule_destination_departures(
        client, config, &index, date, stanox_crs_records, internal_oauth,
    )
    .await;
}
```

`publish_schedule_destination_departures` itself is **unchanged** — it
already takes an arbitrary date and already publishes the whole day
uncapped with `now = NaiveTime::MIN` (`main.rs:461`, deliberately not
`london_local_time_now()`, per that function's own doc comment). This is
the entire backend forward-publish change; nothing in `schedule_query` or
the ingest route needs to change for this part.

**Why not compute on demand for a requested future date instead?**
Considered and rejected, for the same reasons the future-dates sibling
document rejected its own "Approach B" — sharpened, if anything, by this
being a *bounded* 7-day window rather than an unbounded one:

- On-demand would require `api` to gain a dependency it does not have today
  — either linking `schedule_query` and re-reading CIF delivery files from
  the shared PVC (a real new I/O/deployment dependency), or a new
  synchronous request/response endpoint on `schedule-reference`, which has
  no inbound HTTP server at all (it only ever POSTs out, confirmed directly
  by that document and unchanged in this codebase since).
- It fails the correctness property §1.1 already established as automatic
  under the fixed-window approach: a cached/on-demand future date has no
  trigger to be refreshed when a later delivery corrects it, unless it grows
  its own invalidation machinery — at which point it has reinvented the
  fixed-window loop, just triggered differently and with more moving parts.
- At N=7, the fixed-window cost (§1.3) is small enough that there is no real
  savings on the table to justify the added architecture — this is a
  genuinely different conclusion than it might be at N=30, but 7 is a small
  enough number that "always compute it, it's cheap" wins outright.
- This route's own module doc comment already argues, for the *current*
  single-day case, "There is deliberately NO date parameter... publish then
  poll, never synchronous." Re-examined for a bounded 7-day forward window
  specifically (as the brief asks): the argument holds *more* strongly here,
  not less — the whole reason to prefer synchronous compute for a far-future
  date would be "it's rarely queried, don't waste cycles precomputing it
  unconditionally," which is exactly the tradeoff Dimension 2 of the sibling
  document already showed doesn't pay off until much larger N.

## 3. Retention/pruning: only the backward edge needs anything, and it already has a knob

**Forward: nothing new is needed, and this document says so explicitly
rather than leaving it implicit** (per the brief's own ask). A date
published as "7 days out" ages, one calendar day at a time, into "6 days
out", ..., "today", then "1 day in the past", etc. Nothing has to notice or
migrate it — it simply stops being inside the forward window's *publish*
range (so no new value is written for it beyond that point... except that,
per §1.1's correctness property, it keeps being **re-published** with fresh
data on every cycle for as long as it's still `today..=today+7`, which is
exactly the desired behavior, not a bug to guard against) and, once it's
past today, it becomes subject to the *existing* backward-pruning job like
any other historical row. No separate "forward pruning" concept exists or
is needed.

**Backward:** `prune_schedule_destination_departures`
(`crates/aggregator/src/queries.rs:544-556`) is already fully parameterized
by `retention_days` and needs **zero code changes** — only its config
default. `crates/aggregator/src/config.rs:131`'s
`schedule_destination_departures_retention_days` default changes from `2` to
`8` (§1.2's one-day safety margin beyond the 7-day search window), together
with `charts/distant-signal/values.yaml`'s
`aggregator.scheduleDestinationDeparturesRetentionDays: 2`, which the Helm
chart (`charts/distant-signal/templates/aggregator-deployment.yaml:95-98`)
passes through as a literal env var default independent of the binary's own
`#[arg(default_value_t = ...)]` — both must move together or a real
deployment keeps running at the old value regardless of the binary's new
default.

## 4. Index/query shape: unchanged, because `service_date` stays an equality predicate

The brief specifically raises the possibility that `service_date` needs to
become a *range* once results can span multiple days, which would argue for
reordering the existing `(service_date, origin_crs, scheduled, train_uid)`
index to `(origin_crs, service_date, scheduled, train_uid)`.

**This does not apply here, and the reason is §0's wire-shape choice, not an
oversight.** Because one search always resolves to exactly one `service_date`
value (today, by default, or the caller's `date`), `service_date` remains a
**plain equality predicate** in `search_schedule_calling_point_departures`'s
query — never a range. Postgres does not care whether an equality-bound
parameter happens to literally be `CURRENT_DATE` or some other date; the
query plan, the index used, and the cost are identical either way. The
existing `schedule_destination_departures_calling_point_idx` (`service_date,
origin_crs, scheduled, train_uid`) — and the underlying primary key it
sits alongside — already serve this query exactly as they serve today's
single-day case, with **zero regression risk** to the existing "today only"
path, because after this change that path is not a special case at all: it
is simply the `date` parameter's default value, going through the exact
same code and the exact same index.

A true multi-day-range search (one call spanning several `service_date`
values) was considered and rejected: CIF schedules are day-scoped by
construction (a `service_date` is the rail day a schedule runs on;
mid-journey midnight-crossing is already a known, unsolved rough edge per
the destination-arrival design doc's Open Question 2, not something this
document should make worse by inventing a range query on top of it), a
single-day-per-search UX matches how a rider actually thinks about the
question ("what runs on day X"), and a range query would force a genuine
`service_date` range scan across N partitions of the index — the exact
column-reordering cost the brief was right to flag as a real risk, just for
a shape this document does not adopt.

## 5. Wire shape

**New optional query parameter: `date`, `"YYYY-MM-DD"`.** Absent → defaults
to today (computed exactly as today, §6). Present → parsed with
`chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")`; a parse failure is a
`400` ("date must be YYYY-MM-DD"), matching every other malformed-input
field on this route. A syntactically valid but out-of-window date (more
than 7 days in the future or more than 7 days in the past, relative to the
same single `today` reading used everywhere else in the handler) is also a
`400` — **not** a `404`. This route already draws a firm line between "your
input doesn't make sense" (400) and "your input makes sense but nothing
matches today" (404 for no publish at all, `200 []` for published-but-
unmatched); a date outside the supported window is the former: the caller
is asking a question this deployment has already decided it will never be
able to answer, not one that happens to have no data today. Message:
`"date must be within 7 days ago and 7 days from today"`.

`from`/`to`/`destination_from`/`destination_to`/`origin`/`destination`/
`station`/`limit`/`after` are **completely unchanged** in shape and
validation — `date` is purely additive.

**`from`/`to`'s `now`-forward default only applies when `date` resolves to
today.** This is the one genuinely new behavioral rule this feature
introduces, and it is stated explicitly here because getting it wrong would
be a second instance of exactly the bug class `baa4e75`/`8250a9a` already
fixed once (§6):

```rust
let scheduled_from = if service_date == today {
    match from_time {
        Some(from) => std::cmp::max(now, from),
        None => now,
    }
} else {
    from_time.unwrap_or(chrono::NaiveTime::MIN)
};
```

For any date other than today — forward or backward — there is no "now" to
be forward of; the caller browsing next Tuesday wants the whole day (or
whatever `from`/`to` they explicitly supplied), not "next Tuesday's rows
whose *time-of-day* happens to be numerically after the *current* clock
time," which is what naively applying today's `max(now, from)` logic to a
different date would silently do — a real, distinct correctness bug this
document is naming and fixing before it ships, not after.

### Cursor: unchanged

The brief specifically flags that `"HH:MM:SS|train_uid"` "will misbehave
once results can span multiple days." **Under this design, results never
span multiple days** — §0/§4 already established that one search is pinned
to exactly one `service_date`. The cursor's job is only to resume the same
query at the same `(scheduled, train_uid)` position; the `date` (like
`station`/`origin`/`destination`/every other filter) is not carried inside
the cursor and was never carried inside it — it is a caller-supplied query
parameter the frontend already resends verbatim on every "Load more" call
(`TrainSearchForm.tsx`'s `searchParams()` + `after`), the same way `station`
itself is resent today. No change to `encode_cursor`/`decode_cursor`, to
`CallingPointDepartureCursor`, or to the query's `ORDER BY`/keyset predicate
is needed.

## 6. Timezone correctness

**Rule: exactly one `Utc::now()` read per request, unchanged from the
existing fix.**

```rust
let london_now = chrono::Utc::now().with_timezone(&chrono_tz::Europe::London);
let today = london_now.date_naive();
let now = london_now.time();
```

Both `today` (used for the default-`date` fallback, the window-bounds
check, and the `service_date == today` branch in §5) and `now` (used only
inside that same branch) are derived from this **one** read, exactly as
`baa4e75` established. The requested `date` — parsed independently — is
compared only against this same `today`, never against a second,
independently-computed value. This is the specific, concrete guard against
reintroducing the bug class `baa4e75`/`8250a9a` fixed: that bug was two
*separate* `Utc::now()` reads disagreeing about the calendar day around the
UTC/London midnight boundary during BST; the fix, and this document's
extension of it, is "compute the wall-clock moment once, derive everything
else from that one value."

**New test**, alongside the existing
`trains_search_hides_a_departure_inside_the_utc_vs_london_gap`:
`trains_search_applies_now_forward_only_when_date_is_today` — seeds a row
just after London midnight on a **future** date (`today + 1`) and a row at
the same clock time on **today**, asserts the future-dated row is returned
regardless of what `now` currently reads (no now-forward filtering off
`date`'s own day), while the today-dated row is still correctly excluded or
included by the existing `now`-forward logic depending on whether it's
before or after the shared `london_now` reading — directly exercising §5's
`service_date == today` branch, not just the pre-existing single-day BST
gap case. A second test, `trains_search_rejects_a_date_outside_the_supported_window`,
asserts `today - 8`/`today + 8` both 400, and `today - 7`/`today + 7` both
succeed (boundary-inclusive, per §5's "within 7 days" wording) — this test
is itself independent of BST/UTC skew (it only needs `today`, not `now`),
but is placed in this section because it exercises the same single
`london_now`-derived `today` value the timezone fix depends on, so a
regression that reintroduces a second `Utc::now()` read anywhere in the
handler has a real chance of being caught by it too if the two reads ever
disagree about the day.

## 7. Explicitly out of scope

- **Reconstructing historical days beyond the retention window** (Regime B
  from the past-dates sibling document). Never attempted by this design —
  a date outside the window 400s, full stop; no on-demand
  compute-and-cache path exists for either direction.
- **A specific final value for `FORWARD_WINDOW_DAYS`/`retention_days`
  beyond the ones chosen here (7/7).** Recommended and justified in §1, not
  claimed to be provably optimal — like the future-dates sibling document's
  own 14-30-day recommendation, this is a reasoned starting point, not a
  measured one, and real `schedule_reference_cycle_duration_seconds`
  before/after data should gate whether it needs to change.
- **A real, fresh `pg_column_size` measurement of the current 7-column
  table+index.** §1.3's ~75MB/day figure is an extrapolation of the sizing
  design doc's own ~40-60MB/day estimate for the original 5-column schema,
  not a new measurement. Named as a prerequisite task, not blocking this
  design.
- **Extending `schedule_line_population` or `schedule_network_departures`
  to a multi-day window.** Out of scope; `schedule_network_departures`'s
  own `now`-forward-at-publish-time design (§1.1 of the sizing addendum)
  makes it a poor fit for this regardless, and `schedule_line_population`'s
  own future-dates question is already scoped by its own sibling document,
  unrelated to this route.
- **A `date` range spanning multiple `service_date` values in one search.**
  Considered and rejected in §4.
- **Fixing the pre-existing midnight-crossing `destination_arrival`
  same-day-only limitation** (destination-arrival design doc's Open
  Question 2). Inherited unchanged; this document's `date` parameter does
  not interact with or worsen it.
- **Any operator filter, any LDBWS/live-board multi-day search, any
  resident whole-network index, any synchronous `api` → `schedule-reference`
  call.** All untouched, all still binding, per every predecessor document's
  own out-of-scope section.
- **A `date`/calendar picker's exact visual design.** Left to
  implementation, per this route's own established "visual treatment is
  implementation/design review" convention. §8 names which existing
  component to reuse, not its exact copy/props.

## 8. Frontend

`frontend/components/TrainSearchForm.tsx` gains one new optional field:
**a date picker**, reusing `DatePickerInput` from `@mantine/dates`
(`frontend/app/lines/[id]/history/HistoryRangePicker.tsx` is this
codebase's existing precedent for that exact component, already imported
and styled) — **not** `DateTimePicker` (`TrackTrainForm.tsx`'s component),
since time-of-day stays in the existing separate `From`/`To` `TextInput`
fields per §0; the date picker only ever selects a bare calendar date.
`minDate`/`maxDate` bound it to `today - 7`/`today + 7`, matching §5's
window exactly, so the client cannot construct a request the server will
400. Left blank (its default), no `date` param is sent — identical wire
behavior to today. `frontend/app/trains/page.tsx` gains a `?date=` prefill
param alongside its existing `?station=`/`?origin=`/`?destination=`
convention, and `TrainSearchRow`'s consuming code needs no change (the
response shape is unaffected — `date` only changes which day's rows come
back, not their shape).

## 9. Summary of decisions against the brief's six points

1. **Window: 7 days forward, 7 days back** (§1) — chosen for a
   search/browse feature's realistic horizon, not the pin-matching
   correctness problem the 14-30-day sibling recommendation was solving.
   ~377,000 rows/day × 16 resident days (7 forward + today + 8 retained
   backward, the extra retained day a safety margin) ≈ 6M rows, ≈ ~1.2GB
   table+index, versus today's ~225MB baseline (§1.3).
2. **Publish-side: extend the loop, not on-demand** (§2) — a small,
   mechanical change (`publish_schedule_destination_departures` is already
   date-parametric); on-demand rejected for the same reasons the
   future-dates sibling document rejected it, sharpened by N=7 being cheap
   enough that there's no real savings to justify the new architecture.
3. **Retention: forward needs nothing (ages in naturally, stated
   explicitly, §3); backward is a one-line config default change**
   (`retention_days` 2 → 7), zero new code.
4. **Index/query shape: unchanged** (§4) — `service_date` stays an equality
   predicate because one search is pinned to exactly one day (§0), so the
   existing `(service_date, origin_crs, scheduled, train_uid)` ordering
   already serves this correctly with no regression to the existing
   today-only path.
5. **Wire shape: one new optional `date` param (`"YYYY-MM-DD"`)**, `from`/
   `to`/`destination_from`/`destination_to` stay bare `"HH:MM"` (§5) —
   composes cleanly with the existing two independent time-range pairs; the
   cursor needs no change because results never span multiple days.
6. **Timezone: one `Utc::now()` read, unchanged from `baa4e75`'s fix**,
   plus one new rule (`now`-forward only applies when `date == today`) and
   two new tests targeting exactly that boundary (§6).
