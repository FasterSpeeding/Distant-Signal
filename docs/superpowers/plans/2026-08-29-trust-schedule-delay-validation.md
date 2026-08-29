# TRUST-vs-Schedule Delay Inference: Validation Pass — Plan

> **For agentic/human workers:** This is **not** a "build feature X" plan.
> There is no crate to add, no migration to run, no route to mount. The
> deliverable is a short written **go/no-go recommendation** on whether to
> proceed with Option B from
> `docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`
> ("the design spec"), produced by actually running the two cheap checks
> that spec's own Recommendation section said to run before committing any
> further engineering time. Tasks below are still written as
> checkbox-tracked steps for continuity with this repo's other plans, but
> several of them have no code output at all — their "deliverable" is a
> human decision, a piece of information, or a short paragraph in a
> findings write-up, not a commit.

**Reads required before starting:**
- `docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`
  ("the design spec") — in full, especially "Recommendation" and "Open
  questions and risks."
- `docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-timetable-verification.md`
  ("the verification spec") — in full. It confirmed real CIF format/TIPLOC
  claims against `timetable_full.zip` and, most importantly, found that
  CORPUS is likely *not* a separate needed feed (the CIF extract's own `TI`
  + `MSN` files already carry the STANOX↔CRS mapping `trust-consumer`
  needs).

**What this plan answers:** the design spec's own recommendation was
"proceed with caveats — but not yet," conditioned on two things it
explicitly left undone:

1. Independently confirm CIF SCHEDULE's and CORPUS's real RDM licensing,
   approval lag, and cost — unconfirmed because RDM's catalogue requires a
   logged-in account the design spec's research pass didn't have.
2. A cheap empirical check: does segment-level TRUST-vs-schedule
   correlation actually catch/better-attribute real disruptions that this
   app's current LDBWS-sampling inference misses or under-attributes? The
   design spec was explicit this "can only be tested against real running
   data, not reasoned out in the abstract."

This plan is the concrete task breakdown for both. **It does not plan
Option B itself** (the dedicated `trust-line-aggregator`-style consumer
service) — that remains a separate, later planning pass, gated on this
plan's own decision gate (Task 8) coming back "go."

---

## Non-goals — read before writing any code

Because "empirical validation" can quietly balloon into "let's just build
a small prototype of the real thing," these are explicit and binding for
every task below:

- **No new Kafka consumer group.** Task 4's real-TRUST-data capture reuses
  `trust-consumer`'s *existing*, already-deployed, already-approved
  consumer group and pin-tracking feature (`POST /Train/track`,
  `train_movement_events`) exactly as a real end user would use it today —
  not a new consumer, not a modified `trust-consumer`, not a second Kafka
  connection of any kind, temporary or otherwise.
- **No CIF/CORPUS ingestion pipeline.** Task 5's schedule reconstruction
  reads directly from the local `timetable_full.zip` reference file
  already sitting at the repo root (untracked, do not `git add` it, do not
  extract it to disk — stream reads only, per the verification spec's own
  approach) via a one-off script. It does not stand up any recurring
  ingest, file-watcher, or SFTP/bucket endpoint.
- **No new database schema for production use.** Every table this plan
  reads from (`incident_history`, `line_status_history`, `tracked_trains`,
  `train_movement_events`, `train_current_state`) already exists and is
  already written by already-shipped code. Nothing here adds a column, a
  table, or a migration.
- **No engineering effort toward Option B itself.** No `SegmentRegistry`
  matching path, no per-line schedule-adherence aggregation logic, no
  `DataQuality::TrustInferred` wiring. If this plan's decision gate comes
  back "go," *that* work gets its own future plan.
- **The output artifacts are throwaway, except the final write-up.** Any
  helper script (bulk-pin loop, schedule-extract reader) and any raw data
  dump this plan produces stays local/scratch — not committed to the repo.
  Only the final findings + go/no-go recommendation (Task 8) is a
  committed document, following the same precedent as the two existing
  research specs.

---

## What's actually achievable — read before assigning Task 4

Checked directly against the real code (not assumed), because the design
spec flagged this as unresolved and the verification spec didn't touch it:

- **`trust-consumer` does not, and has never, stored a full-feed
  historical archive.** Its own module doc
  (`crates/trust-consumer/src/process.rs`) and `matching.rs` confirm it
  matches TRUST messages against an in-memory set of currently-`pending`
  tracked pins only; non-matching messages are parsed and discarded, never
  persisted. `train_movement_events` (`crates/api/migrations/20260828120000_train_tracking.sql`)
  has exactly one row per TRUST message matched to a *resolved, previously
  user-pinned* train — there is no table, anywhere in this codebase, that
  holds arbitrary historical TRUST traffic for a line or a date that
  wasn't specifically pinned in advance.
- **There is consequently no way to retroactively ask "what did TRUST say
  about this line on a day last month."** Kafka's own broker-side topic
  retention for RDM's Train Movements product is itself unconfirmed in
  this app's research (typically short — hours to a few days — for feeds
  like this, though not independently verified here), so even a brand-new
  consumer group started today could not necessarily replay arbitrarily
  far back even if one were allowed, which it isn't (non-goals, above).
  **This validation is therefore inescapably forward-looking**: pick
  lines/trains/days *ahead of time* and capture real TRUST data for them
  as they run, not backward-looking against days that have already
  happened.
- **This app's own LDBWS-sampled line status has the opposite shape**:
  `line_status_history` (`crates/api/migrations/20260510023522_initial.sql`)
  is real, already-running historical data, queryable today via the
  already-shipped, public `GET /Line/{id}/Status/{from}/to/{to}` route
  (`crates/api/src/routes/line_status.rs`,
  `queries::line_status_history_for_range`). Its production retention
  default is 7 days (`charts/distant-signal/values.yaml`'s
  `aggregator.historyRetentionDays: 7`) — not a blocker for a
  forward-looking validation (the window being monitored is freshly
  written, and Task 7's analysis happens promptly after), but worth
  confirming against the live deployment's actual configured value before
  relying on it, in case it's been changed from the chart default.
- **`incident_history` (same migration) is never pruned** — confirmed by
  grep across `crates/`, no `DELETE FROM incident_history` anywhere. It is
  a real, durable, already-accumulating record of every incident the
  Knowledgebase poller has ever seen change, going back to whenever this
  app started running in production. This *is* usable backward-looking,
  and is this plan's source of real ground truth for "what disruptions
  actually happened, historically, on the lines being validated" (Task 2)
  — even though the TRUST side of the comparison can't reach back to match
  it day-for-day.
- **The critical unlock**: `trust-consumer`'s already-shipped pin-tracking
  feature (`POST /Train/track` → `crates/api/src/routes/train.rs`, backed
  by the *existing* consumer group) is, itself, a working mechanism for
  getting real per-train TRUST movement data for trains chosen in advance
  — with zero new engineering. A validator can pin a deliberately dense
  set of real trains (e.g., every service departing a line's curated
  `sample_stations` in a chosen window) for a chosen future day, and read
  back each one's real movement-event history once it's run. This
  approximates — manually, at the scale of dozens of trains rather than
  the whole national population — the segment-level, every-train coverage
  Option B would eventually automate. It is not full-population coverage
  (a real, honest limitation — see Task 4), but it is real TRUST data,
  today, using nothing this app hasn't already shipped.

---

## Task 1: RDM licensing/access confirmation (human-only)

**This task cannot be done by an agent.** RDM's catalogue requires a
logged-in account — the same wall the design spec's own research hit for
TRUST, and the same wall the train-tracking design doc hit before it. No
subagent, browser tool, or automated process in this session can resolve
it; it needs a human with real RDM credentials to look at real pages.

**Depends on:** nothing — can run in parallel with every other task in
this plan.

**Produces:** a short written answer (a paragraph or two, plus links) to
feed into Task 8's decision gate. Not code.

- [ ] **Step 1: Log into RDM's catalogue directly**
  (https://raildata.org.uk or wherever the current train-tracking
  deployment's credentials point — check `dev.env`/deployed secrets for
  the account already in use for TRUST, since CIF SCHEDULE/CORPUS are, per
  the design spec, likely the same Network Rail Infrastructure Limited
  publisher).

- [ ] **Step 2: Find CIF SCHEDULE's real product listing.** Record:
  - Its exact product name/id in the catalogue (the design spec flagged an
    open nuance: is this literally RDM's "SCHEDULE" product, or a
    differently-named ATOC/RSP "Full Timetable" product under a different
    distribution channel — the verification spec could not settle this
    from the file alone; RDM's own UI can).
  - Whether it requires separate approval beyond whatever licence already
    covers TRUST, or rides on the same Network Rail Infrastructure Limited
    sign-off.
  - Any listed cost tier (free under OGL vs. paid).
  - Any stated approval-lag SLA (or lack of one, same as TRUST's own
    listing).

- [ ] **Step 3: Find CORPUS's real product listing** (if it's still listed
  separately at all — the verification spec's finding that `TI`+`MSN`
  already carry the STANOX↔CRS mapping means CORPUS may not be worth
  pursuing regardless of its terms, but record its listing anyway for
  completeness in case a future need for nightly-refreshed reference data
  arises independent of this feature).

- [ ] **Step 4: Confirm whether the existing TRUST subscription's terms
  say anything about running additional consumer groups** under the same
  account (relevant only if a future Option B build wants its own
  consumer group — not required for this plan's Task 4, which reuses the
  existing one, but worth a one-line note while already looking).

- [ ] **Step 5: Write up the answer** — a short paragraph covering: is
  either CIF SCHEDULE or CORPUS paid or slow-to-approve? If **either**
  requires separate paid licensing or a slow manual-approval process, say
  so plainly — the design spec was explicit that this alone could be
  reason to stop here regardless of what Task 7's empirical results show.
  Feed this into Task 8.

---

## Task 2: Ground-truth disruption history for the chosen line(s)

**Depends on:** nothing (uses only already-running, already-populated
data).

**Produces:** a short list of what real disruptions have actually looked
like recently on the line(s) chosen for validation — calibration for what
Task 7's spot-check should be looking for, and (secondarily) a source of
already-published *future* planned works to target in Task 3.

- [ ] **Step 1: Pick 1-2 lines to validate against.** Recommended:
  `west-coast-main-line` and `swr-alton` — both already used as the worked
  examples in the design spec and verification spec, both already have
  confirmed-real TIPLOC coverage in `timetable_full.zip` (Euston/Watford
  Junction/Carlisle; Waterloo/Alton), and WCML in particular is the
  design spec's own running example for "sampling can't see the whole
  route" (5 sample stations against a line spanning Euston to Carlisle).

- [ ] **Step 2: Query `incident_history` directly** (no API route exposes
  it — `crates/api/src/data/queries.rs` writes it but nothing reads it
  back out; direct `psql` against the deployed database is the only path):

```sql
SELECT incident_id, summary, priority, is_planned, validity_periods,
       affected_stations, operators, recorded_at
FROM incident_history
WHERE operators && ARRAY['VT','SW']  -- WCML/SWR operator ATOC codes; adjust per chosen line(s)
   OR affected_stations && ARRAY['EUS','MKC','CRE','PRE','CAR','WAT','AON']
ORDER BY recorded_at DESC
LIMIT 50;
```

  Read through the results. Note recurring patterns (planned engineering
  weekends, a specific recurring delay-prone segment, etc.) — this is
  real, durable historical signal this app has never had to build
  anything new to access.

- [ ] **Step 3: Separately, pull `line_status_history` for the same
  line(s) over whatever window the live deployment's current retention
  covers**, via the already-public route:

```bash
curl -s "https://<deployed-host>/Line/west-coast-main-line/Status/<from>/to/<to>"
```

  This shows what the *sampling*-derived signal has actually reported
  recently for the chosen line — the baseline Task 7 will be comparing
  against.

- [ ] **Step 4: Write a short paragraph** (a few sentences, not a report)
  summarizing: what real disruptions/planned works has this line had
  recently, and did `line_status_history` reflect them at a severity that
  seems right, too low, or absent? This is context, not a finding yet —
  it primes Task 7's eventual comparison and Task 3's day selection.

---

## Task 3: Choose the validation window and confirm timetable coverage

**Depends on:** Task 2 (uses its incident-history read to find real
already-published planned works to target).

**Produces:** a concrete, written validation plan for Task 4-7: which
line(s), which specific future day(s) (a "handful" — 2-3 is enough), which
stations/trains to pin.

- [ ] **Step 1: From Task 2's `incident_history` read, identify any
  already-published *planned* engineering works** (`is_planned = true`,
  `validity_periods` covering a date still in the future relative to when
  this task runs) affecting the chosen line(s) within the next ~2-4 weeks.
  These are the most reliable "guaranteed disruption day" targets — NRE
  publishes planned engineering closures with real lead time, unlike
  unplanned incidents which can't be predicted.

- [ ] **Step 2: If fewer than 2-3 planned-work days are found**, don't
  wait indefinitely — supplement with an open-ended monitoring window
  instead: pick any 1-2 ordinary days in the near future and pin densely
  regardless, accepting that some days may turn out to be uneventful (a
  "no disruption occurred" day is still informative — see Task 7's
  write-up, which should report both hits and clean misses honestly, not
  cherry-pick only days with drama).

- [ ] **Step 2: Confirm `timetable_full.zip` actually covers the chosen
  day(s).** Per the verification spec's Claim 4, real `BS` date ranges in
  this file span multi-month windows (`260517`–`261206` for one sampled
  WCML schedule). Spot-check by streaming a `grep` for the chosen line's
  key TIPLOCs and eyeballing the date-range field on a few `BS` lines
  covering the chosen day:

```bash
unzip -p timetable_full.zip RJTTF942MCA.txt \
  | grep '^BS.....2604\|^LOEUSTON' | head -5   # adjust date substring to the chosen day, per the CIF Basic Schedule field layout the verification spec decoded
```

  If the chosen day falls outside the file's covered window (unlikely
  given today is 2026-08-29 and the file's own banner says "Generated:
  28/08/2026," but confirm rather than assume), either pick a nearer day
  or accept that a fresh extract would be needed — flag this rather than
  silently working around it.

- [ ] **Step 3: Write down, concretely, which specific stations/services
  will be pinned on each chosen day** — e.g., for a WCML day: every
  scheduled departure from Euston, Milton Keynes Central, Crewe, Preston,
  and Carlisle (the line's existing `sample_stations` — same set the
  current LDBWS sampling already uses, deliberately, so Task 7's
  comparison is apples-to-apples on *coverage location* and differs only
  on *coverage density*) within a chosen multi-hour window, pulled from
  the real LDBWS departure boards (`GET` against `poller-ldbws`'s existing
  sampled data, or the live Darwin board) the morning of, since that's the
  simplest source of "what's actually running today" — not from CIF
  (CIF gives the advance schedule for Task 5's comparison, not a same-day
  running list, and the two won't disagree at this level of granularity
  for a scheduled service).

---

## Task 4: Capture real TRUST data via the existing pin mechanism

**Depends on:** Task 3 (needs the concrete day/station/train list).

**Produces:** real `train_movement_events` rows for the pinned trains,
accumulating naturally as `trust-consumer` (already running, already
connected — per this session's own history) processes the live feed on
the chosen day(s). No new code; this task is *operating* the existing
product, at a slightly larger scale than one end user would.

- [ ] **Step 1: Write a small, throwaway bulk-pin script** — a loop over
  the station/train list from Task 3, calling the *existing*
  `POST /Train/track` route once per train (same call shape the
  train-tracking plan's own manual-verification steps already used):

```bash
curl -s -X POST https://<deployed-host>/Train/track \
  -H "Cookie: nr_session=<a real session token>" -H "Content-Type: application/json" \
  -d '{"service_date":"<day>","origin_crs":"EUS","scheduled_departure":"<iso8601>"}'
```

  Requires a real authenticated session (`AuthenticatedUser`) — either the
  validator's own real account, or a manually-inserted test user/session
  row the same way the train-tracking plan's Task 3 Step 7 manual
  verification did. This script is scratch — do not commit it; it's a
  thin wrapper around an already-shipped API, not new product surface.

- [ ] **Step 2: Run it the morning of each chosen day**, pinning every
  identified departure before it happens (pin creation rejects a
  `scheduled_departure` more than `MAX_PIN_AGE` = 6 hours in the past, per
  `crates/api/src/data/train_tracking.rs` — so pins must go in either the
  night before or same-morning, not after the fact).

- [ ] **Step 3: Let the day run.** No further action needed —
  `trust-consumer`'s existing resolution/event-append/current-state logic
  does the work, exactly as it does for any real user's pin today.

- [ ] **Step 4: The following day, pull every pinned train's resolved
  state and event history.** Bulk read via the existing private reference
  route (needs the internal token, same as any poller):

```bash
curl -s https://<deployed-host>/private/tracked-trains -H "x-internal-token: $INTERNAL_TOKEN"
```

  For each `id` returned, fetch its full detail
  (`GET /Train/{trackingId}`, public) or query `train_movement_events`
  directly for the raw per-location timestamps Task 7 actually needs
  (`loc_stanox`, `planned_timestamp`, `actual_timestamp`,
  `variation_status`) — the public route surfaces only
  `train_current_state`'s denormalized summary, not the full per-event
  log; a direct `psql` read of `train_movement_events WHERE tracked_train_id = ANY($1)`
  is simplest for Task 7's purposes.

- [ ] **Step 5: Note plainly, in whatever you hand to Task 7, which pins
  never resolved** (`resolution_status = 'pending'` or `'unresolved'` —
  per `process.rs`'s own documented gap, a pin can stay `pending` forever
  if its Activation was missed). An unresolved pin is a real data point
  about this feature's reliability, not a bug in this validation — report
  the resolution rate honestly rather than silently dropping unresolved
  pins from the comparison.

- [ ] **Honest scope note to carry into Task 7**: this captures a dense,
  curated *sample* of real trains (dozens, at the existing sample
  stations plus whatever intermediate stops those trains' own movement
  events happen to report), not the full population of every scheduled
  service touching the line's entire TIPLOC set the way Option B's actual
  architecture would. It is real data, and it does extend past the
  sample-station points themselves (a pinned train's event log includes
  *every* location TRUST reports for it along its route, not just its
  origin) — but it is not literally "every train," and Task 7/Task 8's
  write-ups should say so rather than imply this validation proves
  full-population coverage.

---

## Task 5: Reconstruct "what should have happened" from the real timetable file

**Depends on:** Task 3 (which day/trains), can run any time after (does
not depend on Task 4 having finished — this is pure schedule
reconstruction, independent of what TRUST actually reported).

**Produces:** a small, throwaway script (not committed) that, given a
TIPLOC and a date, prints the scheduled calling times CIF says should
apply — the "expected" side of Task 7's comparison.

- [ ] **Step 1: Write a one-off script** (Python or a scratch Rust binary
  outside the workspace — whichever is faster to iterate in; it is not
  workspace code and should not be added to any `Cargo.toml`) that:
  1. Streams `RJTTF942MCA.txt` out of `timetable_full.zip` via
     `unzip -p` (never extract the 711MB file to disk — same approach the
     verification spec used).
  2. For each `BS` record whose date range covers the chosen day, resolve
     STP overlays correctly: group by `(UID, start date)`, and for a given
     day prefer `C`/`O`/`N` over `P` per the CIF STP rule the design spec
     and verification spec both confirmed (lowest-alphabetically wins,
     `C` meaning cancelled-that-day with no body at all — verified real in
     the verification spec's Claim 1).
  3. For schedules that survive, read the `LO`/`LI`/`LT` body lines and
     extract the scheduled time at each TIPLOC of interest (the chosen
     line's stations, per `lines/<line>.toml`'s `tiploc` field).
  4. **Apply the two real gotchas the verification spec already found**,
     rather than rediscovering them the hard way: (a) the schedule-body
     TIPLOC field is fixed 7-character space-padded (`"EUSTON "`), while
     `lines/*.toml` stores the bare unpadded string — pad/trim
     consistently before comparing, or every TIPLOC shorter than 7
     characters silently fails to match; (b) don't assume one CRS maps to
     one TIPLOC (Waterloo is at least three, only `WATRLMN` actually
     appears in schedule bodies) — use `lines/*.toml`'s already-curated,
     already-validated `tiploc` field directly rather than re-deriving a
     CRS→TIPLOC mapping from scratch.

- [ ] **Step 2: Cross-check the TIPLOC→STANOX join** using the same file's
  `TI` records (`TIEUSTON 00144400NLONDON EUSTON  724102893EUS...` →
  STANOX `72410`), so the script's output can be directly compared against
  Task 4's captured `train_movement_events.loc_stanox` values without a
  human doing the translation by hand. Where a `TI` record's CRS field is
  blank (confirmed common in the verification spec's Claim 3, including
  for `WATRLMN` itself), fall back to `RJTTF942MSN.txt`'s `A` records for
  the CRS, exactly as the verification spec's Claim 3 worked out.

- [ ] **Step 3: For each of Task 4's pinned trains, produce a small table**:
  TIPLOC, scheduled time (from this script), reported actual time (from
  Task 4's captured `train_movement_events`), delta. This is the literal
  "expected vs. actual" comparison Task 7 reads.

---

## Task 6: Pull the sampling-side baseline for the same window

**Depends on:** Task 3 (which day/line).

**Produces:** the third input to Task 7's comparison — what the
*existing, currently-shipping* LDBWS-sampling inference actually reported
for the chosen line during the chosen day, unchanged from what a real user
saw at the time.

- [ ] **Step 1: Pull `line_status_history` for the exact chosen day(s)**,
  via the same public route used in Task 2 Step 3:

```bash
curl -s "https://<deployed-host>/Line/west-coast-main-line/Status/<day-00:00Z>/to/<day+1-00:00Z>"
```

- [ ] **Step 2: Also pull the raw `station_samples` rows for the chosen
  line's `sample_stations`, for the same window**, if `station_samples`
  history is needed at finer granularity than `line_status_history`'s
  snapshot-on-change rows — check whether `station_samples` itself keeps
  any history or is wholesale-replaced each poll (per the design spec's
  own note: "no calling-point list, no persisted per-service identity
  across polls" — confirm this is still accurate before assuming any
  history exists here; if it's replace-only, `line_status_history`'s
  snapshot-on-change log is the only real historical record available,
  and that's fine — it's still the actual product output a real user saw).

- [ ] **Step 3: Note the `dataQuality` field on each returned status** —
  `ldbws-inferred` vs `knowledgebase` (if an incident happened to be
  active for part of the window, incident-derived status would have won
  outright, which is itself a relevant data point: if Knowledgebase
  already caught a given disruption at good severity, that's not a case
  where TRUST inference would have added anything — Task 7 should focus
  its comparison on the `ldbws-inferred` stretches, since that's the
  signal this whole feature is trying to improve on).

---

## Task 7: Manual spot-check comparison and write-up

**Depends on:** Tasks 4, 5, 6 all complete for the same day(s).

**Produces:** the actual empirical finding — a short written comparison,
not a scored pipeline or a new tool. This is deliberately manual/eyeball,
matching the design spec's own phrase, "a manual/spot-check TRUST-vs-
schedule read."

- [ ] **Step 1: For each chosen day, lay Task 5's "expected" table, Task
  4's "actual TRUST-reported" table, and Task 6's "what sampling actually
  reported" side by side.**

- [ ] **Step 2: For each real delay/cancellation Task 5 vs. Task 4 reveals
  at a specific TIPLOC**, ask two questions:
  1. **Did Task 6's sampling-derived line status reflect this at all?**
     (Recall sampling only sees the 3-5 curated `sample_stations` — a
     delay/cancellation at an intermediate TIPLOC Task 4's pinned trains
     passed through, but that isn't itself a sample station, is exactly
     the coverage gap the design spec's "segment precision" argument is
     about.)
  2. **If it did, was the *severity*/*location* sampling reported
     accurate**, or would knowing the specific affected TIPLOC (which
     Task 4's per-train event data can show, and `infer_from_samples`
     structurally cannot, per DESIGN.md §6.3) have been a materially
     better answer?

- [ ] **Step 3: Separately, note the reverse case honestly**: any
  stretch where sampling and TRUST-vs-schedule agreed, or where TRUST data
  added nothing sampling didn't already show (a real, permitted, and
  expected outcome for at least some of the spot-checked days — don't
  only report hits).

- [ ] **Step 4: Also note delay-*minute* accuracy**, even though the
  design spec already argued this is the weaker case (Darwin already
  fuses TRUST into something richer than a homegrown diff would produce):
  did the raw TRUST-vs-schedule delta ever diverge meaningfully from what
  Darwin/LDBWS reported for the same train at the same point? If it never
  meaningfully did, that's a real, confirming data point for the design
  spec's existing argument, not a new finding requiring more work.

- [ ] **Step 5: Write it up** — a few paragraphs plus the day-by-day
  tables: how many of the spot-checked days/segments showed TRUST
  revealing something sampling missed or under-attributed, versus how many
  showed no material difference. Be honest about sample size (a "handful"
  of days, dozens of trains — this is not a statistically powered study,
  and shouldn't be written up as one).

---

## Task 8: Decision gate — go/no-go recommendation

**Depends on:** Task 1 (licensing findings) and Task 7 (empirical
findings).

**Produces:** the actual deliverable of this entire plan — a short,
committed markdown document with a clear verdict, e.g.
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-results.md`,
following the same "research spec" precedent as the two existing documents
this plan is following up on (not committed as part of *this* planning
pass — this task describes what that follow-up document should contain
once the validation has actually run).

- [ ] **Step 1: State the licensing verdict plainly.** If Task 1 found
  either CIF SCHEDULE or CORPUS requires separate paid licensing or a slow
  manual-approval process, that alone is grounds to stay at "not yet" or
  move to "no" — say so, regardless of what Task 7 found, per the design
  spec's own explicit framing.

- [ ] **Step 2: State the empirical verdict concretely, not vaguely.**
  What counts as "go":
  - Segment-level TRUST inference (Task 4's per-train event data) would
    have caught or better-attributed a **clear majority** of the real
    spot-checked disruption instances that sampling (Task 6) missed or
    under-attributed — e.g., stated as "N of M spot-checked
    disruption-affected segments," not a vague impression.
  - AND Task 1's licensing findings don't independently disqualify
    proceeding.

  What counts as staying at **"not yet"**:
  - Licensing is fine, but the spot-check mostly showed sampling already
    caught what mattered (few or no real misses/under-attributions found)
    — the honest outcome if Darwin's existing fusion really is "good
    enough," matching the design spec's own weaker-case worry.
  - Too few real disruption days occurred during the monitoring window to
    say anything with any confidence — in which case the honest
    recommendation is "extend the monitoring window and re-run Task
    2-7," not a forced verdict either way.

  What counts as **"no"**:
  - Licensing turns out to be a hard blocker (paid tier this project
    won't fund, or an approval process with no realistic timeline) —
    independent of what the empirical side shows.

- [ ] **Step 3: If "go," name the next step explicitly**: a *new* planning
  pass scoped to Option B specifically (the design spec's own
  recommended architecture — a dedicated consumer service, not a
  `trust-consumer` extension), written the same way this plan's two
  parent specs were written — not started as a side effect of this
  validation pass.

- [ ] **Step 4: Commit only this findings document.** Nothing else from
  this plan's execution (throwaway scripts, raw data dumps, bulk-pin
  helper scripts) should be committed to the repository.
