# Design: Schedule-First Resolution for Past-Dated, Never-Tracked Pins

**Status: scoping/research only. No implementation plan, no code in this
pass.**

Companion to
`docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md`
("the schedule-first design doc" throughout) and its implementation,
`crates/api/src/data/schedule_matching.rs`. That doc and its shipped code
(commits `ee4e18c`, `ce77dc0`, `5c2c898`, `c94b67e`, `08e15f7`, `1f7a19c`)
answer "can a pin resolve against CIF schedule data instead of only live
TRUST movements" for pins pinned close to real time. This document asks
the deliberately narrower, differently-shaped question the schedule-first
doc did not need to consider: **can a user create a tracked-train pin for
a journey that already happened — up to ~30 days ago — for a train that
was never live-tracked at the time, and have it resolve via schedule data
alone?** This is a scoping document for a sibling feature to
`2026-09-06-schedule-line-population-future-dates-design.md` (the
FUTURE-dates half of the same underlying gap, written in parallel; not yet
present in the repo as this document was written, so no cross-reference to
its specific decisions could be made — check for it and reconcile
terminology before this document is treated as final).

Required reading consumed in full before this document was written:
`crates/api/src/data/schedule_matching.rs` (including its `db_tests`
module); `crates/api/src/data/train_tracking.rs`; `crates/api/src/data/
queries.rs` (`get_schedule_line_population`/`upsert_schedule_line_
population`); `crates/schedule-query/src/resolve.rs` (`resolve_for_date`,
`schedules_touching`, `match_pin`, `ScheduleIndex`); `crates/schedule-query/
src/records.rs` (`StpIndicator`, real-byte-verified doc comments);
`crates/schedule-query/src/parse.rs`; `crates/schedule-reference/src/
main.rs`; `crates/schedule-ingest/src/{main,config,delivery}.rs`;
`crates/api/migrations/20260904090000_schedule_line_population.sql`,
`20260828120000_train_tracking.sql`; `crates/api/src/routes/train.rs`
(`post_track`, `validate_pin` call site); `frontend/components/
TrackTrainForm.tsx`; and, for the CIF full-refresh real-data claims,
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-
timetable-verification.md` and `2026-09-03-schedule-feed-cadence-
research.md`.

## Summary answer, up front

**A user cannot create a pin for a month-old journey today, full stop —
not a schedule-matching gap, a pin-*creation* gap.**
`crates/api/src/data/train_tracking.rs:61`'s `validate_pin` rejects any
`scheduled_departure` more than `MAX_PIN_AGE` (6 hours,
`train_tracking.rs:25`) in the past, before schedule matching or even a
database insert is ever reached. This single check is the actual blocker
this feature needs to clear, and it is a *bigger* piece of work than it
looks (Decision 1) — not because relaxing a constant is hard, but because
the comment justifying it (`train_tracking.rs:17-24`) was written for a
world where the only resolution path was live TRUST, and a wholesale
removal reopens a UX/data-quality question that world never had to answer:
what happens to a pin that resolves to nothing at all, 30 days later, with
no live data and (per Decision 3, the central finding of this document) no
guaranteed-correct schedule data either.

**Assuming pin creation is allowed, the schedule-matching *machinery*
itself already works unmodified for past dates** — `attempt_schedule_
match` and `schedule_query::resolve_for_date` are both structurally
date-agnostic (Decisions 1-2). **The real, hard constraint is data
availability, and it resolves cleanly into two very different regimes**
(Decision 3, the central finding of this document):

- **Steady state, once this feature has been live for ≥30 days**: every
  past date within the lookback window already has its own
  `schedule_line_population` row, computed and stored *on that day, from
  that day's own live CIF extract* — this is provably as accurate as
  the schedule-first design's live-pin case, because it *is* that case,
  just read back later. `schedule_line_population` has **no pruning job
  anywhere** (confirmed by grep), so nothing deletes it.
- **The bootstrap gap** — the ~30 days immediately after this feature
  ships, plus any day `schedule-reference` failed to process a delivery —
  has **no pre-existing row**, and reconstructing one after the fact from
  *today's* CIF extract is where real, previously-undocumented risk
  lives: a Permanent (`P`) baseline schedule resolves fine for a month-old
  date (its `date_from`/`date_to` window commonly spans many months either
  side, confirmed against real data), but a short-term STP overlay/
  cancellation (`C`/`N`/`O`) that applied specifically on that past date
  is, per the CIF Full Refresh format's own documented purpose (a
  current-and-future snapshot, not a historical archive), plausibly no
  longer present in a later extract once its own validity window has
  fully elapsed — and this repo has never obtained two full extracts far
  enough apart in time to confirm or refute that directly. A resolution
  built this way would look exactly as confident as a correct one and
  carry no signal that it might be wrong.

## Decision-by-decision findings

### 1. Mechanically, does `attempt_schedule_match` / `get_schedule_line_population` care about past vs. future? No — verified directly

`queries::get_schedule_line_population` (`crates/api/src/data/
queries.rs:805-820`) is a bare `SELECT population FROM
schedule_line_population WHERE line_id = $1 AND service_date = $2` — no
`WHERE service_date >= CURRENT_DATE` or any comparable clause. `attempt_
schedule_match` (`crates/api/src/data/schedule_matching.rs:97-146`) calls
it with the pin's own `service_date` verbatim and only branches on whether
a row exists at all (`let Some(json) = ... else { continue; }`,
`schedule_matching.rs:117-118`) — never on how that date relates to
`Utc::now()`. Every "is this pending row eligible" decision in this file
is about whether *data exists*, never about *when* the date is.

This mirrors the schedule-first design doc's own Decision 6 finding almost
exactly (that a stuck pin from the same rail day retroactively resolves
once the periodic sweep runs, "no special-cased backfill script"). This
document generalizes that same observation from "the same day" to "any
day at all" and asks the harder follow-on question the original doc didn't
need to: does the *data* still exist and is it still *correct*, arbitrarily
far back? See Decision 3.

The table's own primary key, `PRIMARY KEY (line_id, service_date)`
(`crates/api/migrations/20260904090000_schedule_line_population.sql`), is
itself evidence this was never designed around "today only" — it is
already a durable, dated archive by construction, one row per line per
calendar day, indistinguishable in shape whether the date is a month ago
or a month from now.

**Retention/pruning check, as instructed**: grepped `crates/schedule-
reference/`, `crates/api/`, and every migration in `crates/api/migrations/`
for `prune`/`retention`/`DELETE FROM schedule_line_population`/`expire`.
The only pruning logic anywhere near this feature is `crates/schedule-
ingest/src/main.rs:454`'s `prune_old_deliveries`, which deletes old **raw
delivery directories** (the downloaded zip contents) once more than
`retention_keep_deliveries` (default `2`, `crates/schedule-ingest/src/
config.rs:68`) are on disk — an entirely different table/layer.
`schedule_line_population` itself has zero retention logic anywhere: once
a row is written for a `(line_id, service_date)` pair, it sits forever
(`crates/api/src/data/train_tracking.rs:27-31` documents the identical
"no retention job exists" posture for `tracked_trains` itself, for
comparison — this codebase has a consistent, if unbounded-growth, habit of
not pruning tables like this until real usage data says to). This is a
genuine, positive finding for this feature: the raw ingredient for
"steady-state" past-date resolution is not at risk of disappearing once
written.

### 2. Does `resolve_for_date` correctly resolve a past date? Yes, structurally, confirmed by reading the algorithm and its own tests

`schedule_query::resolve_for_date` (`crates/schedule-query/src/
resolve.rs:42-70`) filters `raw` schedules to `schedule.basic.date_from <=
date && date <= schedule.basic.date_to && days_of_week[weekday]`, then
picks the lowest (best-precedence) `StpIndicator` via `min_by_key`. Nothing
in this predicate treats "today" specially — `date` is a bare parameter,
compared only against each record's own `date_from`/`date_to` window. The
function's own test suite already exercises this non-symmetrically: `
resolve_for_date_picks_the_base_pattern_on_an_ordinary_tuesday` and `
resolve_for_date_picks_the_real_cancellation_override_on_260831`
(`resolve.rs:347-366`) both resolve dates that are, relative to the test
fixture's own "as of" framing, in the past — there is no code path here
that special-cases "date >= today," and the tie-break rule (STP-letter
`Ord`) is a pure function of the records and the date, not the wall clock.
`schedules_touching` (`resolve.rs:77-96`) is the same: it calls `resolve_
for_date` for every UID at the given `date` and has no forward-looking
filter of its own — unlike `departures_by_crs` (`resolve.rs:174-216`),
which explicitly drops anything with `booked_departure < now`
(`resolve.rs:193`, a documented, deliberate design choice for the
*different* "next departures" feature, not something `schedules_touching`
inherits). **This confirms the schedule-first design doc's own framing
(§"Comparison to `full-coverage-consumer`") was already correct that `
schedule_line_population`, unlike `schedule_network_departures`, "carries
no such filter."** Nothing new needs to change in `schedule-query` for a
past date to resolve correctly, *given a `ScheduleIndex` that actually
contains the record in question* — which is exactly Decision 3's
question, not this one.

### 3. The real constraint: does the ingested data still cover a date a month ago? Two regimes, confirmed against real code and real sampled data

This is the central question this document exists to answer, and the
honest answer splits into two structurally different cases that the task
brief's framing (treating "is a month-old date still resolvable" as one
question) does not distinguish, but the code does.

**Regime A — the row was already computed on the day itself (steady
state).** `crates/schedule-reference/src/main.rs:202`: `let today =
chrono::Utc::now().date_naive();`, then `publish_schedule_line_
population(client, config, &index, today, internal_oauth)`
(`main.rs:204`). Every cycle publishes for **today only**, using a
`ScheduleIndex` built fresh from whatever CIF delivery was most recently
processed that cycle (and only when a *new* delivery was actually detected
— `poll_once`'s `last_processed_delivery` early return, unchanged from
the schedule-first design doc's own citation of this same mechanism).
Concretely: the row written for `line_id = 'west-coast-main-line',
service_date = 2026-08-10` (if `schedule-reference` was live and
successfully processed a delivery that day) was built by resolving every
UID's STP precedence **using 2026-08-10's own live extract** — the exact
same trustworthy computation the schedule-first design doc already
validated for present-day pins. Reading that row back on 2026-09-06 is
not a *re*-resolution against stale data; it is reading an
already-correct, already-dated answer that has simply been sitting in
Postgres, untouched (Decision 1's pruning-free finding). **For this
regime, past-date resolution is exactly as accurate as present-day
resolution already is — there is no new correctness risk to document.**

**Regime B — no row exists yet for that `(line_id, date)` pair, and one
must be reconstructed after the fact.** This is the regime a "30 days back"
feature must actually confront on day one of shipping: **every date in the
first ~30 days after this feature ships has no pre-existing row**, because
the feature (and the periodic sweep that would have populated it) did not
exist yet. It also recurs any time `schedule-reference` has a processing
gap (missed a delivery, was down) on a given rail day — a permanent, silent
hole in that one line/date's history, never backfilled by anything running
today.

Reconstructing Regime B's answer would mean running `schedules_touching`/
`resolve_for_date` **today**, against **today's** `ScheduleIndex`, asking
for a `date` a month in the past. Decision 2 already confirmed the
*algorithm* has no problem with this. The problem is what `raw` actually
contains by the time it reaches that algorithm:

- **Confirmed, from real sampled bytes** (`docs/superpowers/specs/
  2026-08-29-trust-schedule-delay-inference-timetable-verification.md`,
  "Claim 1"): the one real full-refresh sample this repo has ever obtained
  (`RJTTF942MCA.txt`, generated 28/08/2026 per its own `DAT`/`MSN` banner)
  contains a real `BS` record, UID `C00573`, `Permanent` indicator, with
  `date_from = 2026-05-17` and `date_to = 2026-12-06` — a base pattern
  whose window spans more than three months before and three months after
  the extract's own generation date. **This confirms Permanent-schedule
  resolution for a month-old date is not structurally at risk**: a `P`
  record covering "today minus 30 days" is exactly the ordinary, expected
  shape a full refresh already carries, not an edge case.
- **Not confirmed, and this is the genuinely open, high-stakes gap**:
  whether a short-term STP overlay/cancellation (`C`/`N`/`O`) whose own
  `date_to` has *already fully elapsed* relative to the extract's
  generation date is still present in that extract, or has been dropped.
  The same verification doc's own quoted `C`-indicator example (`BSNG00704
  ...`, `date_from = 2026-05-17`, `date_to = 2026-08-30`) is close to but
  not conclusively past its extract's 28/08/2026 generation date (2 days
  either side) — it does not settle the question either way for a record
  a full 30 days stale. **This repo has never held two full-refresh
  samples spaced far enough apart to test this directly**, and no document
  in this repo's research trail (including `2026-09-03-schedule-feed-
  cadence-research.md`, which investigated cadence/retention questions in
  detail) addresses it. The only grounding available is the *documented
  purpose* of a CIF "Full Refresh" — RSPS5046's own framing, already
  established in this app's research chain, is that a full refresh
  reconstructs the *current and future* timetable state for a fresh
  recipient (`2026-08-30-schedule-feed-ingress-design.md`'s §7.6.1 citation:
  "a full refresh of timetable data" for new recipients), not a historical
  record of what ran on any given past day. **A schedule format built for
  forward operational planning has no obvious reason to keep carrying an
  STP overlay whose own effective window has fully closed** — every day
  that passes, retaining it serves no future consumer of that data. This
  is an inference from the format's stated purpose, consistent with how
  DTD/RSP full refreshes are generally documented to behave, but it is
  **not independently confirmed against real historical samples in this
  repo**, and this document does not claim otherwise.
- **Is there any historical archive that could sidestep this?** No.
  `crates/schedule-ingest/src/config.rs:61-68`'s `retention_keep_
  deliveries` (default `2`) is explicitly "current + fallback," and its
  own doc comment (`config.rs:63-65`) says outright: "No history/
  retention requirement beyond this exists today — a future 'also copy
  elsewhere for long-term retention' need should be a separate,
  purposefully-called copy step, not a change to this simple
  keep-N-most-recent behavior." `2026-09-03-schedule-feed-cadence-
  research.md`'s own §2 (the "full-daily + full-monthly" combination
  discussion) reaches the identical conclusion from a different angle:
  "this app retains no schedule-feed history today beyond
  `retention_keep_sequences`'s [now `retention_keep_deliveries`'s]
  'current + fallback'... no consumer reads or would benefit from a
  months-old snapshot today" — written before this feature was conceived,
  but it is now the exact gap this feature would benefit from closing.
  **There is no code path anywhere in this repo that has ever persisted a
  full historical CIF extract, or even just a `ScheduleIndex` built from
  one, beyond the current ingest cycle.** Reconstructing Regime B's answer
  from "whatever `schedule-reference` can build today" is the *only*
  mechanism available without new work.

**Net risk statement, stated as plainly as the evidence supports**:
Regime B resolution is not merely "sometimes missing" (an honest, visible
gap) — it is "silently confidently wrong" in the specific case where a
short-term STP overlay genuinely altered or cancelled a service on the
requested past date and that overlay's record no longer exists in any
extract this app can reach. `resolve_for_date`'s `min_by_key` will happily
fall through to the surviving `Permanent` base pattern and return a
result — `Some`, not `None` — indistinguishable in shape from a correct
match. A user retroactively logging a journey that ran on an overlay day
(engineering-work diversion, short-notice timetable change — exactly the
kind of day someone is more likely to remember and want to log) would see
a schedule that looks authoritative but describes the wrong service.

### 4. Does the periodic sweep already attempt past-dated pending pins?

Yes, mechanically, with no age discrimination at all — but this is
presently moot because of Decision 1 below. `list_pending_pins_for_
schedule_match` (`crates/api/src/data/train_tracking.rs:529-539`):

```sql
SELECT id, service_date, pin_origin_crs, pin_scheduled_departure
FROM tracked_trains WHERE train_uid IS NULL AND resolution_status = 'pending'
```

No `WHERE service_date >= ...` clause, no age cap, nothing referencing
`now()` at all. Every still-`pending` row, regardless of how old its
`service_date` is, is retried on every sweep cycle
(`schedule_matching::run_schedule_match_sweep`, `schedule_matching.rs:
178-201`, iterates every row this query returns unconditionally). **The
sweep itself needs zero changes to attempt past-dated pins** — it already
would, today, forever, for any row that exists. The only reason it never
does is that no such row can currently come to exist at all (Decision 5).

### 5. The actual, primary blocker: `MAX_PIN_AGE` rejects pin creation itself, before any of the above ever runs

`crates/api/src/data/train_tracking.rs:25`: `const MAX_PIN_AGE:
chrono::Duration = chrono::Duration::hours(6);`, enforced by `validate_pin`
(`train_tracking.rs:50-72`):

```rust
if now - pin.scheduled_departure > MAX_PIN_AGE {
    return Err(format!(
        "That departure time is more than {} hours ago — trains can only be tracked \
         within {} hours of departure.", ...
    ));
}
```

Called synchronously from `post_track` (`crates/api/src/routes/
train.rs:425`) **before** `create_pin` ever inserts a row, and therefore
before `attempt_schedule_match` (`routes/train.rs`'s subsequent call, per
the schedule-first design's Task 7) is ever reached. **A request to track
a month-old journey is rejected with `400 Bad Request` today, full stop —
none of Decisions 1-4's machinery is ever invoked.** The frontend already
knows and surfaces this exact limit: `frontend/components/
TrackTrainForm.tsx:515-525`'s `DateTimePicker` carries the description
"Must be within the last 6 hours, or any time in the future," with a code
comment citing `MAX_PIN_AGE` by name.

**The comment justifying this constant is now partially stale, not
wrong.** `train_tracking.rs:17-24`: "A pin more than this far in the past
is almost certainly a stale frontend view... reject it rather than create
a `tracked_trains` row trust-consumer can never resolve (TRUST's Train
Movements feed is a live stream, not a historical lookup; a pin for a
service that ran days ago will sit 'pending' forever)." That reasoning was
airtight when TRUST was the *only* resolution path — it is precisely
Decision 3 of this document's Regime A that now offers a second, genuinely
different resolution path for exactly the case this comment worries about,
*for dates within the window a `schedule_line_population` row already
exists*. The comment's core worry ("sit pending forever") does not apply
uniformly any more — but it also has not been *disproven* uniformly,
because Regime B (Decision 3) is real and unresolved. Loosening `MAX_PIN_
AGE` wholesale, without addressing Regime B, would trade "pin creation is
rejected, honestly, at the door" for "pin creation is accepted, and then
either resolves correctly (Regime A) or resolves confidently-but-possibly-
wrong (Regime B) or sits at `pending`/some new terminal state forever with
no live TRUST rescue ever coming (any date, since TRUST cannot retroactively
report on an old rail day either — worth stating explicitly: **TRUST
Movements is exactly as incapable of resolving a month-old pin as it is
today; this feature is 100% dependent on schedule data succeeding, with no
live fallback at all**, unlike the schedule-first design's present-day
case where TRUST is still layered on top as the eventual live-status
source).

## Concrete implementation approaches, with tradeoffs

**Approach A — Accept degraded accuracy, resolve past dates against
whatever's currently available, flag it clearly.** Relax `MAX_PIN_AGE` (or
add a distinct, explicitly-past-dated creation path bypassing it up to a
new `MAX_PAST_PIN_AGE` — recommended over touching the existing constant
directly, since the existing 6-hour ceiling's rationale for *near-term*
pins is unrelated and should not be conflated with a *month-old* pin's very
different risk profile) up to ~30 days. Run `attempt_schedule_match`
exactly as today, unmodified — Decisions 1-2-4 already establish it needs
no change. Where no `schedule_line_population` row exists (Regime B),
either (a) leave the pin `pending` forever with no reconstruction attempt
at all — honest but frequently useless, since most of the first 30 days
after shipping will hit exactly this case — or (b) attempt a live, one-off
`schedules_touching`/`resolve_for_date` reconstruction from today's
current `ScheduleIndex`, but surface the result with **caveated,
un-hideable UI copy** distinct from the schedule-first design's own
`schedule_matched` copy — something like "matched against today's
timetable, applied retroactively — short-notice changes on the day may
not be reflected" — so a wrong-due-to-a-dropped-overlay match is at least
never presented with the same confidence as a same-day schedule match.
**Cheap, ships fast, reuses everything.** Its cost is inherent, not
implementation debt: it cannot ever close the Regime B accuracy gap, only
disclose it.

**Approach B — Start archiving daily extracts (or just resolved
`schedule_line_population` rows) going forward, so future "past date"
lookups are exact.** Two sub-variants:

- **B1: Archive raw CIF deliveries.** Add a genuinely new, purposefully-
  called "copy elsewhere" step to `schedule-ingest` — exactly what
  `config.rs:63-65`'s own comment already names as the correct shape for
  this need ("a separate, purposefully-called copy step, not a change to
  this simple keep-N-most-recent behavior"). Buys the most flexibility
  (any future consumer, not just line-population resolution, can replay a
  historical extract) at the highest storage cost — `2026-09-03-schedule-
  feed-cadence-research.md`'s own §1 math (~711MB/delivery uncompressed)
  applies directly: 30 days retained would be ~21GB, dwarfing the current
  5Gi PVC's steady-state ~1.4GB. Needs its own PVC sizing conversation.
- **B2: Widen `schedule_line_population`'s own publish scope.** Instead of
  archiving raw extracts, have `schedule-reference` (or a new backfill
  path) publish `schedule_line_population` rows not just for `today` but
  proactively cover a trailing window — this only helps *from the point
  it ships forward*, identical in kind to Regime A's own steady-state
  story, just widening what counts as "already computed." This does
  **not** solve the bootstrap gap (the 30 days *before* this ships still
  has nothing to backfill from, since the raw extracts that could
  reconstruct them are already gone under `retention_keep_deliveries=2`)
  — only B1, done far enough in advance, or accepting the bootstrap gap
  under Approach A for that one window, closes that specific hole. Much
  cheaper than B1 (JSONB rows, not raw 700MB extracts; `2026-09-03-
  schedule-feed-cadence-research.md`'s own "full-daily + full-monthly...
  costs almost nothing extra to store" framing scales down further here,
  since this is already-summarized per-line population data, not a full
  national extract).

**Hybrid (recommended): ship Approach A now, start B2 (or B1, if a future
full-timetable-ingestion consumer per the schedule-first design doc's own
Non-goals ever needs the raw extracts for other reasons anyway) as an
unconditional, low-cost habit going forward.** Reasoning:

- Approach A alone is honest about its own limits (Decision 3's Regime B
  risk is real and cannot be engineered away after the fact for dates
  whose overlay data is already gone) but is the only option that ships
  anything for the feature's own bootstrap window at all.
- B2 costs little (Regime A's own steady-state accuracy is already free —
  it happens automatically once the sweep/pin-creation-window changes
  ship; B2 only means also computing it for a wider trailing window than
  `today` alone) and converts every day going forward into a Regime A day
  once ~30 days of runtime accumulate, at which point Approach A's
  degraded-accuracy caveat becomes true only for the shrinking, eventually-
  empty bootstrap slice rather than a permanent condition.
- B1 is not recommended as part of *this* feature specifically — its
  benefit (exact reconstruction of arbitrary historical STP state) is
  real but only pays off for the bootstrap window this document already
  scopes as "accept the caveat" under the hybrid, and its storage cost is
  an order of magnitude larger than B2's. It remains worth revisiting
  exactly where `2026-09-03-schedule-feed-cadence-research.md`'s own §4
  already flagged it — "alongside the full-timetable ingestion project,"
  not as a standalone justification for this feature alone.

## Explicitly out of scope (Non-goals)

- **Relaxing or redesigning `MAX_PIN_AGE` itself, or its exact new value.**
  Decision 5 identifies it as the actual blocker and argues a *separate*
  past-dated creation path is preferable to changing the existing
  constant, but the specific new ceiling, its validation message copy, and
  whether it differs by whether a `schedule_line_population` row already
  exists for that date (a "we can probably answer this" vs. "we're
  guessing" distinction at *creation* time, before a match is even
  attempted) are implementation-plan decisions, not scoped here.
- **A UI/UX design for "log a past journey."** This document establishes
  that the feature is data-feasible in Regime A and caveat-feasible in
  Regime B, not what the tracking form, list view, or `TrainJourney.tsx`
  copy should look like for a schedule-matched-but-never-live pin that
  will now *never* receive a live TRUST update (a real, new terminal state
  this feature introduces that the schedule-first design's `resolved`/
  `schedule_matched` split did not anticipate — see Open Questions).
- **Building B1 (raw historical extract archival) as part of this
  feature.** Named as a considered alternative (Approach B), explicitly
  not recommended for this feature alone (see Hybrid reasoning above).
- **Resolving whether CIF full refreshes actually drop fully-expired STP
  overlays.** This is the single most important open technical fact this
  document could not close — it requires either a primary-source read of
  RSPS5046's exact retention rules for expired records (not available to
  this pass any more than it was to any prior research document that
  touched CIF format questions) or two real historical extracts spaced
  ~30 days apart to compare directly. Flagged, not resolved.
- **A migration file or any Rust/TypeScript code.** Per this task's brief,
  this is a scoping document only.
- **Changing `schedule_line_population`'s current no-pruning behavior.**
  Decision 1's finding (nothing prunes it) is treated as a positive
  property this feature benefits from, not something to touch.
- **The FUTURE-dates side of this same underlying question** (a pin for a
  date that hasn't happened yet, beyond whatever `schedule-reference`
  currently computes) — that is the sibling document's scope
  (`2026-09-06-schedule-line-population-future-dates-design.md`), not
  this one's. The two share `schedule_line_population`'s schema and
  `attempt_schedule_match`'s machinery but have essentially opposite risk
  profiles: the future side risks *no data yet existing at all* (nothing
  to overlay-drop, since nothing has been finalized), while the past side
  (this document) risks *data that used to exist correctly but may since
  have been superseded/dropped*.

## Rough task breakdown (scope-sizing only, not a plan)

1. **Decide and implement the pin-creation-time change.** New, distinct
   validation path for a past-dated pin (separate constant/config from
   `MAX_PIN_AGE`, per Decision 5's recommendation), with its own
   user-facing copy distinguishing "recently departed" (today's existing
   6-hour window, TRUST-eligible) from "logging a past journey" (the new,
   schedule-only window, no live-tracking promise). Touches `validate_pin`,
   `post_track`, `TrackPinRequest` if a new field is needed to
   distinguish intent, and `TrackTrainForm.tsx`.
2. **Decide Regime B's product behavior**: pending-forever vs. live
   reconstruction-with-caveat (Approach A's (a) vs. (b)). If (b): a new
   function alongside `attempt_schedule_match` that builds a fresh
   `ScheduleIndex` on demand for a cache-miss past date (needs `schedule-
   reference`'s parsing/index-build logic reachable from `api`, or a new
   RPC to `schedule-reference` — a real architectural question not
   resolved here) and a way to mark the resulting match as
   reconstruction-derived (a new `resolution_status` value or a boolean
   flag, distinct from Regime A's ordinary `schedule_matched`) so the
   frontend can render the caveat.
3. **New terminal-state UX** for a past pin that resolves via schedule
   data alone and will never receive a live TRUST update — decide whether
   this reuses `schedule_matched` as a true terminal state for past pins
   (vs. its current "waiting for TRUST" framing for present-day pins) or
   needs its own value.
4. **If Hybrid's B2 is pursued**: widen `schedule-reference`'s publish
   scope from "today only" to "today plus a trailing N-day window" (or a
   separate backfill job), with its own row-count/storage math (cheap per
   Decision math above, but not zero — worth a real estimate once N is
   chosen) and a decision on whether it publishes for every catalogued
   line every cycle (matching `lines_to_publish`'s existing unconditional
   scope) or is itself capped/opt-in.
5. **Sweep/query changes, if any** — Decision 4 already shows `list_
   pending_pins_for_schedule_match` needs no change to *attempt* past
   rows; only needed if Task 2 introduces a genuinely different matching
   function for the reconstruction path that the sweep must also call.
6. **Frontend**: past-journey entry point (likely reusing/adjacent to
   `TrackTrainForm.tsx`, or the ticket-entry flow's own retroactive-logging
   framing, `2026-08-29-journey-ticket-tracking-design.md`, as prior art
   for "the user is describing something that already happened" copy/UX),
   plus the caveat copy from Task 3.
7. **Verification**: extend `schedule_matching.rs`'s existing `db_tests`
   (already structured exactly for this — `attempt_schedule_match_
   reproduces_the_eus_bug_and_now_resolves_it` is a template) with a case
   seeding a `service_date` 30 days in the past, proving Regime A's
   read-back path end-to-end; a second test proving Regime B's pending
   (or reconstruction) behavior when no such row exists.

## Open questions / risks

1. **Does RSPS5046 (or any DTD documentation) explicitly state whether
   expired STP records are dropped from subsequent full refreshes?** The
   single most consequential unresolved fact in this document (Decision 3).
   Answering it precisely could either substantially de-risk Approach A's
   reconstruction path (if expired records are, in fact, retained for some
   grace period) or confirm the risk is as bad as this document assumes.
2. **What should happen to a past pin that never schedule-matches at
   all** (no candidate line, or no calling point in tolerance) — today's
   equivalent for present-day pins is "stay `pending`, TRUST might still
   claim it later." For a month-old pin, nothing will ever claim it. Does
   it need a distinct terminal `unresolved`-like state applied
   immediately, rather than sitting `pending` indefinitely the way the
   sweep's unconditional retry (Decision 4) would otherwise leave it
   forever re-attempting a match that will never newly succeed?
3. **Multi-day STP-precedence interaction with Regime B reconstruction**:
   if a reconstruction is attempted, should it warn differently depending
   on whether the *only* candidate found is a `Permanent` schedule (higher
   confidence nothing was overlaid, though still not proof) versus a
   surviving `Overlay`/`New` record that happens to still cover that date
   in today's extract (lower residual risk, since at least *some* STP
   machinery for that date is still visible)? Not resolved here — flagged
   as a possible refinement to Approach A's caveat copy.
4. **Exact lookback ceiling** — this document uses "~30 days" per the
   task brief throughout, but does not independently justify why 30 vs.
   any other number; it is treated as a given product requirement, not
   derived from any technical constraint (`schedule-ingest`'s own
   retention is far shorter, 2 deliveries, and is irrelevant to the
   *ceiling* once Regime A/B is understood — it only bears on Regime B's
   reconstruction feasibility, not on how far back a *pin* should be
   allowed to look).
