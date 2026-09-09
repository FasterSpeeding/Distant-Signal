# Design: Multi-Day Train Search (`GET /public/trains/search?date=`)

**Status: design proposal, approved for implementation by the requesting
session — same no-separate-human-sign-off posture this pipeline's
immediate predecessor document already took
(`docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md`,
its own Status line).**

Product request, verbatim intent: `GET /public/trains/search` (backing the
`/trains` page) is CIF-SCHEDULE-derived and meant to search over the WHOLE
published timetable, not just trains happening soon — but today it is
hardcoded to "always today, server-side"
(`crates/api/src/routes/trains.rs`'s own module doc, lines 34-35: "There is
deliberately NO date parameter"). This document extends the search to a
caller-chosen day, bounded by a real forward/backward window, while keeping
every constraint the three immediate predecessor documents already
established (no resident index, no synchronous cross-service call, publish-
then-poll, `now`-forward evaluated at request time, keyset pagination).

Required reading consumed in full before this document was written:
`crates/api/src/routes/trains.rs` (whole file, including `db_tests`);
`crates/api/src/data/queries.rs` (the whole `schedule_destination_departures`
section, `:900-1230`); `crates/api/src/render.rs`
(`calling_point_departure_json`); `crates/schedule-reference/src/main.rs`
(whole file, including `poll_once_tests`); `crates/schedule-query/src/
resolve.rs` (`resolve_for_date`, `schedules_touching`, `schedule_for_uid`,
`departures_by_destination_crs`, and the `ScheduleIndex` they hang off);
`crates/aggregator/src/queries.rs::prune_schedule_destination_departures`;
`crates/api/migrations/20260907130000_schedule_destination_departures.sql`,
`20260908120000_schedule_destination_departures_calling_point_search.sql`,
`20260908130000_schedule_destination_departures_destination_arrival.sql`;
`frontend/components/TrainSearchForm.tsx`, `TrainSearchForm.test.tsx`,
`frontend/app/trains/page.tsx`; `frontend/app/lines/[id]/history/
HistoryRangePicker.tsx` and `frontend/components/TrackTrainForm.tsx` (for
this codebase's existing `@mantine/dates` usage); commits `baa4e75` and
`8250a9a` (the UTC-vs-London-local skew fix and its regression test) and
their diffs; and, for the architecture this document builds directly on:
`docs/superpowers/specs/2026-09-07-train-listing-page-design.md`,
`2026-09-07-train-listing-destination-search-sizing-design.md`,
`2026-09-08-calling-point-train-search-design.md`,
`2026-09-08-destination-arrival-time-filter-design.md`,
`2026-09-06-schedule-line-population-future-dates-design.md`,
`2026-09-06-schedule-line-population-past-dates-design.md`.

## 0. Confirming the constraint the brief asked to verify

`crates/schedule-reference/src/main.rs:202` computes `let today =
chrono::Utc::now().date_naive();` once per cycle and
`publish_cif_derived_products` (`:176-226`) passes that single date into
`publish_schedule_destination_departures` (`:442-478`), which in turn calls
`schedule_query::departures_by_destination_crs(index, today, NaiveTime::MIN,
&tiploc_to_crs)` exactly once. Combined with
`crates/aggregator/src/queries.rs::prune_schedule_destination_departures`
deleting `WHERE service_date < CURRENT_DATE - retention_days` (default `2`,
`crates/aggregator/src/config.rs:132`), the table holds roughly today,
yesterday, and the day before, and nothing else. **Confirmed: changing the
route's params to accept a date is a no-op today** — there is nothing to
read for any other date. This document's center of mass is therefore the
*publish* side, not the route.

**Also confirmed, and this is what makes the rest of this document
tractable**: `schedule_query::resolve_for_date` (`resolve.rs:42-70`),
`schedules_touching` (`:77-96`) and `schedule_for_uid` (`:361-364`) are
already fully date-parametric — `date` is a bare parameter compared only
against each `RawSchedule`'s own `date_from`/`date_to`/`days_of_week`, with
no `>= today` or `== today` special-casing anywhere in that crate.
`departures_by_destination_crs` (`:263-...`) is the same shape: `date` and
`now` are independent caller-supplied parameters, already used today with
`now = NaiveTime::MIN` to mean "no `now`-forward filter, publish the whole
day" (`main.rs:461`, its own doc comment point 1). **Nothing in
`schedule-query` needs to change.** The entire gap is that
`schedule-reference`'s `main.rs` only ever calls these functions with one
`NaiveDate` per cycle, and that `routes/trains.rs` only ever reads one
`service_date`. Both of those are mechanical, bounded changes — see §2 and
§4.

## 1. How many days, and why

### 1.1 The cost multiplier that bounds this decision

This table is not `schedule_line_population` (~109 rows/service_date). It is
the flat, one-row-per-departure-bearing-calling-point table the sizing
addendum built specifically because no cap is defensible:
`schedule_destination_departures_rows`'s own doc comment
(`main.rs:344-380`) puts the real, measured figure at **~377,000 rows per
service date**, ~30MB at the current (post-`true_origin_crs`/
`destination_arrival`) ~80 bytes/row, budgeted at ~100 bytes/row (~37.7MB)
for headroom. Every day this document adds to the window multiplies that
number directly — there is no sub-linear trick available, because the whole
point of the flat shape (§3 of the calling-point doc's required reading) is
that it refuses to cap or bucket.

### 1.2 Recommendation: 7 days forward, 1 day backward

**Forward: `today..=today+6` (7 calendar days, inclusive of today).**
Backward: no publish change at all — "yesterday" is already resident under
the existing `retention_days = 2` default (§1.4), formalized here as an
explicitly supported, tested query value rather than an accidental leftover.
No date further back than yesterday is supported (§1.5).

**The arithmetic, stated plainly, as this repo's own sibling documents
always do:**

| | Per day | × 7 (forward window) |
|---|---|---|
| Rows | ~377,000 | ~2,639,000 |
| Publish payload (at ~80 bytes/row, current real figure) | ~30.2MB | ~211MB total, but **split across 7 independent POSTs, one per date** (§2.2) — each individually ~30MB, comfortably under the 100MB `DefaultBodyLimit::max` (`crates/api/src/routes/mod.rs:86`) |
| Publish payload (at the ~100 bytes/row budgeted figure) | ~37.7MB | ~264MB total, same per-call split — each call still ~38MB |
| Table+index storage (addendum's own ~40-60MB/day figure, bumped slightly for the two extra columns) | ~50-60MB | ~350-420MB resident for the forward window alone |

Including the 1-2 days the existing retention window keeps behind "today"
(§1.4), steady-state residency is roughly **8-9 distinct service dates,
~3.2-3.4M rows, ~450-540MB of table+index** — comfortably inside what this
service already handles every cycle for its other from-scratch rebuilds
(`stanox_crs`, ~3,100 rows, fully rewritten every cycle; the whole-network
research doc's own comparison point, cited by the schedule_line_population
future-dates doc, §1, for the identical argument). The genuinely new cost is
**publish-call count and compute**, not storage — see §1.3 and §2.

**Why 7, not 14-30 (the sibling `schedule_line_population` document's own
recommended range) and not more:** that document's recommendation was sized
against a table costing **~109 rows/service_date** — a table three and a
half thousand times smaller per day than this one. Its own Dimension 3
explicitly flags "the real unknown is bytes-per-row" for that table and
treats `schedule_network_departures`'s ~2,500-row/day, capped shape as the
*safe* comparison point, contrasting it with `schedule_line_population`'s
own larger, uncapped rows. `schedule_destination_departures` is already the
larger, uncapped case that document worried about, and at `377,000 ×
(40-60) MB` per day-equivalent cost, a 14-30 day window (`5.28M-11.3M`
rows, `1.7-2.3GB`, `280-540MB` publish payload per date... compounding to
needing real, careful chunked-publish engineering at a larger scale) is a
materially bigger bet for a search/discovery feature than it was for the
pin-matching correctness gap that motivated the sibling document (where an
unresolved pin sitting wrong for weeks is a correctness bug, not just an
absent nicety). A search feature's failure mode for "the window isn't wide
enough yet" is "the user sees an honest 404 for a date too far out" — not a
silent wrong answer — so there is no correctness argument compelling a wide
window the way there was for pin-matching. **7 days (today plus the next
six) is chosen as a deliberately bounded first step**, consistent with this
codebase's own repeated "ship the honest partial thing, revisit with real
usage" pattern (named explicitly in the original train-listing-page design
doc's Recommendation section and the whole-network research doc before it).
A week covers the single most commonly cited real use ("what's running
Thursday") without the multi-hundred-megabyte-per-date publish throughput
this document would otherwise have to design carefully around on day one.

### 1.3 Publish-compute cost at N=7

`departures_by_destination_crs` runs once per `(date)` over the SAME
per-cycle `ScheduleIndex` (`main.rs:194`, built once, shared across every
CIF-derived product exactly as it is today — Task 3 of the whole-network-
trip-search plan's own finding, reconfirmed by the schedule_line_population
future-dates document's Dimension 2). Going from 1 date to 7 means this
resolve pass runs 7× per cycle instead of once — each iteration paying the
same bounded, already-characterized `O(total UIDs in index)` cost the
existing single-date publish already pays. This publish fires **once per
CIF delivery, roughly daily** (`config.rs:19-23`'s own doc comment,
reconfirmed by the sizing addendum's §1.3), not once per 30-minute poll
tick, so a 7× multiplier of an already-cheap, once-daily pass is not
expected to threaten the 1800-second tick budget. Not independently
benchmarked in this pass — per the sibling documents' own consistent
posture, this is named as something to watch via the already-emitted
`schedule_reference_cycle_duration_seconds` histogram (`main.rs:49-52`)
after shipping, not something this document claims to have measured.

### 1.4 Backward: "yesterday" is free, already-correct, and needs no publish change

`prune_schedule_destination_departures(pool, retention_days)` deletes `WHERE
service_date < CURRENT_DATE - retention_days` with a default of `2`
(`aggregator/src/config.rs:132`, `queries.rs:544-552`'s own doc comment).
That means, at any point in the prune cycle, rows survive while
`service_date >= CURRENT_DATE - 2` — today, yesterday, **and** the day
before yesterday are all resident right up until the next prune run trims
the oldest one. Critically, yesterday's row was written *on* yesterday,
from *yesterday's own* live CIF extract — this is exactly Regime A from
`2026-09-06-schedule-line-population-past-dates-design.md`'s Decision 3
(the case that document found genuinely risk-free), not Regime B
(reconstruction from today's extract, which is where that document's real,
unresolved STP-overlay risk lives). Reading yesterday's already-published
row back today carries no correctness risk beyond what today's own read
already carries. **Recommendation: allow `date` to resolve to yesterday,
formalizing what retention already provides, with zero publish-side
change and zero retention-config change** (the existing default of `2`
already guarantees yesterday survives until the very end of its day,
per the "strictly `<`, never `<=`" comparison `queries.rs:550`'s own doc
comment calls out).

### 1.5 Explicitly not supported: further back than yesterday

Going back two or more days would mean either (a) bumping
`retention_days` further, trading the "protect the producer's edges"
margin that default exists for against an unrelated consumer-facing
feature, or (b) reconstructing a pruned date from today's extract — which
is exactly Regime B, the past-dates document's own named, unresolved,
"silently confidently wrong" risk (a short-term STP overlay whose own
validity window has fully elapsed may simply no longer be present in a
later extract, and `resolve_for_date`'s `min_by_key` would happily fall
through to a surviving `Permanent` base pattern with no signal it might be
wrong). This document does not reopen that question; it is out of scope
here exactly as it was scoped out there.

## 2. Publish mechanism: extend the existing loop, per date, not on-demand

### 2.1 Re-examining "publish-then-poll, never synchronous" for this case — the answer is still no, including for far-future dates

`routes/trains.rs`'s own module doc (`:14-19`) states the route "owns the
`now`-forward boundary" and is "a publish-then-poll read... never a
synchronous call." The brief asks whether that argument still holds once
dates further into the future are in play, since those are queried more
rarely. It does, for the same reason
`2026-09-06-schedule-line-population-future-dates-design.md`'s Dimension 4
already worked out for the sibling table: an on-demand, compute-on-first-
request path only avoids wasted work if it *never* needs to be
re-validated — but it does, because a later CIF delivery can correct an
already-published future date (a VSTP/STP-overlay change), and the only way
an on-demand cache would pick that up is by re-resolving on every new
delivery anyway, which is simply Approach A (fixed window, recomputed every
cycle) with extra moving parts and no net simplification. On-demand compute
also requires `api` to gain a dependency it does not have today — either
linking `schedule_query` and holding a `ScheduleIndex` itself (duplicating
`schedule-reference`'s job and requiring access to the CIF delivery files
`api` has never read), or a new synchronous request/response endpoint on
`schedule-reference`, which has no inbound HTTP server at all today (it
only ever POSTs out). Both are genuinely new architecture, not an extension
of the existing pattern, and neither is justified for a 7-day window this
small. **Decision: extend the existing proactive-publish loop, unchanged
in kind, to cover 7 dates instead of 1. No on-demand path, for any date in
the supported window or beyond it.**

### 2.2 The loop, and the already-correct chunking boundary

`publish_schedule_destination_departures` (`main.rs:442-478`) changes from
resolving and posting exactly `today` to looping over a small pure helper:

```rust
/// Every calendar day this service proactively computes and publishes
/// `schedule_destination_departures` for, starting from `today`. A pure
/// function (no I/O) so the window size is unit-testable in isolation --
/// same "pure logic split out of the I/O function" convention as
/// `lines_to_publish`/`schedule_destination_departures_rows` in this same
/// file.
const DESTINATION_DEPARTURES_FORWARD_DAYS: i64 = 7;

fn dates_to_publish(today: chrono::NaiveDate) -> impl Iterator<Item = chrono::NaiveDate> {
    (0..DESTINATION_DEPARTURES_FORWARD_DAYS).map(move |offset| today + chrono::Duration::days(offset))
}
```

— then, inside `publish_schedule_destination_departures`, one
`departures_by_destination_crs` call and one `post_batch` call **per date**,
not one call covering every date at once:

```rust
for date in dates_to_publish(today) {
    let by_destination =
        schedule_query::departures_by_destination_crs(index, date, chrono::NaiveTime::MIN, &tiploc_to_crs);
    let rows = schedule_destination_departures_rows(by_destination, date);
    if let Err(err) = common::ingest::post_batch(
        client,
        &config.schedule_destination_departures_url,
        internal_oauth,
        &rows,
        "schedule-derived destination departures rows",
    )
    .await
    {
        tracing::error!(error = ?err, service_date = %date, "failed to publish schedule-derived destination departures for this date; other dates in this cycle are unaffected, will retry next cycle");
    }
}
```

**This is a smaller, simpler mechanism than the "chunk a too-big publish
and teach the ingest handler `first-chunk-clears-the-day` semantics"
fallback both `main.rs`'s own doc comment (`:436-441`) and the sizing
addendum (§3, "Escape hatch") already anticipated — and it supersedes that
fallback rather than needing it.** That fallback was designed for a
*different* problem: a single day's ~377,000 rows someday exceeding the
100MB limit on their own, which would require splitting *one day's* rows
across multiple POSTs and teaching `upsert_schedule_destination_departures`
to distinguish "first chunk, clear the day" from "later chunk, append
only." Extending to 7 *days* does not have that problem at all: each day's
~30-38MB is already, individually, comfortably under the 100MB limit
(§1.2's table), so the natural, already-correct chunking boundary is the
calendar day itself, and **`upsert_schedule_destination_departures` needs
no change whatsoever.** Its existing behavior — `DELETE FROM
schedule_destination_departures WHERE service_date = ANY($1)` over the
batch's own distinct dates, then `UNNEST`-insert
(`crates/api/src/data/queries.rs:1002-1053`) — already wholesale-replaces
exactly one service date correctly when the batch it's given happens to
contain rows for exactly one date, which is precisely what each of the
7 per-date calls now sends it. This is a genuinely pleasant, confirmed
consequence of the data layer's existing per-batch-scoped DELETE, not a
new property being added for this feature.

**Per-date error isolation is a deliberate, new improvement, not
incidental.** Looping with `tracing::error!` + implicit `continue` (the
loop simply moves to the next iteration) means a transient POST failure for
one date (say, `today+4`) does not prevent the other six dates in that
cycle from publishing correctly. The monolithic single-POST version this
replaces could only fail or succeed as a whole; this version degrades
per-date, which is strictly better given there is now something to degrade.

**No change to `schedule_destination_departures_rows`, to
`departures_by_destination_crs`, or to `schedule_query` at all** — every
per-date call reuses the SAME already-correct, already-tested
row-shaping/flatten function, just called 7 times with different `date`
values instead of once with `today`.

### 2.3 No change to `schedule_network_departures`

Exactly the same conclusion the sibling future-dates document already
reached for this same question (its own §4, "`schedule_network_departures`
should NOT be extended by this same project"): it is a different table for
a different job (a live next-few-departures picker, capped at 10/station),
and its cap/correctness tradeoffs are unrelated to this document's. Not
touched here.

## 3. Retention/pruning: no change required, stated explicitly rather than left implicit

**Forward direction: nothing needed.** A date published under the 7-day
window ages into "today" naturally as the calendar advances, exactly the
same finding the schedule_line_population future-dates document already
made for its own sibling table (§4 of that document: "`schedule_network_
departures`... future days age into 'today' naturally"). There is no
forward-edge table to prune — the per-date wholesale-replace (§2.2) already
means a date that falls out of the 7-day window simply stops being
re-published; it is not actively deleted by anything, it just stops being
refreshed, and the EXISTING trailing-edge prune job (below) is what
eventually removes it once it is old enough.

**Backward direction: `prune_schedule_destination_departures` and its
default of `2` are unchanged.** §1.4 already established that this default
already keeps exactly the one backward day (`yesterday`) this document adds
support for, with margin to spare. No config change, no code change.

**Net effect: zero changes anywhere in `crates/aggregator`.** This is a
positive, confirmed finding, stated explicitly per the brief's own
instruction not to leave it implicit: the existing retention job was sized
for "protect the producer's edges around the midnight/delivery-timing
boundary," and it turns out to already be exactly the right shape for "keep
one day of read-side backward lookback" too, for the same underlying reason
(both needs are satisfied by "don't delete a date's row the instant the
calendar moves past it").

## 4. Index/query shape: no change, because of the wire-shape decision in §5

The brief's own framing (point 4) worried that a multi-day search would
turn `service_date` from an equality into a range, forcing a reconsideration
of the existing `(service_date, origin_crs, scheduled, train_uid)` index
order in favor of a station-first `(origin_crs, service_date, scheduled,
train_uid)` one. That concern is real **only if one search request can
itself span multiple `service_date` values** — and §5 deliberately chooses
a wire shape where it cannot: one `date` parameter, resolved to exactly one
`NaiveDate`, still an equality filter on every single request, exactly as
today. Under that shape, `service_date` never becomes a range within a
query; it simply takes a caller-chosen value instead of a hardcoded one.

**Working through the rejected alternative's own index reasoning, because
the brief asked for it shown rather than asserted:** if `from`/`to` had
instead become full datetimes capable of spanning a day boundary within one
request, `service_date` genuinely would become a range bound
`(service_date, scheduled) BETWEEN (d1, t1) AND (d2, t2)`. Under the
*current* column order `(service_date, origin_crs, scheduled, train_uid)`,
a btree index can only use a *leading* column as a range condition if every
column before it is an equality and use trailing columns as index
conditions at all if every column before THEM is equality too (the
leftmost-prefix rule) — so a `service_date` *range* as the very first column
would mean `origin_crs` could no longer be pushed down as an index
condition, only as a post-filter: the scan would walk every row across the
whole date range at every station, not just the searched one. Reordering to
`(origin_crs, service_date, scheduled, train_uid)` would fix that (equality
on `origin_crs` first, then `service_date` as a range immediately after it
still qualifies under the leftmost-prefix rule), **and would cost nothing
for today's existing single-day equality query**, since two equality
columns in either relative order are equally optimal for a btree. That
reordering would have been the right call *if* the wire shape required a
genuine date range per query. It is not needed under the wire shape this
document actually picks (§5), so **no migration, no index change, here.**
This reasoning is recorded so a future document that *does* want a true
date-range-per-query search (e.g. "show me this whole week in one page")
does not have to re-derive it.

**Confirmed directly: every function this feature touches already takes
`service_date` as a real, non-hardcoded parameter.**
`search_schedule_calling_point_departures` (`queries.rs:1119-1131`),
`schedule_destination_departures_published_for`
(`queries.rs:1070-1081`) both already accept `service_date:
chrono::NaiveDate` as an ordinary argument — they were written this way
from the calling-point design doc onward, never hardcoded to "today"
internally. **This means the entire `crates/api/src/data/queries.rs` data
layer needs zero changes for this feature.** Only `routes/trains.rs`
changes: it currently always passes `today` into these already-generic
functions; it will instead pass whichever date the request resolves to.
This mirrors, almost exactly, the schedule_line_population future-dates
document's own Dimension 5 finding ("no caller anywhere assumes 'only one
date row exists at a time'... every read site already scopes by the exact
date it needs") — the same foresight pays off here a second time.

## 5. Wire shape: one `date` param, not datetime `from`/`to`

**Decision: add one new, optional query parameter, `date` (`"YYYY-MM-DD"`),
defaulting to today (London-local) when absent. `from`/`to` and
`destination_from`/`destination_to` stay exactly `"HH:MM"` time-of-day, now
interpreted as bounds within whichever day `date` resolves to, rather than
always today.**

This is chosen over turning `from`/`to` (and, for consistency,
`destination_from`/`destination_to`) into full datetimes, for three
concrete reasons:

1. **It is additive, not a breaking change to four existing fields.** Every
   existing caller of this route (today: exactly `TrainSearchForm.tsx`) that
   never sends `date` gets byte-for-byte the same behavior as before — the
   existing `db_tests` assertions about `from`/`to`/`destination_from`/
   `destination_to` parsing and semantics do not need to change at all, only
   gain siblings for the new `date`-bearing cases.
2. **It composes correctly with the SEPARATE `destination_from`/
   `destination_to` pair already layered on top of `from`/`to`** (per the
   task's own instruction to confirm this for consistency). Both pairs stay
   pure time-of-day, both scoped to the SAME one `date` the whole request
   resolves to — `destination_arrival` is a column on the same
   per-`service_date` row as `scheduled`, so there is exactly one day in
   play per request, and one `date` param cleanly scopes both time-range
   pairs at once. Had `from`/`to` become independent datetimes while
   `destination_from`/`destination_to` stayed bare times, the two pairs
   would have silently disagreed about which calendar day they apply to
   whenever a search crossed midnight — a real new ambiguity this shape
   avoids by construction, not by convention.
3. **It keeps `service_date` an equality in the query, not a range** — see
   §4's full reasoning for why that is also what keeps the index and the
   data layer untouched.

### 5.1 Route changes (`crates/api/src/routes/trains.rs`)

- `TrainSearchParams` gains `date: Option<String>`.
- New helper, mirroring `normalize_time`/`normalize_crs`'s existing shape:

  ```rust
  fn normalize_date(label: &str, raw: &str) -> Result<chrono::NaiveDate, (StatusCode, String)> {
      chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| {
          (StatusCode::BAD_REQUEST, format!("{label} must be a date in YYYY-MM-DD form"))
      })
  }
  ```

  Same "malformed input 400s, never silently ignored" posture this file's
  own module doc already argues for every other field.

- The `now`/`today` computation keeps its existing single-`Utc::now()`-call
  shape (§6 — this is the one piece of code this document's own brief
  explicitly warns is delicate) and gains a conditional:

  ```rust
  let london_now = chrono::Utc::now().with_timezone(&chrono_tz::Europe::London);
  let today = london_now.date_naive();
  let now = london_now.time();

  let requested_date = params
      .date
      .as_deref()
      .filter(|s| !s.trim().is_empty())
      .map(|s| normalize_date("date", s))
      .transpose()?;
  let service_date = requested_date.unwrap_or(today);

  // `now`-forward only makes sense for TODAY. A future day hasn't started
  // yet -- clamping it to the current clock time would hide its entire
  // morning for no reason. A past day is entirely over -- clamping it to
  // "now" would hide everything (today's wall-clock time is always later
  // than any time on a day that's already finished), making the whole
  // backward-lookback feature in §1.4 pointless. Only when the requested
  // day IS today does "now" mean anything at all.
  let scheduled_from = if service_date == today {
      match from_time {
          Some(from) => std::cmp::max(now, from),
          None => now,
      }
  } else {
      from_time.unwrap_or(chrono::NaiveTime::MIN)
  };
  ```

- `search_schedule_calling_point_departures` and the existence probe are
  called with `service_date` instead of the old bare `today` — both already
  take it as a parameter (§4), so this is the entire diff to the call site.
- The 404 body names the actual resolved date, not a hardcoded "today":
  `format!("no CIF-derived schedule data has been published for {}",
  service_date.format("%Y-%m-%d"))`. This is a deliberate, visible change to
  existing test `trains_search_nothing_published_for_today_is_a_404`'s
  assertion (`body.contains("today")` no longer holds when `date` is
  explicit) — that test is updated to assert against the now-resolved
  date string for its no-`date`-param case (which is still literally
  today, so a test asserting the literal ISO date it computes itself
  continues to prove the same thing), and a new sibling test covers the
  explicit-future-date 404 case by asserting the SPECIFIC requested date
  string appears in the body.
- Module doc comment (lines 34-35, "There is deliberately NO date
  parameter: like `get_station_schedule_departures`, this is 'always today,
  server-side'") is rewritten to describe the new, bounded window instead of
  asserting its absence.
- `calling_point_departure_json` (`render.rs`) gains a `service_date:
  chrono::NaiveDate` parameter, alongside its existing `station_crs` one,
  and emits a new `"serviceDate": "YYYY-MM-DD"` field on every row — the
  same "attach a whole-response constant onto every row at render time"
  convention `stationCrs` already established, not a new pattern. This is
  necessary, not cosmetic: see §7 for why the frontend needs this on every
  row rather than trusting its own request-time `date` state.

### 5.2 Cursor: unchanged, and here is why that is still correct

`encode_cursor`/`decode_cursor` stay `"HH:MM:SS|train_uid"`, exactly as
today. This file's own existing doc comment on `encode_cursor`
(`trains.rs:172-180`) already states the precedent this decision rests on:
"`origin_crs`... is now the fixed equality filter for the whole query, not a
value that varies within one page, so it carries no ordering information
and doesn't belong in the cursor." `date` (resolved to `service_date`) is
exactly such a fixed equality filter, constant across every row of one
paginated response — by the SAME reasoning already applied to `station` in
the predecessor document, it does not belong in the cursor either. The
caller is already responsible for resending every other filter param
(`station`, `origin`, `destination`, etc.) unchanged on a "Load more"
request; `date` joins that same existing contract, not a new one.

## 6. Timezone correctness: the exact failure mode this document must not reintroduce

Commits `baa4e75`/`8250a9a` fixed a bug where `today` and `now` were read
from two independent `chrono::Utc::now()` calls, which could disagree about
which calendar day it was near the UTC/London midnight boundary during BST
— the regression test added by `8250a9a`,
`trains_search_hides_a_departure_inside_the_utc_vs_london_gap`, pins this
down by seeding a row 20 minutes before the correct London-local `now` and
asserting it is excluded specifically during BST.

This document's design (§5.1) **keeps the single `london_now` read
completely intact** — `today` and `now` are still two fields split off
ONE `Utc::now().with_timezone(...)` call, exactly as today; nothing about
adding `date` touches that. The only new risk surface is the **conditional**
around it: whether `scheduled_from` clamps to `now` depends on `service_date
== today`, and `today` here is always the SAME already-correctly-computed
London-local date used everywhere else in this function. The caller-
supplied `date` itself is a bare, timezone-free `YYYY-MM-DD` civil date —
parsed with no timezone arithmetic at all (`NaiveDate::parse_from_str`,
§5.1) — so there is no second opportunity for a UTC/London mismatch to creep
in on the *input* side; the only comparison that matters is `NaiveDate ==
NaiveDate`, unambiguous regardless of DST.

**New tests this document adds specifically for this risk, alongside the
existing (unmodified) `..._hides_a_departure_inside_the_utc_vs_london_gap`
regression test:**

- `trains_search_omitting_date_matches_explicitly_passing_todays_own_date`
  — proves the default-resolution path and the explicit-date path are one
  code path, not two that could independently drift: the SAME request
  sent with no `date` and with `date=<today's own correctly-computed
  London-local date>` must return byte-identical results.
- `trains_search_a_future_date_is_not_clamped_to_the_current_time_of_day`
  — seed a row for `today+1` at a time-of-day EARLIER than the current
  wall-clock time; assert it IS returned (proves `now` does not leak across
  a day boundary the way a naive `max(now, from)` applied unconditionally
  would have hidden it).
- `trains_search_yesterday_returns_the_whole_day_not_just_what_remains`
  — seed rows for `today-1` both before AND after the current time-of-day;
  assert BOTH are returned (proves the backward-lookback window in §1.4 is
  not silently neutered by an incorrectly-applied `now`-forward filter,
  which would otherwise make it return nothing useful at all, since every
  time on a day that has already fully elapsed is "before now").
- `trains_search_malformed_date_is_a_400` and
  `trains_search_a_date_beyond_the_published_window_is_a_404_naming_that_date`
  — ordinary input-validation coverage, matching this file's existing
  pattern for every other field.

No change is made to `normalize_time`, to the existing `scheduled_from`
computation's reliance on one `london_now` read, or to the existing BST
regression test — this document adds a condition around that logic, it does
not touch its internals.

## 7. Frontend (`frontend/components/TrainSearchForm.tsx`, `TrainSearchForm.test.tsx`, `frontend/app/trains/page.tsx`)

### 7.1 Date picker: reuse `@mantine/dates`' `DatePickerInput`, single mode

`@mantine/dates` is already a dependency (`frontend/package.json`) and
already used twice in this codebase: `DateTimePicker` in
`TrackTrainForm.tsx` (date+time, for a pin's scheduled departure) and
`DatePickerInput` with `type="range"` in `HistoryRangePicker.tsx` (a date
*range*, for the history page). This feature needs neither of those shapes
— one single calendar date, no time component (time-of-day is already
handled by the existing `from`/`to` `TextInput`s). **Use `DatePickerInput`
in its default (single-date) mode**, the same component `HistoryRangePicker`
already imports, just without `type="range"`:

```tsx
import { DatePickerInput } from '@mantine/dates';
// ...
const [date, setDate] = useState<string | null>(initialDate ?? null);
// ...
<DatePickerInput
  label="Date (optional)"
  placeholder="Today"
  description="Defaults to today. Searches the published timetable for any day in the upcoming week, or yesterday."
  value={date}
  onChange={setDate}
  clearable
/>
```

`value`/`onChange` use `@mantine/dates`' own `DateStringValue` (`"YYYY-MM-DD"
| null`) shape — the same string shape `HistoryRangePicker`'s `value` state
already uses (`useState<[string | null, string | null]>`), so this needs no
new date-formatting utility; `date` (when non-null) is sent verbatim as the
`date` query param.

### 7.2 A real, necessary fix: per-row links must use the SEARCHED date, not the client's own "today"

`TrainSearchForm.tsx:141` currently computes `const today =
dayjs().format('YYYY-MM-DD')` once per render and uses it for BOTH the
`/train/{uid}/{today}` link (`:282`) and `TrackThisTrainButton`'s `date`
prop (`:287`), on the (currently true) assumption that every result is for
today. **Once a caller can search a different day, that assumption breaks**:
searching tomorrow and clicking "View live status" or "Track this train" on
a result would silently link to TODAY's `(uid, date)` pair instead of
tomorrow's — a real, wrong-train-identity bug, not a cosmetic one, since
`/train/{uid}/{date}` and `POST /Train/by-uid/{uid}/{date}/track` are both
keyed on the exact `(uid, service_date)` pair.

**Fix: use `row.serviceDate` (§5.1's new per-row field) instead of the
component-level `today` constant**, for both the link and the button:

```tsx
<TextLink href={`/train/${encodeURIComponent(row.uid)}/${row.serviceDate}`}>
  View live status
</TextLink>
<TrackThisTrainButton uid={row.uid} date={row.serviceDate} attachTicketId={attachTicketId} size="xs" />
```

This is why §5.1 puts `serviceDate` on every row rather than only in the
form's own request-time state: a row already in view from page 1 of a
"Load more" sequence must keep linking correctly even if, hypothetically,
the form's own `date` state were to change before the user clicks it (it
can't today, since the date field is disabled mid-search the same way every
other field implicitly is via `searching`/`canSearch`, but trusting the
server-echoed value per row is strictly more robust than trusting client
state to stay in sync, and costs nothing).

### 7.3 Everything else

- `TrainSearchRow` gains `serviceDate: string`.
- `searchParams()` sets `date` only when non-empty, same optional-param
  pattern `origin`/`destination`/`from`/`to` already use; `handleLoadMore`
  needs no change beyond that, since it already reuses `searchParams()`
  verbatim and therefore already resends `date` unchanged across pages
  (§5.2).
- `canSearch` gains no new validity check: `@mantine/dates`' `DatePickerInput`
  cannot produce a malformed string the way a free-text `TextInput` can, so
  there is no client-side `dateValid` needed — only the server's `400` for
  a genuinely malformed value guards that path, consistent with how this
  component already leans on the backend for some validation classes.
- `resultsContent()`'s `'unpublished'` copy (`:253-258`, currently "Today's
  scheduled timetable data isn't available yet...") becomes date-aware,
  e.g. "Scheduled timetable data for that date isn't available — it may not
  have been published, be outside the searchable window, or that station
  may not be one this feed covers."
- `frontend/app/trains/page.tsx`: `searchParams` type gains `date?: string |
  string[]`, destructured and passed as `initialDate`, same pattern as
  `initialStation`/`initialOrigin`/`initialDestination`.
- `TrainSearchForm.test.tsx`: existing tests are unaffected (no `date`
  sent → default behavior, unchanged wire contract); new tests cover
  sending `date` in `searchParams()` when set, omitting it when cleared,
  and — the regression this document exists to prevent — that a result
  row's "View live status" link and `TrackThisTrainButton` use
  `row.serviceDate`, not a client-computed "today", including a test that
  explicitly sets `serviceDate` to a DIFFERENT value than the client's own
  current date and asserts the link/button still use the row's value.

## 8. Explicitly out of scope

- **Any per-query date RANGE (e.g. "this whole week in one response").**
  §5 picks a single-`date`-per-request shape specifically to avoid this;
  §4 records the index-reordering work a future document would need if this
  is ever revisited.
- **Further backward lookback than yesterday, or any reconstruction of a
  pruned date.** §1.5; this is the past-dates sibling document's own
  unresolved Regime B risk, not reopened here.
- **Extending `schedule_network_departures`'s own window or cap.** §2.3;
  unrelated table, unrelated job, explicitly declined by the sibling
  future-dates document already and not reopened here.
- **Any change to `schedule_query`, its `ScheduleIndex`, `resolve_for_date`,
  `schedules_touching`, or `schedule_for_uid`.** All already fully
  date-parametric (§0); nothing here needed a change.
- **Any change to the table's schema, primary key, or either existing
  index.** §4; the wire-shape decision in §5 makes this unnecessary.
- **Any change to `crates/aggregator`**, including
  `prune_schedule_destination_departures`'s retention default. §3;
  already exactly the right shape.
- **Operator filtering, LDBWS/live-board search, the pure-terminus gap, or
  any other constraint already declined by the three immediate predecessor
  documents.** All still binding, all untouched.
- **Measuring real `schedule_reference_cycle_duration_seconds` impact before
  shipping.** Named in §1.3/§2 as a post-ship observation, matching this
  repo's own established posture of shipping a reasoned-but-unbenchmarked
  bounded change and watching the existing metric, not gating on a new one.

## 9. Open questions / risks

1. **7 days forward is a reasoned choice, not a measured one**, exactly like
   the sibling future-dates document's own 14-30-day range for
   `schedule_line_population`. If real usage shows users routinely want
   further out, widening `DESTINATION_DEPARTURES_FORWARD_DAYS` is a
   single-constant change with linearly predictable cost, per §1.2's table
   — revisit with real query-date-distribution data once this ships, the
   same way the sibling document named its own future metric need.
2. **Publish-call count going from 1 to 7 per cycle is new, unmeasured
   latency**, even though each call is independent and the cycle itself
   runs roughly once a day. Not expected to be a problem (§1.3) but not
   independently benchmarked here.
3. **The 404 semantics already diverged from `get_station_schedule_
   departures`'s** under the calling-point-search predecessor document
   (§3 of the sizing addendum); this document does not reopen that, but a
   caller requesting a date genuinely outside the 7-day-forward/1-day-
   backward window gets the SAME 404 shape as "no publish landed at all,"
   with no way to distinguish "this date is simply unsupported" from "this
   date's delivery failed to process" from the response alone. Named, not
   solved — matches this table's existing posture of not distinguishing
   those cases for any date, not a new gap this document introduces.
4. **`DatePickerInput`'s own min/max-selectable-date props are not wired to
   the 7-day-forward/1-day-backward window** in §7.1's sketch, meaning a
   user COULD pick a date outside the supported range in the UI and receive
   an honest 404 rather than being prevented from picking it at all. Left
   as an implementation-time UX polish (`minDate`/`maxDate` props already
   exist on this Mantine component and could be wired to the same
   7-day/1-day constants this document names), not a correctness gap —
   matches this component's own existing "visual/UX treatment is
   implementation/design-review, not fixed in the design doc" convention.
