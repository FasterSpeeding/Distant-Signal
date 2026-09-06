# Design/Scoping: Persisting `schedule_line_population` (and `schedule_network_departures`) for Future Dates, Not Just "Today"

**Status: scoping/research document, not approved, not implementation-ready.
No migration, no Rust code, no config change in this pass — matching this
repo's own spec-doc convention
(`docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md`,
`docs/superpowers/specs/2026-09-03-schedule-feed-cadence-research.md`).**

Required reading consumed in full before this document was written:
`crates/schedule-reference/src/{main,config,discovery}.rs`;
`crates/schedule-query/src/{resolve,records}.rs`;
`crates/api/migrations/20260904090000_schedule_line_population.sql`,
`20260904110000_schedule_network_departures.sql`;
`crates/api/src/data/{queries,train_tracking,schedule_matching}.rs`;
`crates/full-coverage-consumer/src/main.rs`;
`frontend/components/TrackTrainForm.tsx`;
`docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md`
(the doc that first scoped this gap out, Decision 6);
`docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md`
(the only prior document with real row-size/row-count reasoning for either
of these two tables);
`docs/superpowers/specs/2026-09-03-schedule-feed-cadence-research.md` (cited
for `schedule-reference`'s per-cycle cost characteristics and this repo's
established retention-job precedent);
`docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md` (cited for
this repo's pruning-job style, Task 6).

## Why this document exists

The schedule-first design doc's Decision 6 states the periodic retry sweep
it designed only mechanically fixes a stuck `tracked_trains` row "within
the same/current rail day," not a future-dated one, and names that
limitation explicitly rather than solving it. The reason is structural, not
a missed edge case: `crates/schedule-reference` computes and publishes
`schedule_line_population` for exactly one date per cycle — whatever
`chrono::Utc::now().date_naive()` happens to be at that moment
(`crates/schedule-reference/src/main.rs:202`) — and nothing else. A pin
created today for a service running two weeks from now has no row to match
against until the calendar itself reaches that day and a `schedule-reference`
cycle finally treats it as "today." This document scopes what it would take
to close that gap by persisting a forward window of dates instead of one.

## 1. Current architecture, recapped with exact citations

**The poll loop.** `schedule-reference`'s `main` (`main.rs:23-57`) runs one
`tokio::time::interval` on `config.poll_interval_secs` (default `1800`,
`config.rs:22`) and calls `poll_once` every tick. `poll_once`
(`main.rs:91-164`) does three things in order:

1. `discovery::latest_complete_delivery(&config.storage_dir)`
   (`discovery.rs:40-74`) finds the most-recent delivery directory (by
   lexicographic/chronological directory name) that has both a
   `RJTTF*MCA.txt`- and `RJTTF*MSN.txt`-shaped file. Returns `None` (a
   silent no-op, `main.rs:98-99`) if none exists yet.
2. **Change detection**: if this delivery's directory name equals
   `last_processed_delivery` (an in-memory `Option<String>`, reset only on
   process restart), the cycle returns immediately without doing any work
   (`main.rs:101-107`) — "no new delivery since last successful parse;
   nothing to do." This is the *only* trigger for recomputation in this
   entire service. There is no separate timer, no date-rollover check, and
   no "recompute because a new date needs a row" logic anywhere in this
   file.
3. On a genuinely new delivery: parses `TI`/`A` records for the
   STANOX/CRS table (`main.rs:109-121`), POSTs it
   (`main.rs:144-151`), advances `last_processed_delivery`
   **only on a successful POST** (`main.rs:153-158`), then calls
   `publish_cif_derived_products` (`main.rs:160-161`).

**`ScheduleIndex` build and discard.** `publish_cif_derived_products`
(`main.rs:176-214`) reads `BS`/`BX`/`LO`/`LI`/`CR`/`LT` lines from the same
delivery's MCA file (`main.rs:183-192`) and builds exactly one
`schedule_query::ScheduleIndex::from_text(&mca_schedule_text)`
(`main.rs:194`) — a national, UID-grouped in-memory index
(`crates/schedule-query/src/resolve.rs:221-260`, `ScheduleIndex::build`
groups every parsed `RawSchedule` by `uid` into a `HashMap<String,
Vec<RawSchedule>>`). This index build is the expensive step: it parses the
CIF `SCHEDULE` body, whose real reference sample is 707,743,886 bytes /
8,631,021 lines (`docs/superpowers/specs/2026-09-03-schedule-feed-cadence-research.md:65-67`,
independently re-confirmed there against the still-present
`timetable_full.zip`), of which 488,798 are `BS` records
(`crates/schedule-query/src/records.rs:23`, cited from the validation
findings doc). **This index is built exactly once per cycle, then used for
both downstream publishes, then dropped when `publish_cif_derived_products`
returns** — `schedule_query`'s own module doc (`crates/schedule-query/src/lib.rs`)
already states this crate does no I/O and nothing it builds is ever
persisted by this crate itself.

**The single-date publish.** `let today = chrono::Utc::now().date_naive();`
(`main.rs:202`) is computed once per cycle and passed to both publishers:

- `publish_schedule_line_population` (`main.rs:230-263`) loops over
  `lines_to_publish(&config.lines)` (`main.rs:354-360` — every catalogued
  line with at least one `tiploc`-bearing station; **109** files under
  `lines/*.toml`, confirmed `ls lines/*.toml | wc -l`), calls
  `schedule_query::schedules_touching(&index, &tiplocs, today)`
  (`resolve.rs:77-96`) per line, and POSTs each line's population
  individually (`post_schedule_line_population`, `main.rs:365-374`) to
  `POST /private/schedule-line-population`.
  `queries::upsert_schedule_line_population` (`crates/api/src/data/queries.rs:777-798`)
  does a plain `INSERT ... ON CONFLICT (line_id, service_date) DO UPDATE` —
  **wholesale replace**, per that function's own doc comment
  (`queries.rs:771-776`, "a fresh CIF read supersedes the prior one
  entirely, never merged").
- `publish_schedule_network_departures` (`main.rs:277-310`) does the
  equivalent for `schedule_network_departures`, additionally filtering to
  `booked_departure >= now` (`resolve.rs:174-216`, `departures_by_crs`) and
  capping at `MAX_DEPARTURES_PER_STATION = 10` (`main.rs:273`) per CRS.

**`schedules_touching`/`resolve_for_date` are already fully date-parametric
and correct for any date, past or future**, within a schedule's own
`date_from..=date_to` validity window (`resolve.rs:42-70`,
`resolve_for_date`) — this is not a limitation anywhere in `schedule_query`
itself. The gap is entirely that `schedule-reference`'s `main.rs` only ever
calls these functions with one `NaiveDate` value per cycle.

**One existing precedent for "more than one date," and why it doesn't
already solve this.** `crates/full-coverage-consumer/src/main.rs` already
fetches **two** dates per reload — `service_date` and `service_date + 1`
(`main.rs:267-279`, `reload_population`, "Decision 2b: today's AND
tomorrow's, to avoid a gap at the rail-day rollover boundary,"
`main.rs:125-126`). This looks, at first glance, like exactly the mechanism
this document is scoping — but it is not a real forward-window feature: it
is a **reader-side buffer against a race**, not a writer-side forward
publish. `full-coverage-consumer` computes its own `service_date` from
`chrono::Utc::now().date_naive()` too (`main.rs:102,117`), independently of
`schedule-reference`. Its "tomorrow" fetch exists so that, in the narrow
window right after midnight UTC before `schedule-reference`'s own next
cycle has run (up to `poll_interval_secs` = 1800s later), a population row
for the *new* today already exists if `schedule-reference` happened to
publish it slightly early relative to `full-coverage-consumer`'s own
rollover check. In steady state, before that boundary, `service_date + 1`
almost always resolves to `Ok(None)` — `main.rs:305-309`'s own comment
calls this "Decision 2e's Pending case... not an error." **This mechanism
never asks `schedule-reference` to compute anything for a date further
than one day out, and does not help a pin for next week or next month at
all.** It is real, working evidence that this codebase already tolerates a
"fetch a date that might not exist yet, treat absence as Pending" pattern —
useful precedent for Decision 2 below — but it is not itself the feature
this document scopes.

**How far ahead a user is actually allowed to pin a train today: unbounded.**
Checked directly, both sides:

- Backend: `crates/api/src/data/train_tracking.rs`'s `validate_pin`
  (`train_tracking.rs:50-72`) only rejects a pin whose
  `scheduled_departure` is more than `MAX_PIN_AGE` (6 hours,
  `train_tracking.rs:25`) **in the past**. Its own doc comment
  (`train_tracking.rs:17-24`) states outright: "A pin arbitrarily far in
  the future is fine — 'track before it even starts running' is an
  explicit design goal." There is no `MAX_PIN_FUTURE`-shaped constant
  anywhere in this file, and none was found grepping the crate.
- Frontend: `frontend/components/TrackTrainForm.tsx`'s `DateTimePicker`
  (`TrackTrainForm.tsx:515-527`) has no `maxDate` prop; its own
  `description` text reads "Must be within the last 6 hours, or any time in
  the future" (`TrackTrainForm.tsx:524`).

This matters directly for Dimension 1 below: there is no existing product
constraint this document can lean on to justify a specific window size —
any number chosen (7/14/30 days) is a **new** constraint this document
would be introducing, not one already implied by product behavior
elsewhere.

## 2. What "persisting future dates" would actually require

### Dimension 1 — how far ahead to compute

No existing UI/API bound exists to anchor this (§1, last point) — the
window has to be chosen on cost/benefit grounds, not derived from a
pre-existing limit. Two considerations pull in different directions:

- **User behavior is presumably front-loaded.** Nothing in this codebase
  measures the actual distribution of how far ahead real users create pins
  (no `pin_scheduled_departure - created_at` histogram exists anywhere in
  `crates/aggregator` or `crates/api`'s metrics) — this is a real,
  unmeasured gap, flagged rather than papered over. Absent real data, the
  reasonable prior is that most tracked-train pins are for "today" or "in
  the next few days" (checking a specific commute, a planned trip), with a
  long tail of much-further-ahead pins (the exact bug this whole feature
  chain exists to fix: a pin created for a service many days or weeks out).
- **CIF's own validity windows are genuinely long.** The background brief's
  own cited real decoded record spans `2026-05-17` to `2026-12-06` — nearly
  seven months. A short window (e.g. 3-7 days) would still leave the
  documented failure mode (a pin for "next month") unresolved; a window has
  to be meaningfully longer than "a few days" to actually close the gap
  this document exists to close, or the fix is cosmetic.

**Recommendation for this dimension: a rolling window on the order of
14-30 days**, revisited once real pin-lead-time data exists (a metric this
document names as a prerequisite for confidently narrowing the number, not
something to ship blind). This is deliberately not pinned to a single
number in this scoping pass — see the task breakdown's Task 1, which names
"measure real pin lead time, then choose N" as its own step before the
window size is hard-coded anywhere.

### Dimension 2 — compute cost

**The expensive step (parsing the ~707MB MCA file into `ScheduleIndex`) is
NOT multiplied by the number of future dates published.** This is the most
important, concretely-grounded finding in this section: `ScheduleIndex::from_text`
(`main.rs:194`) is called exactly once per cycle, before either publisher
runs, and produces one in-memory index that both publishers already share
(`main.rs:204-213`, both calls take `&index` — this sharing was itself the
whole point of Task 3 of the whole-network-trip-search plan, per
`main.rs:166-175`'s own doc comment). Extending to N future dates does not
mean re-parsing the CIF file N times.

**What genuinely does scale with N is the resolve+serialize+POST step.**
`schedules_touching` (`resolve.rs:77-96`) does one `O(all UIDs in index)`
resolve pass per `(line, date)` call — for each of the ~109 catalogued
lines, once per date. Going from 1 date to N means this loop runs
`109 * N` times per cycle instead of `109` times, each iteration paying the
same `O(total UIDs)` cost it already pays today. `departures_by_crs`
(`resolve.rs:174-216`) is structured the same way, once per date rather
than once per (CRS, date) — already the efficient shape per the
whole-network-trip-search design doc's own §4 analysis (cited below). This
resolve cost is real but bounded and already-characterized: that same
document confirms the *existing* single-date, whole-network
`departures_by_crs` pass is "the same complexity class `schedule-reference`
already pays once per cycle for its `schedules_touching`-based line
publish, not a new one" (whole-network-trip-search-design.md:445-453). This
document has no reason to believe an N-times repetition of an already-cheap
pass becomes a real bottleneck at N=14-30 — but this is reasoned from the
existing design doc's own analysis, not independently benchmarked in this
pass; a real timing measurement (`schedule_reference_cycle_duration_seconds`,
already emitted per cycle at `main.rs:49-52`) before and after any window
expansion is cheap to obtain and should gate the actual window size chosen,
not be skipped.

**Can it be made incremental instead of recomputed fully every cycle?**
In principle yes — the background brief's own framing ("schedules rarely
change day-to-day except around STP overlays") is directionally correct:
`resolve_for_date`'s own STP-precedence logic (`resolve.rs:42-70`) means a
`P`(Permanent)-indicator schedule's resolution for a far-future date is
almost always stable across cycles unless a new `O`/`N`/`C` overlay record
for that specific date is introduced by a later delivery. But this
codebase has **no existing per-date diffing mechanism to build on** —
`schedule-reference` is deliberately, structurally a
"parse everything from scratch every cycle, never diff against a prior
state" service (the schedule-feed-cadence-research doc's own §1 finding,
quoting the STANOX/CRS design doc: "every delivery's `TI`/`A` extraction is
a complete, standalone, from-scratch snapshot — never a merge against a
prior day's partial state," restated for the CIF `SCHEDULE` body itself at
`main.rs:194`'s full from-scratch rebuild). Building true incrementality
(track which dates' resolved output actually changed since last cycle, skip
re-publishing unchanged ones) would mean retrofitting exactly the kind of
stateful-diff machinery the cadence-research doc already argued against
adopting for a different reason (daily-update deltas) — real, working
precedent that this codebase's own architecture leans away from that
complexity class. **Given Dimension 2's own finding that the resolve pass
is already cheap and bounded, incremental recomputation is not recommended
as a first cut** — a full recompute of the whole window every cycle is
simpler, already fits this crate's existing all-from-scratch posture, and
the cost this section already bounds does not obviously need to be paid
down further. Flagged as a possible later optimization if real
`schedule_reference_cycle_duration_seconds` measurements after shipping a
plain N-day window show it's actually needed — not before.

### Dimension 3 — storage cost

No prior document in this repo has measured a real, single
`schedule_line_population` row's byte size — this is an honest, named gap,
not an invented number. What does exist, and is directly relevant:

- **Row count today**: ~109 rows (one per catalogued line) per `service_date`
  currently retained (only "today"'s row persists in practice, since
  nothing ever writes a second date and nothing prunes the first — see
  Dimension 6). Going to an N-day rolling window multiplies this to
  `109 * N` rows resident at once — e.g. ~1,526 rows at N=14, ~3,270 at
  N=30.
- **The only real per-row size reasoning anywhere in this codebase**
  compares `schedule_line_population` unfavorably, not favorably, to its
  sibling table: the whole-network-trip-search design doc states plainly
  that "a `schedule_line_population` row stores every UID touching a line,
  each with its full, uncapped `calling_points` array — a busy line's row
  can be large," contrasted with `schedule_network_departures`'s
  deliberately-capped ~700 bytes-1KB/row
  (whole-network-trip-search-design.md:468-475). That same document's own
  row-count comparison point is useful context here: `schedule-reference`
  already fully rewrites `stanox_crs`, a ~3,100-row table, every cycle
  (whole-network-trip-search-design.md:457-461) — so `109 * 30 ≈ 3,270`
  rows in one table is not, by row-count alone, outside what this service
  already handles comfortably every 30 minutes. **The real unknown is
  bytes-per-row, not row-count** — a genuinely busy line (WCML, ECML) with
  a full day's uncapped calling-point list, times N days, is the honest
  unknown this document cannot close without a real measurement (e.g.
  `SELECT line_id, service_date, pg_column_size(population) FROM
  schedule_line_population ORDER BY pg_column_size(population) DESC LIMIT
  10` against a live delivery once this ships even at N=1, before
  committing to N=30). Recommended as the very first implementation task
  (Task 1 in the breakdown below), not deferred to after the window is
  already live at full size.
- `schedule_network_departures`, if extended the same way, is much safer by
  construction: its ~700 bytes-1KB/row figure (already cited above) and its
  cap of `MAX_DEPARTURES_PER_STATION = 10` per station
  (`main.rs:273`) mean an N-day extension there is bounded at
  `~2,500 * N` rows at a known, small per-row size — e.g. `2,500 * 30 =
  75,000` rows at roughly 1KB each ≈ 75MB, comfortably small. **This
  document's storage-risk concern is concentrated entirely on
  `schedule_line_population`, not `schedule_network_departures`.**

### Dimension 4 — staleness/correctness (the part this document will not hand-wave)

This is the dimension the background brief specifically calls out as
needing a real answer, not a gesture. The concrete failure mode: a user
pins a train for 3 weeks out on day 1; `schedule-reference` computes and
persists a row for that future `service_date` on day 1, using whatever CIF
data that day's delivery contains for it; a VSTP overlay or short-term-plan
change is issued by Network Rail on day 10, altering that same service for
that same future date; `schedule-reference`'s day-10 delivery *does*
contain the corrected schedule (per the background brief's own framing —
the daily full-refresh always contains the current full state of every
forward-dated validity window it covers) — **but nothing in the proposed
design would cause day 10's cycle to re-publish a row for that already-far-out
date unless the day-10 date is still inside whatever window is being
recomputed that cycle.**

**Does the existing change-detection (`main.rs:101-107`) already give a
natural trigger?** Partially, and this is worth being precise about:
`last_processed_delivery`'s early-return check answers "is there a new
delivery to process at all" — a coarse, whole-delivery-level gate, not a
per-date or per-line one. Once a genuinely new delivery is detected, the
existing code already re-derives `ScheduleIndex` from scratch and
re-resolves whatever dates it's told to (currently just `today`) — meaning
**every date recomputed on any given cycle is automatically re-derived from
the newest available CIF data, with no explicit "did this date's content
change" check needed at all.** The correctness property this document
needs is not "detect that day 10's overlay changed something" — it's
simpler: **as long as every date in the rolling window is recomputed from
the newest delivery on every single cycle** (not just newly-entering-the-window
dates), any correction that lands in a later delivery is picked up
automatically, the same way `today`'s row already benefits from this
today. **The one thing that must NOT happen: computing a future date's row
once, when it first enters the window, and then leaving it alone until the
window slides it out.** That would silently miss exactly the VSTP/STP-overlay
correction case the background brief is worried about. This has a direct,
concrete design consequence for Decision-space §3 below: **the "fixed N-day
rolling window, recomputed in full every cycle" approach gets this
correctness property for free, with no separate invalidation logic
required** — every date in the window is unconditionally recomputed from
the latest `ScheduleIndex` on every cycle that processes a new delivery,
exactly like `today` is today. An on-demand/cache-on-first-use approach
(considered in §3) does **not** get this for free — it would need its own
explicit invalidation trigger (e.g. "re-resolve any cached future date
whenever a new delivery is processed," which is really the same
"recompute everything, every cycle" work, just deferred and more complex to
reason about) or it silently risks serving a stale cached row past a
correction. This is the single strongest argument this document has for
preferring the fixed-window approach over the on-demand one (see Decision
6).

**One remaining honest limitation, independent of which implementation
approach is chosen**: if a correction lands for a date that has *already
been pruned* (Dimension 6) or is *outside* whatever window is currently
kept, it is simply never seen — no different from today's single-date
behavior, and out of scope to solve further here (a schedule correction for
a date nobody's tracking yet, further out than the chosen window, has no
consumer to serve anyway).

### Dimension 5 — migration/schema impact

**The existing `(line_id, service_date)` primary key already supports this
cleanly — no schema change is required to store more dates.** Confirmed
directly:

- `schedule_line_population`'s migration (`20260904090000_schedule_line_population.sql:16-22`)
  already keys on `(line_id, service_date)`, not just `line_id`; storing a
  second, third, ... Nth `service_date` row per line is exactly what this
  key already allows — `upsert_schedule_line_population`'s `ON CONFLICT
  (line_id, service_date) DO UPDATE` (`queries.rs:783-796`) already handles
  "insert if new, replace if this exact date already has a row" correctly
  for any date, not just today's.
- `get_schedule_line_population(pool, line_id, service_date)`
  (`queries.rs:805-821`) already takes `service_date` as a real parameter,
  not a hardcoded "latest" lookup — every caller (`attempt_schedule_match`,
  `schedule_matching.rs:113`; `crates/api/src/routes/lines.rs:174`; the
  ingest route's own GET handler, `crates/api/src/routes/ingest.rs:383-391`)
  already passes a specific date through. **No caller anywhere assumes
  "only one date row exists at a time"** — every read site already scopes
  by the exact date it needs. This was directly checked by grepping every
  call site of `get_schedule_line_population` (five real, non-test call
  sites; listed above).
- `schedule_network_departures`'s migration
  (`20260904110000_schedule_network_departures.sql:19-25`) is the identical
  shape, `(crs, service_date)` — same conclusion applies.

**What genuinely is new, schema-wise, is the write side, not the read
side**: today, exactly one date's worth of rows gets written per cycle
(109 upserts). Under an N-day window, `publish_schedule_line_population`
would need to loop over N dates as well as 109 lines (`109 * N` upserts per
cycle instead of 109) — a change to `schedule-reference`'s own publish loop
(`main.rs:230-263`), not to the schema or to `api`'s query layer. No
migration file is needed for this feature at all, beyond whatever pruning
job's own bookkeeping needs (Dimension 6) — which, per that section, also
needs no new table.

### Dimension 6 — retention/pruning

**No pruning job exists for this table today, and none is needed at N=1**
(a wholesale-replace on the same PK means yesterday's row for "today" is
gone the instant `service_date` itself moves forward and nothing new is
ever inserted for the old date — the table simply never accumulates past
rows today because nothing ever writes a second date to begin with). **This
changes completely once N > 1**: with a rolling window, a date that has
now scrolled past the window's trailing edge (e.g., a date that's now
`service_date - 1` relative to a 14-day-ahead window) needs to be actively
deleted, or the table grows without bound as the window slides forward
every day, forever.

**This repo already has an established pruning-job pattern to reuse, not
invent from scratch** — the background brief's own hint
(`prune_trust_event_backlog`) is a real, directly-applicable precedent:
`crates/aggregator/src/queries.rs:503-511` is a plain `DELETE FROM
trust_event_backlog WHERE received_at < NOW() - ($1 || ' days')::interval`,
parameterized by a `retention_days` config value, called once per
`aggregator` cycle (`crates/aggregator/src/main.rs:249-260`) alongside
several structurally identical siblings in the same file —
`prune_history` (`queries.rs:487-495`), `prune_daily_stats`
(`queries.rs:641-649`), `prune_half_hourly_stats` (`queries.rs:734-742`),
`prune_daily_coverage_stats` (`queries.rs:829-837`),
`prune_half_hourly_coverage_stats` (`queries.rs:896-904`). Every one of
these follows the identical shape: one `DELETE ... WHERE <time column> <
NOW() - interval`, parameterized by a retention count, run once per
scheduled cycle, count of rows pruned logged/metriced. `schedule_line_population`
would need the exact same shape, just keyed on `service_date < CURRENT_DATE`
(the trailing edge, not a rolling "N days old" retention count the way the
other prune jobs use, since this table's natural retention boundary is
"the rail day has already passed," not an arbitrary day count) —
`DELETE FROM schedule_line_population WHERE service_date < CURRENT_DATE`.
Whether this job lives in `schedule-reference` itself (natural, since it
already owns the writes and already runs on a timer) or in `aggregator`
(where every other prune job in this codebase currently lives) is a real,
open implementation choice — `aggregator`'s existing pattern argues for
consistency/one-place-to-look; `schedule-reference` owning it argues for
locality (the writer prunes its own trailing edge, no cross-crate DB access
needed for aggregator to reach into a table it doesn't otherwise touch).
This document does not resolve which service owns it (see task breakdown).

## 3. At least two concrete implementation approaches

### Approach A: fixed N-day rolling window, recomputed in full every cycle

Every cycle that processes a new delivery, `publish_schedule_line_population`
(and, if extended, `publish_schedule_network_departures`) loops over
`today..=today+N` instead of just `today`, calling `schedules_touching`
once per `(line, date)` pair as it already does for the single date today.
A companion prune step (Dimension 6) deletes any row with
`service_date < today` after each cycle (or on its own timer).

**Pros:**
- Gets Dimension 4's correctness property for free — every date in the
  window is unconditionally re-derived from the latest `ScheduleIndex` on
  every processed delivery, so a VSTP/overlay correction for a
  still-in-window future date is picked up automatically, with zero
  additional invalidation logic (§ Dimension 4, above).
- Minimal new code: `main.rs`'s existing per-line/per-date loop already has
  the right shape (`schedules_touching(&index, &tiplocs, today)` already
  takes a `NaiveDate` parameter) — extending it to iterate a date range is
  a small, mechanical change, not new architecture.
- Every future-dated pin, from the moment it's created, has a real chance
  of being schedule-matched at creation time or on the very next periodic
  sweep — no special-cased "is this pin's date already covered" branching
  needed in `attempt_schedule_match` itself (Dimension 5 already confirmed
  every read call site is already date-scoped correctly).
- Matches this crate's own existing architectural posture exactly
  ("everything recomputed from scratch every cycle, nothing diffed against
  prior state" — the same posture the cadence-research doc already
  identified and endorsed for this crate).

**Cons:**
- Multiplies the per-cycle resolve+serialize+POST cost by N (Dimension 2)
  — bounded and likely cheap based on existing reasoning, but not
  independently benchmarked in this pass; real measurement recommended
  before committing to a specific N.
- Multiplies row count by N (Dimension 3) — the byte-size unknown flagged
  there is the real open risk, not row count itself.
- A pin for a date *beyond* the window (further out than N days) is no
  better off than today — still has to wait for the window to slide far
  enough to reach it. This is a real, honest limit of this approach, not a
  flaw unique to it (every approach considered here shares some version of
  this limit unless N is unboundedly large, which Dimension 3's storage
  risk argues against).

### Approach B: on-demand compute-and-cache, triggered by the first pin that needs a future date

Instead of proactively computing every future date every cycle,
`schedule-reference` keeps publishing only `today` as it does now. When
`attempt_schedule_match` (`schedule_matching.rs:94-155`) is asked to match
a pin whose `service_date` has no `schedule_line_population` row yet, `api`
(or a new small service) would need to compute that date's population
itself, on demand, then cache/store it the same way. This requires `api`
to gain the ability to build a `ScheduleIndex` (or ask `schedule-reference`
to do so via a new synchronous "compute this date now" endpoint) — real new
machinery neither crate has today.

**Pros:**
- No wasted computation for dates nobody has actually pinned yet — if real
  pin-lead-time data (Dimension 1's named gap) eventually shows the long
  tail is rare, this approach never pays for it.
- Naturally self-limiting on storage: only dates someone actually asked
  about ever get a row, rather than every date in a fixed window whether or
  not anyone cares about it.

**Cons — significant, and the reason this document does not recommend it
as the first cut:**
- **Fails Dimension 4's correctness property outright, unless it grows its
  own separate invalidation trigger** — a cached future-date row, once
  computed, has no mechanism to be refreshed when a later delivery corrects
  that date's schedule, unless this approach also builds "re-resolve every
  cached future date on every new delivery" — at which point it has simply
  reinvented Approach A's per-cycle full-window recompute, just triggered
  differently and with more moving parts (a cache layer, a "first use"
  code path in `api`, a cross-crate synchronous compute request) for no
  net simplification.
- **Requires `api` to gain a new dependency direction it does not have
  today**: either `api` links `schedule_query` and holds/rebuilds a
  `ScheduleIndex` itself (duplicating `schedule-reference`'s own
  responsibility, and requiring `api` to also read the CIF delivery files
  from the shared PVC — a real new I/O/deployment dependency `api` has
  never had), or a new synchronous request/response endpoint has to be
  added to `schedule-reference` for "compute this one date right now,"
  which doesn't fit its existing purely-cyclical, POST-only architecture
  (`schedule-reference` has no inbound HTTP server at all today — checked,
  it only ever POSTs out).
- **First-pin latency**: the user whose pin triggers the on-demand compute
  pays for it synchronously (or the pin sits pending until an async
  compute finishes) — a strictly worse first-touch experience than
  Approach A, where the row is very likely to already exist by the time
  any pin needs it.
- Genuinely new architecture, not an extension of an existing pattern —
  the opposite of Approach A's "small, mechanical extension of an
  already-correct per-date loop" character.

### Hybrid considered and rejected: fixed short proactive window + on-demand for the long tail

Briefly considered: proactively compute a short window (e.g. 3-7 days, most
pins) via Approach A's mechanism, and fall back to Approach B's on-demand
path only for the rare pin further out than that. Rejected for this pass:
it combines Approach A's implementation cost with Approach B's genuinely
new architecture and correctness-invalidation burden, for a benefit
(avoiding wasted compute on rarely-pinned far-future dates) that Dimension
2's own analysis suggests isn't a real cost problem at N=14-30 in the first
place. Worth revisiting only if real `schedule_reference_cycle_duration_seconds`
measurements after shipping Approach A show the full window is genuinely
expensive — not a first-cut recommendation.

## 4. Recommendation

**Approach A: a fixed N-day rolling window (N in the 14-30 day range,
narrowed by a real pin-lead-time measurement before being hard-coded),
recomputed in full from the latest `ScheduleIndex` every cycle, with a
same-crate-or-`aggregator` trailing-edge prune job mirroring
`prune_trust_event_backlog`'s exact shape.**

Reasoning, restated from the sections above rather than asserted fresh:

- It is the only approach that gets Dimension 4's staleness/correctness
  property (automatic pickup of later VSTP/STP-overlay corrections for an
  already-published future date) without inventing new, separate
  invalidation machinery — a direct, load-bearing consequence of this
  crate's existing "recompute everything from scratch every cycle" design,
  not a coincidence.
- Dimension 5 already confirms the schema and every existing read call site
  need zero changes to support it — the only real new code is
  `schedule-reference`'s own publish loop (iterate a date range instead of
  one date) and a prune job in the established style.
- Dimension 2's own reasoning (grounded in the existing whole-network-trip-search
  design doc's cost analysis, not invented fresh) suggests the marginal
  per-cycle compute cost is bounded and likely acceptable, though not yet
  independently measured — named as the first real task, not skipped.
- Approach B would be justified only if real usage data showed most pins
  are for dates so far out that proactively computing a wide window is
  genuinely wasteful — no such data exists yet (Dimension 1's named gap),
  and Approach B's own cost (new cross-crate architecture, a
  correctness-invalidation problem it does not solve for free) is high
  enough that it shouldn't be adopted speculatively.

**`schedule_network_departures` should NOT be extended by this same
project.** It already has a `now`-forward filter and a 10-per-station cap
specifically because it was built for a different job (a live "next few
departures" picker, per the schedule-first design doc's own Decision 1)
that a multi-day window doesn't fit — a departure list capped at 10 entries
per station is already exhausted well before a busy terminus's day is over,
let alone N days out. If a forward window is wanted for the trip-search
picker's own use case, that is better scoped as its own follow-up than
folded into this one, since its correctness/cap tradeoffs are different
from `schedule_line_population`'s (this document's own title reflects that
asymmetry: the pin-matching problem is what motivates
`schedule_line_population`'s extension; `schedule_network_departures` is
included in this document's title only because the task brief asked it be
considered, and the finding is that it should not move with the rest of
this work).

## 5. Explicitly out of scope / non-goals

- **A specific final value for N.** Recommended range (14-30 days) given,
  not decided — Dimension 1 names the missing prerequisite (real
  pin-lead-time data) this document does not have.
- **Extending `schedule_network_departures` to a multi-day window.**
  Considered and explicitly rejected above (§4) — a separate, later,
  differently-scoped piece of work if ever pursued.
- **Building real incremental/diff-based recomputation** (only re-resolving
  dates whose underlying schedule actually changed, rather than
  full-window-every-cycle). Considered in Dimension 2, not recommended as
  a first cut — this codebase has no existing diffing infrastructure to
  build on, and the full-recompute cost is not yet shown to need this
  optimization.
- **A migration file or any Rust code.** Per this task's brief, this is a
  scoping document only.
- **Deciding whether the prune job lives in `schedule-reference` or
  `aggregator`.** Named as a real, open choice in Dimension 6/the task
  breakdown, not resolved here.
- **Changing `MAX_PIN_AGE`, `MATCH_TOLERANCE`, or any other existing
  tracked-train-pin constant.** Untouched by this document.
- **Retroactively fixing any specific already-stuck pin.** This document
  describes the general mechanism; any already-stuck row is fixed the same
  way the schedule-first design doc's own periodic sweep already fixes
  same-day stuck rows, once the sweep can see a populated row for that
  pin's date.
- **A UI change communicating "matched N days in advance" or any other
  frontend-visible signal of this feature.** The schedule-first design
  doc's own `resolution_status = 'schedule_matched'` semantics already
  cover the resulting user-visible state; this document does not add a new
  one.
- **Any change to `schedule-ingest`'s own retention (`retention_keep_sequences`)
  or delivery-detection logic.** This document only concerns what
  `schedule-reference` does with a delivery once discovered, not how
  deliveries are discovered or retained on disk.

## 6. Rough task breakdown

Sized to show the scope is estimable, not to serve as the implementation
plan itself (per this repo's plan-follows-design convention, a separate
`docs/superpowers/plans/*.md` document would break these down further with
real code).

1. **Measure real row size before committing to N.** Ship the loop change
   at N=1 first (behaviorally a no-op vs. today) purely to get a real
   `pg_column_size(population)` distribution across all 109 lines for one
   date, closing Dimension 3's named unknown. Cheap, low-risk, and
   directly gates whether N=30 is safe or N=14 (or less) is the ceiling.
2. **Extend `schedule-reference`'s publish loop to an N-day window.**
   `publish_schedule_line_population` (`main.rs:230-263`) iterates
   `today..=today+N` instead of a single `today`; `ScheduleIndex` build
   stays a single per-cycle operation (unchanged, Dimension 2). Emit a new
   metric (rows published per cycle, or reuse/extend the existing cycle
   duration histogram) to directly observe the real Dimension 2 cost this
   document could only bound from prior analysis.
3. **Trailing-edge prune job**, mirroring `prune_trust_event_backlog`'s
   shape (`crates/aggregator/src/queries.rs:503-511`) —
   `DELETE FROM schedule_line_population WHERE service_date < CURRENT_DATE`.
   Decide placement (`schedule-reference` vs. `aggregator`, Dimension 6) as
   part of this task, not before.
4. **Verify `attempt_schedule_match`/the periodic sweep need no code
   change**, only data — re-run the existing `db_tests` in
   `schedule_matching.rs` (`schedule_matching.rs:295-456`) against a
   seeded future-dated row to confirm the already-date-scoped query path
   (Dimension 5) genuinely requires zero changes, closing the loop on that
   section's claim with a real test rather than code inspection alone.
5. **Real pin-lead-time metric** (Dimension 1's named prerequisite) —
   instrument `create_pin` (`train_tracking.rs:79-100`) to record
   `pin_scheduled_departure - now` at creation time, so a future revision
   of this document (or its eventual implementation plan) can replace the
   14-30-day recommendation with a real, data-backed number rather than a
   reasoned guess. Not blocking for Task 2 to ship, but should land in the
   same window so the "narrow N later" promise in §4 is actually
   actionable.
6. **Decide and implement `schedule_network_departures`'s own fate**
   separately, if ever pursued — explicitly NOT bundled into this task
   breakdown per §4/§5's non-goal.

## Open questions this pass could not resolve

1. **Real bytes-per-row for a busy line's `schedule_line_population`
   entry** — the single largest unresolved unknown behind the storage
   recommendation, named as Task 1 precisely because it needs a real
   measurement, not further reasoning from this pass.
2. **Real distribution of how far ahead users actually create tracked-train
   pins** — Dimension 1's named gap; no metric exists today to answer this,
   and the 14-30-day recommendation is a reasoned prior, not measured
   fact.
3. **Whether `schedule-reference`'s per-cycle duration genuinely stays
   acceptable at N=30** under real production data volumes — bounded by
   existing analysis (§ Dimension 2), not independently benchmarked here.
4. **Where the trailing-edge prune job should live** (`schedule-reference`
   vs. `aggregator`) — a real, open choice, not resolved in this pass (§
   Dimension 6, Task 3).
5. **Whether `full-coverage-consumer`'s own "today + tomorrow" fetch
   (`main.rs:267-279`) should be widened once a real forward window exists**
   — out of scope for this document (it already works correctly as a
   narrow rollover buffer today; a wider window existing upstream does not
   obligate this consumer to fetch further ahead unless a concrete need for
   that is identified separately).
