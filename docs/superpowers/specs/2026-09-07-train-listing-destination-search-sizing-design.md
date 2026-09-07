# Design Addendum: Sizing the Destination-First Train Search

**Status: design proposal, not approved. Research and architecture only —
no migration, no Rust code, no frontend code, no config change in this
pass. This document exists to put an explicit decision in front of a human
decision-maker after an approved plan's own gating diagnostic returned a
number that its escape hatch says to stop on. Nothing here should be
implemented until §4's recommendation (or a variant of it) is picked.**

**What this supersedes.** This addendum replaces the **publish / storage /
query mechanism** of:

- `docs/superpowers/specs/2026-09-07-train-listing-page-design.md` §3,
  Approach B — specifically its "publish it from `schedule-reference` on
  the same cycle into a new table, copy-adjacent to
  `schedule_network_departures`'s own migration and POST/GET route shape"
  sentence, and its "a decision on the result cap's shape" bullet.
- `docs/superpowers/plans/2026-09-07-train-listing-page-implementation-plan.md`
  Tasks **1, 2, 4, 5** (changed outright), Task **7** (changed additively),
  Task **10** (one frontend control), and Tasks **3** and **6** (kept, with a
  single named correction each). It also adds one task the plan did not
  have. §5 gives the full task-by-task disposition.

**What this deliberately does NOT reopen.** The product decision in the
design doc's §3 Recommendation — destination-first whole-network CIF-derived
search, *not* Approach A (a reskinned per-station picker) and *not*
Approach C (line-scoped filtering) — stands unchanged and is not
re-litigated here. Neither is anything in the design doc's §4 (`/track` is
not replaced), §5 (the `/train/[uid]/[date]` CTA) or §6 (the exclusion
list, which remains binding in full — see §5 below). §2 of this addendum
states plainly which of the plan's 12 tasks are untouched, rather than
leaving it implied: **Tasks 8, 9, 11 and 12 are entirely unaffected**, and
Task 10 changes only by one frontend control.

Required reading consumed in full before this document was written:
`docs/superpowers/specs/2026-09-07-train-listing-page-design.md`;
`docs/superpowers/plans/2026-09-07-train-listing-page-implementation-plan.md`;
`crates/schedule-reference/src/main.rs`;
`crates/schedule-reference/src/config.rs`;
`crates/schedule-reference/src/discovery.rs`;
`crates/schedule-query/src/resolve.rs`;
`crates/api/migrations/20260904090000_schedule_line_population.sql`;
`crates/api/migrations/20260904110000_schedule_network_departures.sql`;
the `schedule_line_population`/`schedule_network_departures` section of
`crates/api/src/data/queries.rs` (`:771-897`);
`crates/api/src/routes/mod.rs`; `crates/api/src/routes/departures.rs`;
`crates/api/src/routes/lines.rs`; `crates/api/src/render.rs`;
`crates/common/src/ingest.rs`;
`docs/superpowers/specs/2026-09-07-shared-train-status-write-race-design.md`
(for tone/structure precedent).

## 0. Why this document exists

The plan's Task 1 is a controller-run diagnostic that measures the
destination-bucket distribution before Task 4 hard-codes
`MAX_DEPARTURES_PER_DESTINATION`. It carries an explicit escape hatch
(plan, Task 1 Step 3): *"If the size check fails at that value, stop and
flag it back to the repo owner: the honest fix would be pagination or a
narrower `now`-forward window, both of which are their own design pass and
are not this plan's to invent unprompted."*

Task 1 was run for real, against the live production CIF timetable extract
pulled from the running `schedule-reference` service's own delivery (not a
stale or synthetic file), using this codebase's actual
`schedule_query::ScheduleIndex`/`schedule_for_uid` resolution rather than a
text-scan approximation. It returned numbers the plan's own size check
fails on. This document is the design pass the escape hatch calls for.

It also, in the course of grounding that size check, found two errors of
fact in the plan itself — one arithmetic (§1.2) and one about the service's
actual publish cadence (§1.3). Both change which candidate fixes are viable,
so both are stated before any approach is evaluated.

## 1. What was measured, and what the plan got wrong

### 1.1 The measurement

For service date 2026-09-08, resolved network-wide:

| Fact | Value |
|---|---|
| Non-cancelled schedules resolving for the day | **25,305** |
| Busiest destination, London Waterloo (`WATRLMN`) — distinct terminating trains | **646** |
| `WATRLMN` departure-bearing calling points, `now` = `00:00` | **9,634** |
| `WATRLMN` departure-bearing calling points, `now` = `08:00` | **8,145** |
| `WATRLMN` departure-bearing calling points, `now` = `16:00` | **3,897** |
| Next several busiest destinations (Liverpool Street, Glasgow Central, Edinburgh, Victoria, Charing Cross, Leeds…), `now` = `00:00` | **~3,000-5,000 each** |

Two consequences follow immediately.

**The plan's default cap of 200 is not defensible at any reading.** It would
show a searcher the earliest 200 of Waterloo's 9,634 — about 2% of the day.

**"Special-case the outlier" is not available.** The next several busiest
destinations are within the same order of magnitude. A cap chosen to cover
Waterloo covers everything; a cap chosen for the median covers neither
Waterloo nor the twenty destinations behind it, which are precisely the
destinations a whole-network destination search exists to serve.

### 1.2 The plan's payload formula is structurally wrong — and correcting it
changes the answer

The plan's size check (Task 1 Step 3) is:

> the publish is one batch-array POST of roughly `(distinct destination CRS
> codes) × (cap) × (~55 bytes per JSON entry)`. With ~2,500 CRS codes and a
> cap of 200 that is ~27MB…

Evaluated at caps large enough to cover the measurement, that formula gives:

| Cap | Formula result |
|---|---|
| 200 | ~27.5 MB |
| 1,000 | ~137.5 MB |
| 10,000 | **~1.375 GB** |

against the plan's own ~60MB stop-and-flag threshold and the private
router's real hard limit, `DefaultBodyLimit::max(100 * 1024 * 1024)`
(`crates/api/src/routes/mod.rs:86`). That is the reported blocker, and at
face value it is catastrophic.

**But the formula treats the cap as a floor as well as a ceiling.** It
multiplies a worst-case bucket size by *every one of ~2,500 destinations*,
as if Bootle Oriel Road terminated 10,000 trains a day. It cannot be the
real bound, because of a structural property of the grouping:

> Every departure-bearing calling point is filed under **exactly one**
> destination bucket — the CRS of its own schedule's terminating calling
> point. `departures_by_destination_crs` (plan, Task 3 Step 4) takes
> `resolved.calling_points.last()` once per schedule and uses that single
> value as the bucket key for all of that schedule's entries.

So the true bound on total published volume is not a product but a sum, and
the sum is invariant to the cap:

```
Σ_d min(bucket_d, cap)  ≤  Σ_d bucket_d
                        =  total departure-bearing calling points network-wide
                        =  (non-cancelled schedules) × (mean departure-bearing calling points per schedule)
```

The measurement gives both factors, one directly and one by division:

- non-cancelled schedules for the day: **25,305**
- Waterloo's mean departure-bearing calling points per terminating
  schedule: **9,634 ÷ 646 = 14.9**

Waterloo is a suburban stopping terminus, so 14.9 is very likely *above* the
network mean, which is dragged down by freight, ECS and long-distance
schedules with few calling points. Taking 14.9 as a deliberately
conservative (high) stand-in:

```
25,305 × 14.9  ≈  377,000 entries
377,000 × ~55 bytes  ≈  20.7 MB
```

Even at an implausibly high mean of 20 calling points per schedule the whole
network's **uncapped, whole-day** destination-keyed publish is ~506,000
entries ≈ **~27.8 MB** — under the plan's own 60MB threshold and under the
100MB router limit with ~3.6x headroom. At a mean of 8 it is ~11 MB.

**This is the load-bearing correction in this document.** The reported
1.4GB is an artifact of the formula, not a property of the data. Publishing
*every* destination bucket *uncapped* for the *whole day* is roughly a
20MB POST — comparable to, and smaller than, the ~55MB stations feed that
`DefaultBodyLimit` was raised to 100MB for in the first place
(`crates/api/src/routes/mod.rs:76-85`).

**Stated honestly: the ~377,000 figure is an arithmetic re-derivation from
Task 1's own two measured numbers, not a third measurement.** It is a
correction of the plan's formula, not a substitute for running the
diagnostic again. §2's revised Task 1 measures `Σ_d bucket_d` directly —
which, usefully, the diagnostic already computes: it is the **sum** of the
third column of the same output whose **maximum** was reported.

### 1.3 The publish runs once per *delivery*, not once per 30-minute cycle

The plan describes the new publish as happening "on its existing 30-minute
cycle" (plan, Architecture paragraph and Task 2's migration header comment).
That is not what the service does.

- `crates/schedule-reference/src/config.rs:24-25` — `poll_interval_secs`
  defaults to `1800`, and that field's own doc comment (`:19-23`) says
  plainly: *"Independent of the underlying daily delivery cadence… most
  checks find nothing new, since a fresh delivery only lands roughly once a
  day."*
- `crates/schedule-reference/src/main.rs:101-107` — `poll_once` returns
  early whenever `Some(&delivery.dir_name) == last_processed_delivery`.
- `crates/schedule-reference/src/main.rs:158-161` —
  `publish_cif_derived_products` is only reached *past* that gate, on the
  same path that advances `last_processed_delivery`.

So the 30-minute interval is a **directory-listing check**, and every
CIF-derived publish — including the shipped `schedule_network_departures`
one — fires **roughly once a day**, when a new delivery appears.

Three consequences, all of which bear directly on the candidate fixes:

1. **The publish-time `now`-forward filter is evaluated once per day.**
   `publish_schedule_network_departures` calls `london_local_time_now()` at
   `main.rs:294` and passes it into `departures_by_crs`, whose filter is
   `if departure < now { continue; }` (`crates/schedule-query/src/resolve.rs:193`).
   Whatever the clock reads when the delivery lands is the boundary for the
   rest of the day.

2. **Any earliest-first cap therefore freezes at delivery time.** A cap of
   *N* keeps "the first *N* departures after the delivery landed" — not "the
   next *N* from now". Late in the day, those are all in the past.

3. **A rolling, per-cycle-refreshed window has no cycle to hang on.**
   Making one real would mean either re-reading and re-parsing the ~707MB
   MCA on every 30-minute tick (`read_prefixed_lines_multi`, `main.rs:73-86`,
   then `ScheduleIndex::from_text`, `main.rs:194`) or holding the
   `ScheduleIndex` resident between ticks. The first is the "second parse"
   the design doc's Approach B explicitly promises not to do; the second is
   the resident whole-network index its §6 explicitly excludes.

This also means the shipped `schedule_network_departures` has an inherited
staleness property of its own: `get_station_schedule_departures`
(`crates/api/src/routes/departures.rs:71-88`) relays the stored array
without re-applying a `now`-forward filter, so a station's "next 10" are the
first 10 after the delivery landed, not after the request. **That is not
this document's to fix** — see §5 — but the recommended approach happens not
to inherit it, and that is worth naming rather than discovering later.

## 2. The two constraints that were put back in question

The brief reopened two of the design doc's §6 exclusions. The answer is that
**neither needs relaxing**, and the recommended approach keeps both:

- **"No resident, permanently-in-memory whole-network index."** Kept.
  Approach C below runs against the same transient, stack-local
  `ScheduleIndex` built once per delivery at `main.rs:194`, in the same pass
  as the two existing products, exactly as the plan's Task 3 already
  specifies. No second parse, no cache, no resident structure.
- **"No synchronous request-time call from `api` into `schedule-reference`."**
  Kept. Approach C is publish-then-read, the same shape as
  `schedule_line_population` and `schedule_network_departures`.

Only Approach B below (the narrower rolling window) would have forced
relaxing the first, per §1.3 point 3. That is one more reason to reject it,
not a reason to relax the constraint.

## 3. Approaches

### Approach A — Keep the JSONB destination bucket; cap it larger; paginate on the read side

Keep the plan's shape exactly — `schedule_destination_departures
(destination_crs, service_date, departures JSONB)`, one row per destination,
the array capped per destination — but raise the cap to 500-1,000 and move
the pagination burden to `GET /public/trains/search` via a cursor or offset
into the stored array.

**Size is no longer the objection.** Under §1.2's corrected math a cap of
1,000 saves very little: only a handful of destinations exceed it, so
`Σ_d min(bucket_d, 1000)` differs from the uncapped sum by roughly the
overflow at the top ~20 destinations — about `(9,634 − 1,000) + 19 ×
(4,000 − 1,000) ≈ 66,000` entries, or **~3.6 MB out of ~21 MB**. The cap
buys ~17% of payload while truncating the busiest destinations by 75-90%.
That is a bad trade on its own terms.

**What the user does not see, and why the gap is not an honest one.** With a
1,000 cap, Waterloo's 9,634 entries become the earliest 1,000 — which,
against a rate of ~535 departure-bearing calling points per hour (9,634
across an ~18-hour operating day), is roughly the **first two hours after
the daily delivery lands** (§1.3). Search "trains to
Waterloo" at 18:00 and every published row is already in the past; the read
route honestly returns `200 []`, and the user sees "no trains to Waterloo
today."

The design doc's other gaps are honest because they are *nameable and
stable*: "no operator filter on CIF rows," "today only," "LDBWS covers ~286
stations." This one is neither. Its size depends on the time of day and on
what hour the daily delivery happened to land, and it is worst for exactly
the destinations a whole-network destination search exists to serve. A UI
cannot label it truthfully without saying something like "results may be
incomplete for busy destinations, more so later in the day, depending on
when today's timetable arrived." That is a silent gap wearing a caption.

**Secondary objection: the shape fights the query.** A JSONB array bucket
forces `jsonb_array_elements` over the *whole* blob on every request (the
plan's Task 5 `LEFT JOIN LATERAL`). Even at 1,000 elements that is an
unindexable full-blob expansion per request, and there is no cheap way to
keyset-paginate into it — an `OFFSET` into a lateral expansion re-expands
the whole array to skip rows. Read-side pagination is the right idea; this
storage shape is the wrong substrate for it.

**Verdict: reject.** Right instinct (pagination belongs on the read side),
wrong place to pay for it.

### Approach B — A narrower `now`-forward publish window

Publish only the next 2-3 hours of departures per destination, refreshed
each cycle, instead of the rest of the day.

**It does bound the bucket.** At the same ~535/hour rate used in Approach A,
a 3-hour window is ~1,600 entries and a 2-hour window ~1,070 — genuinely
small.

**It is defeated by §1.3.** There is no per-cycle refresh to hang it on: the
publish fires once per delivery, roughly daily. A 3-hour window computed at
delivery time is empty for the other 21 hours — strictly worse than
Approach A, not better. Making the refresh real requires either a second
parse of the 707MB MCA every 30 minutes or a resident `ScheduleIndex` —
the two things the design doc's §6 excludes, and the *only* candidate here
that would force relaxing one of them.

**It also narrows the product.** "Search for trains to X" would mean "trains
to X in the next three hours", which is a real regression against the
existing per-station picker's own framing — `departures_by_crs` is
`now`-forward with no window limit at all
(`crates/schedule-query/src/resolve.rs:167-173`), and the whole point of a
destination-first search is planning a journey, which routinely happens more
than three hours ahead.

**Verdict: reject.** It buys a bound the recommended approach gets for free,
at the cost of a §6 constraint and a materially worse product.

### Approach C (recommended) — A flat, one-row-per-departure table; publish the whole day uncapped; filter `now`-forward and paginate at read time in indexed SQL

Stop storing a bucket. Store the rows.

```sql
CREATE TABLE schedule_destination_departures (
    service_date    DATE NOT NULL,
    destination_crs TEXT NOT NULL,
    scheduled       TIME NOT NULL,
    train_uid       TEXT NOT NULL,
    origin_crs      TEXT NOT NULL,
    PRIMARY KEY (service_date, destination_crs, scheduled, train_uid, origin_crs)
);
```

The primary key is also the covering index for the only query shape the
route needs, in exactly the order it needs it. There is **no per-destination
cap and no publish-time `now`-forward filter** — the whole rail day is
published once per delivery, and every time-dependent decision moves to
request time, where the clock is actually correct.

**The read** (`GET /public/trains/search`) becomes an index range scan
bounded by `LIMIT`, not a blob expansion:

```sql
SELECT train_uid, origin_crs, scheduled
FROM schedule_destination_departures
WHERE service_date = $1
  AND destination_crs = $2
  AND scheduled >= $3                                   -- `now`, computed server-side
  AND ($4::text IS NULL OR origin_crs = $4)             -- optional origin filter
  AND ($5::time IS NULL OR scheduled <= $5)             -- optional `to=`
  AND ($6::time IS NULL                                 -- optional keyset cursor
       OR (scheduled, train_uid, origin_crs) > ($6, $7, $8))
ORDER BY scheduled, train_uid, origin_crs
LIMIT $9
```

Worst case — Waterloo, no origin filter, 8,145 matching rows at 08:00 — this
touches `LIMIT + 1` index entries, not 8,145. The `(scheduled, train_uid,
origin_crs)` tuple comparison is a keyset cursor that rides the PK's own
ordering, so "Load more" is free rather than an `OFFSET` re-scan.

This is the *spirit* of the brief's third candidate — let `api` answer the
query with an indexed SQL query against a table it already holds locally,
rather than a resident in-memory index — executed against a table of the
right shape. §3.1 below explains why the existing `schedule_line_population`
cannot be that table.

**Sizing.** ~377,000 rows/day (§1.2). At roughly 40 bytes of payload plus
Postgres' ~24-byte tuple header, alignment and the PK index, call it
~40-60MB of table+index per service date. That is a rounding error for
Postgres to *store* and to *query*, but it is a real number to *accumulate*
— see Retention below.

**The publish payload** is one array of ~377,000 flat
`{destination_crs, origin_crs, scheduled, train_uid}` objects ≈ **~21 MB**,
which `common::ingest::post_batch` (`crates/common/src/ingest.rs:97-120`, a
single unchunked `client.post(url).json(items)`) sends as one body, under
`DefaultBodyLimit::max(100 * 1024 * 1024)` with ~4.7x headroom.

**Escape hatch if the revised measurement comes in high.** `post_batch`
takes `&[T]`, so `for chunk in rows.chunks(50_000) { post_batch(…, chunk, …) }`
chunks the publish with **zero new shared code**. The cost is that the
per-`service_date` atomic replace weakens to "converges once all chunks
land", which needs the ingest handler to learn "the first chunk of a
publish clears the day" semantics. That is why chunking is the documented
fallback rather than the default — §2's revised Task 1 decides between them
with one number.

**The ingest write must not copy its sibling's shape.**
`upsert_schedule_network_departures` (`crates/api/src/data/queries.rs:843-871`)
loops one `INSERT … ON CONFLICT` per row inside a transaction — correct for
~2,500 rows, and ~377,000 round trips here. Use instead, in one
transaction: `DELETE FROM schedule_destination_departures WHERE service_date
= $1`, then a single `INSERT … SELECT * FROM UNNEST($1::text[], $2::text[],
$3::time[], $4::text[])`. Both are plain runtime-checked `sqlx::query`,
within the plan's global constraint against `query!` macros, and the pair
preserves the "wholesale replace, never merged" posture both existing
CIF-derived products document
(`crates/api/src/data/queries.rs:771-776`, `:837-842`).

**Retention becomes required, reversing the plan's own resolution.** The
plan resolved the design doc's Open Question 4 by inspection: the sibling
table has no pruning job, so the new one gets none either (plan,
"Decisions this plan resolves", item 3, and Task 2's migration header
comment). That reasoning was sound for a ~2,500-row table and does not
survive a ~377,000-row-per-day one accruing forever. Add
`prune_schedule_destination_departures(pool, retention_days)` to
`crates/aggregator/src/queries.rs`, modelled directly on `prune_history`
(`:487-496`) and `prune_trust_event_backlog` (`:503-511`), deleting
`WHERE service_date < CURRENT_DATE - $1`. Nothing reads a past date — every
read computes `today` server-side — so a retention of 1-2 days is enough.

**The 404-versus-`200 []` split needs one explicit decision.** The JSONB
shape distinguished "no row published for `(destination, today)`" (→ 404)
from "row published, filters matched nothing" (→ `200 []`) for free; that is
the split `get_station_schedule_departures` already draws
(`crates/api/src/routes/departures.rs:66-70`). A flat table loses it: an
empty result set is empty either way. Restore it with a cheap existence
probe scoped to the day, not the destination:

```sql
SELECT 1 FROM schedule_destination_departures WHERE service_date = $1 LIMIT 1
```

— one indexed lookup. **This deliberately changes the semantics**: 404 now
means "no CIF publish has landed for today at all," and an unknown or
train-less destination CRS returns `200 []` rather than 404. That is
arguably the more honest split of the two — "we have today's timetable and
nothing goes there" and "we don't have today's timetable" are genuinely
different answers — but it *is* a divergence from the sibling route's
behaviour and should be a conscious call, not a side effect (§6, Open
Question 3).

**Tradeoffs, stated plainly.**

- *Cost:* a new pruning job the original plan did not have; an `UNNEST`-based
  bulk insert instead of a copied per-row loop; ~40-60MB/day of table churn
  and its WAL, once per delivery.
- *Cost:* the read route gains a `limit` and a cursor, and the frontend
  gains a "Load more" control (§2, Task 10).
- *Benefit:* no cap anywhere, so nothing is silently truncated and no gap
  needs a caption.
- *Benefit:* `now` is evaluated at request time, so the result is correct at
  18:00 and not only at delivery-time-plus-two-hours — and the approach does
  not inherit §1.3's staleness property.
- *Benefit:* the query is a bounded index range scan regardless of how busy
  the destination is; Waterloo costs the same as Bootle Oriel Road.
- *Benefit:* both §6 constraints the brief reopened stay intact (§2).

### 3.1 Three further candidates investigated and rejected on evidence

**Reusing `schedule_line_population`.** A false lead, on coverage grounds,
before shape even matters. That table is keyed by `line_id`
(`crates/api/migrations/20260904090000_schedule_line_population.sql:16-22`)
and is populated only for `lines_to_publish(&config.lines)`
(`crates/schedule-reference/src/main.rs:238`) over the static `lines/`
catalogue — **110 files**, verified by direct count, which the whole-network
research doc counts as naming only **267 distinct TIPLOCs**, against the
~2,500-station `stanox_crs` coverage the CIF fallback already achieves. A
destination search built on it would cover roughly a tenth of the network,
which is the exact regression the design doc's §3 rejected Approach C for.
Its shape is also wrong independently: `population` is a nested JSONB blob
of `LinePopulationEntry` with an inner `calling_points` array, relayed
completely unprocessed precisely *because* it is nested and awkward to
project (`crates/api/src/routes/lines.rs:128-160`), so filtering by
destination would mean a nested lateral or `jsonb_path` query over 110 blobs
with no usable index.

The candidate's underlying instinct was right, though, and Approach C is
that instinct done properly: a well-indexed SQL query over a Postgres table
`api` already holds is categorically different from a resident in-memory
index, and it is the reason the design doc's "no resident index" exclusion
does not need relaxing.

**Bucketing by `(origin_crs, destination_crs)` pairs.** Does not reduce the
total volume at all: the sum over pairs equals the sum over destinations
equals the total departure-bearing calling-point count (§1.2's identity),
because pair-keying redistributes the same entries into many more, smaller
buckets rather than removing any. It buys a smaller *maximum bucket* at the
cost of a much larger *bucket count* — and, decisively, it forces the user
to supply an origin up front, which is exactly the capability the feature
exists to add (design doc §3: *"Scope Approach B's v1 to Destination-first
search only — Origin remains… optional, not required"*). Approach C's PK
already gives the pair-scoped query as a **narrowing** of the same index
scan (`AND origin_crs = $4`), with origin optional. Strictly better on both
axes.

**One-row-per-schedule with array containment** (a `departure_crs TEXT[]`
column and a GIN index, ~25,305 rows/day instead of ~377,000) is
attractive on row count and was taken seriously. It fails on ordering: the
route must sort and paginate by *the scheduled departure time from the
origin the user picked*, which is a per-element value a containment index
cannot order by — so every candidate row would still be array-expanded at
query time, then sorted. A GIN prefilter plus a lateral expansion plus a
sort is strictly more machinery than a plain btree range scan, for a table
that is only ~377,000 rows. Rejected on that basis, not on principle.

## 4. Recommendation

**Approach C.** Reject A and B.

The reported blocker is real in its consequence — the plan's Task 4 cannot
be dispatched as written, and no cap value is defensible — but its stated
magnitude is an artifact of a formula that multiplies a worst-case bucket by
every destination (§1.2). Corrected, the whole network's uncapped, whole-day
destination-keyed data is ~20MB and ~377,000 rows: comfortably a single POST
and trivially a Postgres table. The genuine problem is not volume. It is
that a **JSONB-bucket-per-destination shape forces a cap**, and — because
the publish fires once per delivery rather than once per cycle (§1.3) — any
cap silently truncates the busiest destinations to the first couple of hours
after the daily timetable lands, in a way that no honest UI caption can
describe.

So change the shape rather than the cap. Store one row per departure, index
it on `(service_date, destination_crs, scheduled, train_uid, origin_crs)`,
publish the whole day uncapped, and move the `now`-forward filter and the
pagination to the read, where they are an index range scan with a keyset
cursor. Pagination *is* the right answer, as the plan's escape hatch
guessed; it just belongs on the read side, where it costs a `LIMIT` and a
button, rather than on the publish side, where it costs the user their
results.

Both §6 constraints the brief reopened stay intact: no resident index, no
synchronous cross-service call at request time.

## 5. What this does to the implementation plan's 12 tasks

| Task | Disposition | What changes |
|---|---|---|
| **1** — cap/cardinality diagnostic | **Changed, re-scoped** | No longer picks a cap (there is none). Now measures `Σ_d bucket_d` — the network-wide total of `now`-forward departure-bearing calling points for a whole day from `00:00`, which is the **sum** of the third column of the same output whose max was already reported. Multiply by ~55 bytes. Gate: if the single POST body exceeds ~60MB, adopt §3's chunked-publish fallback (and Task 6 grows "first chunk clears the day" semantics) — but **do not stop**; the escape hatch has now been exercised and this document is its output. Still controller-run, still no commit. |
| **2** — migration | **Changed** | Same file slot, different DDL: the five-column flat table with the composite PK from §3, no `departures JSONB`. Its header comment's *"RETENTION: none, deliberately"* paragraph is now wrong and must be rewritten to point at the new pruning task. |
| **3** — `departures_by_destination_crs` | **Kept, one correction** | The function signature, the `DestinationDeparture` record and all seven tests survive verbatim — the function is shape-agnostic (it returns an uncapped, unsorted `HashMap<String, Vec<DestinationDeparture>>` and its caller decides everything else). The one correction: its doc comment's *"The caller caps each bucket (see `MAX_DEPARTURES_PER_DESTINATION`)"* sentence is false under this addendum and must cite this document instead. The deliberate drop-vs-degrade asymmetry for an unresolved destination TIPLOC is unaffected and still right. |
| **4** — `schedule-reference` publish | **Changed** | `MAX_DEPARTURES_PER_DESTINATION` is **deleted**, not re-valued. `schedule_destination_departures_rows` becomes a flatten — one JSON object per departure, each carrying its own `destination_crs` — instead of sort+truncate+group. Its two cap tests become meaningless and go; the third becomes "produces one row per departure carrying its destination". The `now` argument passed to `departures_by_destination_crs` changes from `london_local_time_now()` to `NaiveTime::MIN`: publish the whole day, filter at read time (§1.3). Steps 4, 5 and 6 (the `publish_cif_derived_products` wiring, the config field, the Helm env var) are **unaffected**. **This task is no longer gated on Task 1** — no constant depends on it; Task 1 now only chooses one POST versus chunked. |
| **5** — `api` data layer | **Changed** | `ScheduleDestinationDeparturesRow` becomes four flat scalar fields, no `serde_json::Value`. The upsert becomes `DELETE` by `service_date` + `UNNEST` bulk insert in one transaction, not a per-row loop. The search becomes §3's indexed range scan with a keyset cursor, not `LEFT JOIN LATERAL jsonb_array_elements`; its signature gains `after` and returns a shape that preserves the 404/`200 []` split via the day-scoped existence probe. Its `db_tests` are rewritten around the new shape; the `ZRB`-`ZRF` fixture CRS reservation still holds. |
| **6** — `api` ingest route | **Kept, one conditional change** | Route path, method, `internal_oauth_group_schedule_reference` authorization, `UpsertResponse`, and the `app.rs` registration are all unchanged. Only the body type follows Task 5's new row struct, with its `db_tests` seed/assert. *If and only if* Task 1 forces chunking, this handler also gains "the first chunk of a publish clears the day" semantics. |
| **7** — `GET /public/trains/search` | **Changed additively** | The route path, its required `destination` and optional `origin`/`from`/`to` parameters, the camelCase wire shape `{uid, scheduled: "HH:MM", originCrs, destinationCrs}`, `render.rs`'s `destination_departure_json` and **both of its render tests survive verbatim**. Two additions: a `limit` (default 50, max 200) and an opaque `after` cursor; and consequently the response body becomes `{results: [...], nextCursor: string | null}` rather than a bare JSON array. That envelope change is the only thing that ripples to the frontend. |
| **8** — `create_subscription_for_train` idempotency | **Unaffected** | Confirmed explicitly: this task concerns `train_subscriptions`, `crates/api/src/data/train_tracking.rs`, and a double-click on a CTA. It shares no table, no query, no constant and no constraint with the sizing question. Its evidence (the function's own doc comment at `train_tracking.rs:162-169`, the `assert_ne!` live test at `:3506-3556`, the dropped `tracked_trains_resolved_identity` index) and its rejection of a `(user_id, trains_id)` unique index are all untouched. Keep exactly as written, **including its ordering before Tasks 9-12**. |
| **9** — `TrackThisTrainButton` | **Unaffected** | Confirmed explicitly: it consumes `POST /Train/by-uid/{uid}/{date}/track` and `POST /Train/tickets/{id}/attach`, neither of which this addendum touches. Its props, its `attachTicketId` split between the two call sites, and its tests all stand. |
| **10** — `TrainSearchForm` | **Changed, minimally, frontend-only** | Must read `{results, nextCursor}` instead of a bare array, and render a "Load more" control that re-requests with `after=`. Everything else stands: both `Autocomplete`s, `searchStations`/`useSuggestions`, the row rendering, the `/train/{uid}/{today}` link, and `TrackThisTrainButton` with `attachTicketId`. This is where the pagination burden honestly lands, and it costs a button. |
| **11** — the `/trains` page and nav entry | **Unaffected** | Confirmed explicitly: the page, its `?destination=`/`?origin=`/`?ticketId=` `searchParams` reading, the `layout.tsx` nav link and both test files are all independent of the backing storage shape. |
| **12** — the `/train/[uid]/[date]` CTA | **Unaffected** | Confirmed explicitly: the CTA, its placement, and its explicit no-`attachTicketId` rule are untouched. |
| **NEW** — `prune_schedule_destination_departures` | **Added** | Genuinely new work the original plan did not have, existing solely because §3 turns a ~2,500-row table into a ~377,000-row-per-day one. Add to `crates/aggregator/src/queries.rs` modelled on `prune_history` (`:487-496`), with a config field for the retention (1-2 days), wiring into the aggregator's existing prune cycle, and a `db_tests` test mirroring `prune_trust_event_backlog_deletes_only_rows_older_than_the_retention_window` (`:3106`). Ordering: after Task 2, independent of everything else. |

Net effect on the plan, with every one of the 12 accounted for: **four
changed outright (1, 2, 4, 5); one changed additively on the backend (7) and
one minimally on the frontend (10); two kept, each with a single corrected
or conditional clause (3, 6); four confirmed entirely unaffected (8, 9, 11,
12); one new task added.** The plan's Global Constraints block is unchanged
in every particular, and its ordering constraint that Task 8 precede Tasks
9-12 is unchanged.

## 6. Explicitly out of scope

- **Everything in the original design doc's §6 remains binding**, unchanged
  and unrelitigated: no operator/`BX`/headcode decoding, no date other than
  today, no resident whole-network in-memory index, no synchronous
  request-time `api` → `schedule-reference` call, no broadening of
  `poller-ldbws`'s ~286-station set, no whole-network LDBWS destination
  search, no merging of LDBWS and CIF rows, no `ticketId` convention on
  `/train/[uid]/[date]`, no pixel-level UI design.
- **Re-opening Approach A or Approach C of the original design doc.** The
  product decision to build a destination-first whole-network CIF search
  stands; only its storage mechanism is replaced.
- **Any change to `schedule_network_departures`**, its migration, its
  `MAX_DEPARTURES_PER_STATION = 10` cap, `GET
  /public/stations/{crs}/schedule-departures`, or `TrackTrainForm`'s use of
  either. They are untouched.
- **Fixing the once-per-delivery staleness of `schedule_network_departures`'s
  own `now`-forward window** (§1.3). It is named here because it was found
  while grounding this document and because it explains why a capped
  destination bucket fails; fixing it is a separate call on a shipped
  feature, and the recommended approach avoids inheriting it rather than
  repairing it.
- **Consolidating the two CIF-derived departure tables.** The new flat table
  would, uncapped, contain a superset of what `schedule_network_departures`
  holds, so deriving one from the other is plausible. It is a separate
  decision about retiring a shipped table with a live reader (§7, item 6).
- **A `COPY`-based or extension-based bulk loader.** An `UNNEST` insert is
  sufficient at ~377,000 rows and stays inside the crate's
  runtime-checked-`sqlx`-only rule.
- **Partitioning `schedule_destination_departures` by `service_date`.**
  Named as the standard mitigation if §7 item 5 turns out to bite; not
  designed here.
- **The visual treatment of "Load more."** A control that re-requests with
  the cursor and appends; anything beyond that is implementation/design
  review, exactly as the original design doc's §6 says of the rest of the
  page.
- **Task 8's scope.** Untouched, in either direction.

## 7. Open questions / risks

1. **`Σ_d bucket_d` is derived, not measured.** §1.2's ~377,000 is
   arithmetic on Task 1's own two measured numbers, using Waterloo's
   above-average 14.9 stops per schedule as a deliberately conservative
   stand-in for the network mean. The revised Task 1 must produce the real
   sum before Task 2's migration is written. If it lands materially above
   ~1,000,000 entries the single-POST default flips to chunked, and Task 6
   grows the "first chunk clears the day" semantics — a bounded, already-named
   consequence, not a new blocker.
2. **Is `NaiveTime::MIN` the right publish-time `now`?** Publishing the whole
   day makes CIF's midnight-crossing schedules more visible than the current
   filtered publish does, because early-hours calling points are no longer
   dropped. The underlying model — `booked_departure` as a bare `NaiveTime`
   compared against a same-day `now` — is inherited unchanged from
   `departures_by_crs` (`crates/schedule-query/src/resolve.rs:190-195`) and
   is not made worse here, but it is made more observable. Named, not solved.
3. **The 404 semantics change (§3).** Under the flat shape, 404 means "no
   CIF publish has landed for today at all" and an unknown destination CRS
   returns `200 []`, which diverges from `get_station_schedule_departures`'s
   split. This document argues the new split is the more honest of the two;
   it should be an explicit approval, not an inferred one.
4. **Retention default.** 1 day matches `trust_event_backlog`'s, but that
   default exists for a licensing reason that does not apply here; 2 days is
   safer around the rail-day/midnight boundary and around a delivery that
   lands late. A human call at implementation-planning time.
5. **Write pressure of a ~377,000-row `DELETE` + `UNNEST INSERT` in one
   transaction, once per delivery, against the production Postgres.**
   Unmeasured. Once a day is a forgiving cadence and the table has no other
   readers mid-write, but the WAL volume and post-delete bloat are real. The
   standard mitigation is `service_date` partitioning with a partition swap;
   that is more machinery than this addendum commits to unprompted.
6. **Should `schedule_network_departures` eventually be derived from this
   table** (a `LIMIT 10` range scan per origin CRS) rather than published
   separately? Plausible, would remove a publish and a table, and is
   explicitly not proposed here — it touches a shipped route with a live
   frontend caller.
7. **Same-day VSTP-style urgent CIF amendments** — inherited unresolved from
   both prior trip-search documents and from the original design doc's own
   Open Question 6. §1.3's finding sharpens it: with a once-per-delivery
   publish, a same-day amendment is not reflected until the next delivery
   lands, whatever the storage shape. Nothing in this addendum makes it
   better or worse.

## References

- `docs/superpowers/specs/2026-09-07-train-listing-page-design.md` — the
  approved design; its §3 Approach B mechanism is what this addendum
  replaces, and its §2 goal, §4, §5 and §6 are what it preserves.
- `docs/superpowers/plans/2026-09-07-train-listing-page-implementation-plan.md`
  — the 12-task plan; Task 1's escape hatch is what triggered this document.
- `docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md` —
  Decision 1, the `schedule_network_departures` precedent this addendum
  deliberately diverges from on storage shape while keeping on publish
  posture.
- `crates/schedule-reference/src/main.rs:91-107` (delivery-change gate),
  `:176-214` (`publish_cif_derived_products`), `:265-329`
  (`publish_schedule_network_departures` and its cap).
- `crates/schedule-reference/src/config.rs:19-25` — the poll interval's own
  "a fresh delivery only lands roughly once a day" doc comment.
- `crates/schedule-query/src/resolve.rs:153-216` — `departures_by_crs`, the
  `now`-forward filter, and the grouping primitive Task 3 mirrors.
- `crates/api/src/data/queries.rs:771-897` — both existing published-product
  read/write pairs.
- `crates/api/src/routes/mod.rs:76-87` — the `DefaultBodyLimit` and the
  measured ~55MB stations feed that set it.
- `crates/common/src/ingest.rs:97-120` — `post_batch`, single unchunked POST.
- `crates/aggregator/src/queries.rs:487-511` — the pruning precedent the new
  task copies.
