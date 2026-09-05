# Design: A Windowed TRUST-Event Backlog for Late-Tracking Pins

**Status: design proposal, not approved. Spec stage only — no implementation
plan, no code, no migration in this pass.**

## Why this document exists, and what it is *not*

`docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md`
(read in full before this document was written) just designed a fix for a
real bug: a pin created after its train's one-shot origin-departure TRUST
Movement event has already flowed through and been consumed gets
permanently stuck at `resolution_status = 'pending'`. That fix is
**schedule-first**: resolve *which train this is* (its `train_uid`, its
booked stopping pattern) from CIF SCHEDULE data, which never expires and
needs no live event at all, with TRUST events still layered on top for
real-time status once/if they arrive.

The repo owner's own follow-up question, verbatim: *"would we not want to
keep a backlog of the key timings for trust events of all recent trains
(i'd say day) so that the first person to track a train still has the
tracking data for it even if it had departed before anybody had tracked
it. could we also look into how best to handle historical stores of data
like this so that this could be extended to a week or even a month (or
maybe even a year if storage permits)."*

**This is a related but genuinely different idea, not a restatement of
schedule-first, and this document treats it as such throughout:**

- **Schedule-first** answers "what train is this, and what was it *booked*
  to do" — from CIF data alone, true even if TRUST has said nothing at all,
  ever, about this specific service today.
- **This document** answers "what did TRUST *actually report* happening to
  this train — real departure/arrival times, real delay, a real
  cancellation" for a pin created after the fact, by retaining a windowed
  copy of TRUST's own observed events for trains nobody had pinned yet at
  the time those events arrived.

The two are complementary, not competing. Schedule-first is the fallback
for "nothing at all is known." A TRUST-event backlog is what lets a late
tracker see the real thing that happened, whenever it's still inside the
retention window, layered *on top of* whatever schedule-first has already
established — exactly the schedule-first document's own "schedule first,
TRUST layered on top" framing, just applied retroactively instead of live.

Required reading consumed in full before this document was written:
`crates/movement-relay/src/event_sink.rs`;
`docs/superpowers/specs/2026-09-04-movement-relay-design.md`;
`crates/api/migrations/20260828120000_train_tracking.sql`;
`crates/api/src/data/train_tracking.rs`;
`crates/trust-consumer/src/{config,matching,process}.rs`;
`crates/full-coverage-consumer/src/{correlate,stats}.rs`;
`docs/superpowers/specs/2026-09-05-mcp-deeper-api-integration-design.md`
(Decision 6 / Phase 3b, and the phased-scope section);
`docs/superpowers/plans/2026-09-01-ldbws-data-retention.md`;
`crates/aggregator/src/config.rs`;
`docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md`
(the just-merged design this one builds on top of).

## Current relevant state, grounded directly in code

### 1. The current Redis Streams cap falls far short of even one day, and re-verified against the same file the brief pointed at

`crates/movement-relay/src/event_sink.rs:18`: `const MAXLEN: usize =
500_000`, confirmed unchanged from the value the movement-relay design
document set. Its own comment (`event_sink.rs:13-17`) cites the sizing
rationale directly: *"~630k entries/day of real, cited average volume, N =
500,000 chosen as ~19h of full-volume headroom."*

Doing the math against the movement-relay design doc's own cited figures
(`docs/superpowers/specs/2026-09-04-movement-relay-design.md`, Decision 2:
"a real 9-day pipeline capture on the unfiltered national feed measured
~630k messages/day... ~7.3/s average"):

- `500,000 / 7.3 ≈ 68,493` seconds `≈ 19.03` hours — matches the doc's own
  "~19h" figure exactly, confirming the math holds today, not just at
  design time.
- To reach a genuine 24h of headroom at this same average rate:
  `7.3 × 86,400 ≈ 630,720` — i.e., `MAXLEN` would need to rise to roughly
  the *entire* national daily volume, since 500,000 was deliberately chosen
  to sit *below* a full day (the exact "generous cover for a redeploy
  window... not hours" reasoning in that doc, not "cover a day").
- The repo owner's own ask ("I'd say a day") **already exceeds what today's
  `MAXLEN` provides by roughly 5 hours** — confirming the brief's framing
  precisely: this is not "already basically covered," it is a real,
  measurable shortfall even at the *shortest* window asked for.

**Is bumping `MAXLEN` alone a viable lever, even just for a day?** Only
partially, and not for anything past a day, for two independent reasons:

- **Memory, not `MAXLEN` itself, is the real constraint**, and this is
  where this document adds a genuinely new finding the movement-relay
  design didn't need to make (its own Non-goals section flags Redis's
  *unsized, unbounded* single-instance posture,
  `charts/distant-signal/values.yaml:685-728`, as an already-known,
  already-unsolved gap — this document inherits that gap, doesn't invent
  it). No real measured per-entry payload byte size exists anywhere in
  this repo (an honest gap, flagged rather than invented, same posture the
  movement-relay design used for its own unmeasured peak-vs-average
  figure). A reasoned estimate: a TRUST envelope's JSON body plus header is
  typically a few hundred bytes; add Redis Streams' own per-entry internal
  overhead (rax/listpack structures) and a round, deliberately generous
  **~700 bytes/entry** is a plausible planning figure. At that estimate:
  - 1 day (~630k entries) ≈ **~440MB** of Redis memory for this stream
    alone.
  - 1 week (~4.4M entries) ≈ **~3.1GB**.
  - A month (~18.9M entries) ≈ **~13GB**.
  - A year (~230M entries) ≈ **~161GB**.

  Even the 1-day figure is a meaningful fraction of memory on an
  already-unsized single Redis instance that also serves
  `incident-text-changed` and every other cache/session use this app has;
  the week/month/year figures make clear that `MAXLEN` is **not** a lever
  that scales to what the repo owner is actually asking about — it hits a
  real memory wall at roughly the "day" tier already, confirming the
  brief's suspicion directly.
- **Redis Streams is also the wrong *shape* for this job, independent of
  memory.** A Stream has exactly one axis of lookup: entry ID (an
  approximately-time-ordered opaque cursor), read via `XRANGE`/
  `XREADGROUP`. There is no secondary index by CRS, by `train_uid`, or by
  scheduled time — answering "what happened at EUS around 19:15" requires
  scanning the whole retained range, which gets worse, not better, the
  longer the retention window is stretched. This is the actual query this
  design needs to serve (Decision 3, below); a Postgres table with real
  indexes is the right tool for it regardless of how long retention turns
  out to be, not merely for the longer tiers.

**Conclusion**: bumping `MAXLEN` to reach one real day is *technically*
possible and cheap in code (one constant), but is a real new memory cost on
an already-flagged-as-unsized dependency, and does not solve the actual
problem (arbitrary CRS+time lookup) even if the memory were free. This
document does not recommend it as a substitute for real persistence, though
it is a legitimate, free-standing, low-risk *interim* step worth doing on
its own merits regardless of whether this design is ever built (see
Non-goals).

### 2. `train_movement_events` is per-pin only, and confirmed to have zero pruning of any kind

`crates/api/migrations/20260828120000_train_tracking.sql:92-114`:
`train_movement_events.tracked_train_id BIGINT NOT NULL REFERENCES
tracked_trains(id)` — every row requires an existing pin. There is no path
by which an event for a train nobody has pinned yet can ever land in this
table. This confirms the brief's framing precisely: it is a **per-pin**
event log, not a universal backlog.

**Retention/pruning, re-verified directly, with one concrete bonus finding
beyond what the brief asked to confirm:**

- `crates/api/src/data/train_tracking.rs:27-30`'s own comment (about the
  sibling `MINE_LIST_LIMIT` constant): *"No retention or pruning job exists
  anywhere in this codebase for `tracked_trains`"* — grepped directly
  (`DELETE FROM tracked_trains`/`prune`/`expire`/`retention`), only
  `ON DELETE CASCADE` FKs and unrelated matches turned up.
- A direct grep for `DELETE FROM train_movement_events` across the entire
  repository returns **zero matches**. There is no prune job, scheduled or
  otherwise, for this table.
- **New finding, not previously documented**: `crates/trust-consumer/src/config.rs:63-66`
  declares a `retention_days: i64` config field (`default_value_t = 90`),
  with a doc comment that reads: *"How long to keep `train_movement_events`
  rows before pruning."* This field is **never read anywhere in the
  `trust-consumer` crate** beyond its own declaration — a grep for
  `config.retention_days`/`.retention_days` across every `.rs` file in
  `crates/trust-consumer/src` returns nothing but the declaration itself.
  There is no `prune_train_movement_events` function, no call site, no
  wiring of any kind. **This is dead configuration that implies a pruning
  behavior that does not exist** — a real, small, pre-existing
  documentation/code mismatch this document surfaces as a byproduct of its
  own research, worth fixing independently of whether this design is ever
  built (see Non-goals).

Net effect: `train_movement_events` genuinely does accumulate every event
for every ever-resolved pin, forever, today — this document's proposed new
table is not competing with an existing retention mechanism, because there
isn't one.

### 3. `full-coverage-consumer`'s precedent, and the MCP design's already-flagged Phase 3b — read directly, positioned precisely (Decision 6, below)

`crates/full-coverage-consumer/src/correlate.rs`'s `CorrelationState`
computes a `HashMap<(line_id, uid), DerivedState>` — real, TRUST-derived
per-train state for **every** scheduled service on a catalogued
full-coverage-enabled line, whether or not any user has pinned it. This is
confirmed, direct proof that "compute real per-train derived state for
trains nobody specifically pinned" already works in this codebase's
production shape. But `crates/full-coverage-consumer/src/stats.rs`'s
`build_line_row` folds all of that per-train detail down into **one
aggregated row per `(line_id, service_date)`**
(`FullCoverageLineStatsRow`, posted via `POST /private/full-coverage-stats`)
— the per-train `DerivedState` values themselves are discarded once that
fold happens; nothing persists them individually.

`docs/superpowers/specs/2026-09-05-mcp-deeper-api-integration-design.md`'s
Decision 6 ("Phase 3b") already names this exact discarded data as *"the
single biggest untapped data source"* and proposes a new table,
`full_coverage_train_state` — one row per `(line_id, uid, service_date)`,
**a snapshot, wholesale-replaced per cycle** ("following the same...
posture `station_full_coverage_samples` and `full_coverage_line_stats`
already established... a live snapshot, not an append log"), explicitly
recommending its own dedicated design pass, since its value is bounded by
how far `full_coverage_enabled` rollout actually extends (unmeasured, per
that document's own Open question 2) and it is "the biggest lift of
everything in [that] document."

**This document is not that Phase 3b design**, and does not attempt to be
— see Decision 6 for the full reasoning on how the two relate, argued
rather than assumed.

### 4. Storage sizing — done with real numbers, not guesses

Using the same real, cited national-feed figures from Decision 1 above
(`~630k messages/day`, `~7.3/s average`,
`2026-08-28-train-tracking-design.md:400-406` as re-cited by the
movement-relay design), and a **compact, per-event schema** (per the
brief's own instruction: CRS, event type, scheduled vs. actual timestamp,
delay/variation, no raw payload — sketched fully in Decision 2 below):

Estimating **~200 bytes/row all-in** (heap row + a couple of supporting
btree indexes) for a schema of this shape — a small `BIGSERIAL` id, two
short `TEXT` identifiers (`train_uid`, `train_id`), a `DATE`, a `CHAR(3)`
CRS, a short `TEXT` event-type, two `TIMESTAMPTZ` columns, a small
delay/variation field, and a `received_at` `TIMESTAMPTZ` — this is a
reasoned planning estimate, not a measured one (this repo has no comparable
existing table this size to benchmark against directly; flagged explicitly
as an estimate, same posture this repo's other unresearched sizing figures
use, e.g. `crates/movement-relay/src/event_sink.rs`'s own `MAXLEN`
comment):

| Retention window | Rows (national-feed volume) | Estimated size |
|---|---|---|
| 1 day | ~630,000 | ~126 MB |
| 1 week | ~4,410,000 | ~880 MB |
| 1 month (30d) | ~18,900,000 | ~3.8 GB |
| 1 year | ~229,950,000 | ~46 GB |

**Cross-checked honestly against this repo's own precedent for what counts
as "trivial" Postgres storage** — the language other specs actually use,
not a new standard invented here:

- `crates/aggregator/src/config.rs:56-66`'s `daily_stats_retention_days`
  doc comment and `docs/superpowers/plans/2026-09-01-ldbws-data-retention.md`
  cite **"~38k rows/year for the whole current catalogue"** as the
  reference "trivial" figure — repeated verbatim across at least four
  other specs (`2026-08-31-line-history-graphics-design.md:773`,
  `2026-09-02-trend-chart-granularity-design.md:412`,
  `2026-09-05-status-observability-page-design.md:400-403`,
  `2026-09-05-configurable-trend-granularity-design.md:274`).
- `crates/aggregator/src/config.rs:71`'s `half_hourly_stats_retention_hours`
  comment calls **~176,400 rows** (105 lines × 48 rows/day × 35 days)
  "trivial for Postgres."

**Every single tier above, including the shortest (one day, ~630k rows),
already exceeds this codebase's own prior "trivial" reference point
(176,400 rows) by roughly 3.5×, and exceeds the "~38k rows/year"
cross-catalogue figure by more than 16× — in a single day.** Being
precise and honest rather than reusing "trivial" language this data
doesn't earn:

- **1 day (~126MB, ~630k rows)**: a real, ordinary Postgres table size —
  not alarming, comparable in kind (if not exactly in row count) to the
  half-hourly precedent, and a genuinely bounded, one-time cost to
  provision for. Recommended as a reasonable, un-controversial starting
  tier.
- **1 week (~880MB, ~4.4M rows)**: a real step up from anything this
  codebase has called trivial before. Still an ordinary size for a
  dedicated Postgres table/index, but this document does **not** claim
  it's "trivial" the way `daily_stats`/`half_hourly_stats` are — it's a
  genuine, if modest, new operational commitment (index size, backup/
  restore time growing accordingly).
- **1 month (~3.8GB, ~18.9M rows)**: a real tradeoff conversation, not a
  rubber-stamp. Nothing in this repo currently hosts a table this large;
  whether the current Postgres instance is provisioned/sized for it is
  unconfirmed (same category of gap as movement-relay design's own
  flagged "Redis's unsized single-instance posture" — this document
  surfaces the equivalent question for Postgres at this tier, doesn't
  answer it).
- **1 year (~46GB, ~230M rows)**: **explicitly not trivial**, and this
  document does not pretend otherwise. This is a real infrastructure
  commitment on the same order of magnitude as this app's entire database
  today (unconfirmed exact current size, but every other table this repo
  has ever sized in a spec is in the thousands-to-low-hundred-thousands of
  rows, not hundreds of millions). Committing to this tier at full,
  undownsampled, national-feed fidelity should not happen without a real
  conversation with the repo owner — this document recommends **against**
  defaulting to it, not because a year of retention is a bad idea in
  principle, but because doing it at this fidelity/scope is a materially
  different cost than the "day" the repo owner explicitly anchored on.

**One honest mitigating factor these numbers don't yet account for**: the
table above uses the *national* feed volume, unfiltered. Decision 2 below
recommends scoping ingestion to catalogued-line CRSs only — the same
boundary the schedule-first design already established for
`schedule_line_population` — which would reduce every figure above by
however much of the national feed falls outside the 109 catalogued lines.
**That fraction is not measured anywhere in this repo** (the same kind of
gap the MCP design's own Phase 3b flags for `full_coverage_enabled`
rollout extent) — flagged explicitly as Open Question 1, not guessed at.
The numbers above are therefore an honest **ceiling**, not the expected
real cost; the real cost could plausibly be meaingfully smaller, but this
document does not invent a fraction to make the table look better than it
honestly can be shown to be.

### 5. Licensing — an existing citation exists, but it is secondhand and less precisely sourced than the LDBWS finding it sits next to

The brief asked this document not to assume TRUST licensing mirrors
LDBWS's cited 1-year cap, and to say plainly if no existing citation
resolves it. **A citation does exist, found directly, not assumed**:

`docs/superpowers/plans/2026-09-01-ldbws-data-retention.md:178-181`
(Step 4, discussing `train_movement_events`/`train_current_state`
directly): *"`trust-consumer` has its own `retention_days` config field,
default 90 days,
`crates/trust-consumer/src/config.rs:74-78`... but since **TRUST's own
licence is unrestricted per the audit**, this is not a compliance question
and is not investigated further here."* That same document's own opening
summary (lines 8-10) frames the underlying audit's general finding: *"Every
other checked licence said 'may retain any data received' — unrestricted"*
— i.e. TRUST (Train Movements) was one of the licences that audit checked
and found unrestricted, distinct from and by contrast to LDBWS's own
Schedule 1 §9 cap.

**Two real caveats on relying on this, stated plainly rather than treating
it as fully closed:**

1. **This is secondhand, not independently re-derivable today.** The
   `ldbws-data-retention.md` document itself says its audit's *"source
   PDFs [are] since deleted — its findings are taken as ground truth per
   this task's brief, not re-derived."* This document inherits that same
   secondhand status; there is no primary source in this repository this
   document (or any future reader) can re-check directly.
2. **The citation is markedly less precise than the LDBWS one it sits
   beside.** LDBWS's cap is quoted to an exact clause: *"Schedule 1 §9:
   'Must delete all data received within 1 year.'"* TRUST's is stated only
   as *"TRUST's own licence is unrestricted"* — no product name, schedule,
   or clause is cited. RDM's Train Movements product (`TRAIN_MVT_ALL_TOC`,
   confirmed as the actual product name in
   `docs/superpowers/specs/2026-09-04-movement-relay-design.md`'s opening
   paragraph) is presumably what "TRUST" refers to here, but that mapping
   is inferred, not quoted verbatim the way LDBWS's citation is.

**Conclusion, stated as the brief asked — plainly, not guessed at further**:
this document's best available evidence says TRUST/Train Movements
retention is currently unrestricted by licence, which is genuinely good
news for the week/month/year tiers the repo owner asked about — but that
evidence is secondhand and imprecisely sourced compared to the LDBWS
finding it sits next to. **Recommendation: before committing to any tier
longer than a few days in a real implementation, get a human to confirm
this against RDM's actual current Train Movements product terms
directly** — not because this document has reason to doubt the existing
citation, but because "unrestricted, per an audit whose source documents no
longer exist, described in one unquoted sentence" is a materially weaker
foundation than the quoted-clause standard this same repo already holds
itself to for LDBWS. This is a blocker for the **longer** tiers specifically
(week/month/year), not for the feature as a whole or for a short (day-ish)
window, which this document's own storage-cost findings (Decision 4) argue
against defaulting to at full national fidelity anyway, for cost reasons
independent of licensing.

## Decisions

### Decision 1: Yes — a global TRUST-event backlog is worth building, but it requires genuinely new persistence, not a bigger `MAXLEN`

Per Decision 1 of "Current relevant state": the current Redis Streams
`MAXLEN` (500,000) already falls ~5 hours short of even the repo owner's
shortest-asked tier ("a day"), bumping it to cover a day is a real new
memory cost on an already-unsized dependency and does not scale past a
day, and Redis Streams' single-axis ID lookup is the wrong shape for this
design's actual query need regardless. **This design requires a new
Postgres table**, fed by a new consumer reading the same
`movement-events` Redis Stream `movement-relay` already publishes to
(Decision 2).

### Decision 2: New table shape, new consumer, scoped like `schedule_line_population`

**New table**, tentatively `trust_event_backlog` (name not load-bearing,
left to implementation):

```sql
CREATE TABLE trust_event_backlog (
    id                 BIGSERIAL PRIMARY KEY,
    crs                TEXT NOT NULL,       -- best-effort STANOX->CRS
                                             -- translation, same table/
                                             -- posture as
                                             -- train_movement_events.loc_crs
    train_uid          TEXT,                -- NULL until an Activation
                                             -- for this train_id is seen
    train_id           TEXT,                -- TRUST's own daily id
    service_date       DATE NOT NULL,
    msg_type           TEXT NOT NULL,       -- '0001'..'0007', same
                                             -- confirmed-type coverage as
                                             -- movement-events (Decision 1,
                                             -- movement-relay design) --
                                             -- Activation/Cancellation
                                             -- included deliberately, not
                                             -- Movement-only (see below)
    event_type         TEXT,                -- ARRIVAL/DEPARTURE/PASS,
                                             -- Movement only
    planned_timestamp  TIMESTAMPTZ,
    actual_timestamp   TIMESTAMPTZ,
    variation_status   TEXT,
    received_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
    -- deliberately NO raw_body JSONB -- the brief's own instruction, and
    -- the single biggest fidelity difference vs. train_movement_events.
    -- Named as a real, deliberate scope-narrowing tradeoff, not an
    -- oversight: this repo's own convention elsewhere (movement-relay
    -- design's Decision 1, on why it forwards raw bytes rather than a
    -- typed re-encoding) is "never lose a field neither consumer has
    -- modeled yet" -- this table knowingly does lose that property, in
    -- exchange for roughly an order of magnitude smaller rows at the
    -- national-feed volumes Decision 4 computes. Recommend an explicit
    -- comment at this column's absence saying so, so a future reader
    -- doesn't "fix" it by adding raw_body back without re-running
    -- Decision 4's sizing math against the much bigger number that would
    -- produce.
);

CREATE INDEX trust_event_backlog_crs_time
    ON trust_event_backlog (crs, planned_timestamp);
CREATE INDEX trust_event_backlog_train
    ON trust_event_backlog (train_uid, service_date);
```

**All five confirmed TRUST types are kept, not just Movement** — this is a
deliberate choice, not the default. Activation carries the `train_uid`
that a CRS+time-only backlog lookup (Decision 3, step 2) needs to
discover in the first place; Cancellation is what lets a late tracker see
"this train was cancelled" rather than silence, which is exactly the kind
of real-thing-that-happened data this feature exists to surface (a
schedule-matched-only pin, per the schedule-first design's own Decision 6,
cannot distinguish "cancelled" from "just hasn't reported yet" — a backlog
Cancellation event closes that gap).

**New consumer, not a change to `movement-relay` or `trust-consumer`
itself.** `movement-relay`'s own scope is transport, stated explicitly in
its design (Decision 1: "raw passthrough... classify... forward"); adding
persistence logic to it would be scope creep on an already-reviewed,
narrowly-justified service. `trust-consumer`'s own module doc is equally
explicit that it is *"filtered to exactly the currently user-tracked
`(train_uid, date)` set"* — writing catalogued-line-wide, unfiltered rows
from inside it would silently break that stated boundary, the same kind of
boundary-crossing the schedule-first design flagged as a real (if
justified) decision when `api` gained a `schedule_query` dependency, not
something to do quietly as a side effect.

The natural home is a **third named consumer group on the existing
`movement-events` Redis Stream** — `movement-relay`'s own Decision 2
already establishes "two consumer groups... `trust-consumer` and
`full-coverage-consumer`," and nothing about that design closes the door
on a third. The shared `crates/movement-feed` crate (`RedisStreamMovementFeed`,
startup-PEL-replay, `XAUTOCLAIM` sweep — all from the movement-relay
design's Decision 3) already generalizes to this with **zero change** —
only a new `(stream, group, consumer)` triple at construction time, exactly
the same "only a call-site change" property that design already proved for
`trust-consumer`/`full-coverage-consumer`. Whether this third consumer is a
genuinely new binary crate or a new mode of an existing one is not decided
here (Open Question 3).

**Scoping: catalogued-line CRSs only, mirroring the schedule-first
design's own boundary exactly, not the full national feed.** The
schedule-first design's Decision 2 builds a `crs -> Vec<line_id>` reverse
index over every catalogued line's TIPLOC-bearing stations specifically so
`api` can answer "does this pin's origin CRS have any schedule data at
all." A backlog entry for a CRS with **no** catalogued-line match is
useless for exactly the same reason a pin there has no schedule-first path
either (per that document's own Decision 1: "for a pin whose origin CRS is
on no catalogued line at all, there is no usable schedule data source
today"). Reusing that same reverse index to filter this new consumer's
writes keeps both designs' scope boundaries honestly aligned, rather than
this document inventing a second, differently-scoped notion of "which
stations matter." This is also what makes Decision 4's storage numbers a
genuine ceiling rather than the expected cost (Open Question 1).

### Decision 3: The concrete late-tracking-pin flow, and how it interacts with `schedule_matched`

Walking through precisely, per the brief's own request:

1. **Pin created**, at time T, for `pin_origin_crs` + `pin_scheduled_departure`
   already in the past (the exact "tracked after the fact" case). Existing
   flows run first, unmodified: live TRUST matching (already missed, that's
   why this pin needs this feature at all) and, per the already-merged
   schedule-first design, a schedule match attempt against
   `schedule_line_population` — if a candidate line/service is found,
   `train_uid` is set and `resolution_status` moves to `'schedule_matched'`.
2. **New backlog lookup, run alongside (not instead of) the schedule
   match.** Query `trust_event_backlog` by `(crs = pin_origin_crs, planned_timestamp
   BETWEEN pin_scheduled_departure - MATCH_TOLERANCE AND pin_scheduled_departure
   + MATCH_TOLERANCE)` — reusing `matching.rs:23`'s existing
   `MATCH_TOLERANCE` (±20 minutes) constant/value, the same "reuse the
   value, not necessarily the crate" posture the schedule-first design
   already recommended for its own equivalent reuse. If this returns rows
   carrying a `train_uid` (directly, or via an Activation row for the same
   `train_id`), that `train_uid` is authoritative — a real, observed TRUST
   match, strictly stronger evidence than a schedule-only guess.
3. **Full backfill, once `train_uid` is known** (whether from step 2
   directly, or already known from step 1's schedule match): a second
   query, `trust_event_backlog WHERE train_uid = $1 AND service_date = $2
   ORDER BY received_at` — the entire observed history for this train, not
   just the origin event. Not gated on `crs` this time, since the point is
   the whole journey, not just where the pin happened to be created.
4. **Replay, not a special-cased bulk insert.** For each backfilled row, in
   `received_at` order, call the *same* `upsert_train_event` path a live
   event would have taken (constructing the equivalent
   `TrainMovementEventMessage` shape from the backlog row — `raw_body`
   necessarily becomes an empty/placeholder JSON value, since the backlog
   never kept it; a real, named fidelity loss vs. a live-processed event,
   flagged here rather than glossed over). This is a deliberate design
   choice, not a shortcut: it means `train_movement_events` and
   `train_current_state` end up in *exactly* the state a live-watching
   trust-consumer would have left them in, because the same idempotent,
   already-tested write path (`train_tracking.rs:394-462`,
   `ON CONFLICT (tracked_train_id, dedup_key) DO NOTHING`) is doing the
   writing — no new, parallel "backfill-shaped" write logic to
   independently get right and keep in sync with the live path forever.
5. **`resolution_status`, reasoned through precisely, per the brief's own
   question**: replaying real `upsert_train_event` calls means
   `resolution_status` reaches `'resolved'` **directly**, not
   `'schedule_matched'`, whenever a backfilled row carries the two fields
   that function's existing guard checks (`resolved_train_uid` +
   `resolved_train_id` — an Activation event, in backlog terms). This is
   the correct outcome, not an accident of reusing the write path: per the
   schedule-first design's own Decision 4, `'schedule_matched'` means
   specifically *"we know which physical service this is... TRUST has not
   yet confirmed anything live about it"* — a backlog hit is the opposite
   of that: real TRUST data, confirming real things, already exists and is
   being read right now. Leaving a backlog-resolved pin sitting at
   `'schedule_matched'` would be actively misleading, understating what's
   genuinely known. `'resolved'` is the honest, correct status the moment
   real observed TRUST data is available, live or backfilled — this
   document does not want a user-visible distinction between "TRUST told us
   live" and "TRUST told us five minutes after the fact"; both are real.
6. **A real, honest coupling to inherit, not invent**: if the backlog holds
   Movement events for this `train_id` but **no** Activation (e.g., the
   Activation itself fell outside the retention window, or predates this
   consumer's own deployment), `upsert_train_event`'s current two-field
   guard (`train_tracking.rs:400-412`) will not advance
   `resolution_status` past `'schedule_matched'`even though real Movement
   data is being written into `train_movement_events`/`train_current_state`
   right now — **exactly the gap the schedule-first design's own Decision 5
   already names and proposes fixing** (relax the guard to key off
   `resolved_train_id.is_some()` alone). This document does not re-litigate
   that decision; it simply confirms the same companion fix is a real
   prerequisite for this design to fully deliver its own stated benefit in
   this specific case, the same way it already was for schedule-first's
   own live-matching path. Sequencing between the two designs' companion
   fixes is Open Question 4.
7. **Ongoing behavior after backfill**: nothing new needed. A backfilled
   pin is a normal `tracked_trains` row afterward — if the backlog's last
   event already shows `status = 'completed'`/`'cancelled'`,
   `list_active_tracked_trains`'s existing `WHERE` clause
   (`train_tracking.rs:376-378`) already excludes it from further live
   matching, unchanged. If the train was still en route as of the backlog's
   most recent entry, the pin continues to receive live TRUST events
   exactly as any other tracked train does — no new logic needed here
   either.
8. **No match in the backlog at all** (train_uid unknown from schedule
   match too, or a schedule match exists but the backlog has nothing for
   it — e.g. the backlog's own retention window has already rolled past
   this service_date): the pin is left exactly as it would be today under
   the schedule-first design alone — `'pending'` or `'schedule_matched'`,
   whichever already applies, no regression, no new failure mode
   introduced.

### Decision 4: Storage-cost tiers and recommendation

Restating Decision 4/"Current relevant state" as an explicit
recommendation, since the brief asked for one:

- **1 day**: recommended as the safe, unambiguous starting tier — real,
  ordinary Postgres size (~126MB, ~630k rows at national-feed ceiling
  volume, likely smaller once catalogued-line scoping is measured), and it
  is literally what the repo owner asked for first. No tradeoff
  conversation needed to justify this tier alone.
- **1 week**: a reasonable second step (~880MB) — no longer "trivial" by
  this repo's own established language, but not a hard sell either; worth
  doing once the day tier is live and the real (not ceiling) volume is
  measured.
- **1 month**: a genuine tradeoff conversation, not a default — ~3.8GB is
  real infrastructure the repo owner should explicitly sign off on, ideally
  after Postgres instance sizing/backup posture at that scale is confirmed,
  not assumed.
- **1 year at full, undownsampled, national-feed fidelity**: **not
  recommended as designed here.** ~46GB (national ceiling; smaller if
  scoped, per Open Question 1, but not confirmed smaller) is a step change
  this repo has never taken on for any table. If a year (or longer) is
  genuinely wanted, the honest path — mirroring this exact codebase's own
  precedent, `line_status_history` (7-day full fidelity, pruned) feeding
  `line_status_daily_stats` (aggregated, much longer retention) — is a
  **two-tier retention model**: full per-event fidelity for a short window
  (a day or a week), rolling off into a coarser, smaller per-train summary
  (first/last event, final status, total delay — not every intermediate
  calling-point event) for anything older, not a single table holding
  everything at full resolution for a year. This document does not design
  that downsampled shape in full (Open Question 5) but names it as the
  correct direction rather than leaving "just retain everything for a
  year" unchallenged.

### Decision 5: Licensing — a real citation exists but is weaker than the LDBWS one, a blocker for the longer tiers specifically

Per "Current relevant state" #5: this document found and cites an existing
statement that TRUST/Train Movements retention is unrestricted by licence,
but flags it as secondhand and imprecisely sourced relative to the LDBWS
finding it sits beside. **This is stated as a blocker for the week/month/
year tiers specifically, not the feature as a whole** — the 1-day tier
(Decision 4's recommended starting point) is uncontroversial under either
reading of the evidence (even LDBWS's own strict 1-year cap wouldn't touch
a 1-day retention window), so implementation of the day tier need not wait
on re-confirming this. Anything longer should get an explicit human check
against RDM's actual current Train Movements product terms first.

### Decision 6: Positioning relative to the MCP design's Phase 3b — genuinely different scope and shape, sharing an upstream opportunity, argued rather than assumed

Both this document and Phase 3b were independently flagged, in sibling
documents, as "the biggest lift"/"the single biggest untapped data
source" — worth being precise about whether they're the same idea seen
from two angles, or genuinely different, rather than assuming either
answer.

**They are genuinely different, on two independent axes:**

1. **Different data shape.** Phase 3b's `full_coverage_train_state` is
   explicitly a **snapshot** — *"one row per `(line_id, uid,
   service_date)`... wholesale-replaced per cycle, not an append log"*
   (the MCP design's own Decision 6, citing `correlate.rs`'s own
   `DerivedState` fields as "last-write-wins per event, not additive").
   This design's `trust_event_backlog` is explicitly an **append-only
   event log**, keyed by time, meant to answer "what happened, in order,
   over some historical window" — a question a snapshot table cannot
   answer at all, by construction: once a later Movement event overwrites
   an earlier one's fields in a snapshot row, there is no way to recover
   what the 08:03 arrival at an intermediate stop actually reported. These
   are not two names for the same table.
2. **Different scope boundary.** Phase 3b is explicitly bounded to
   `full_coverage_enabled` lines — a strict, currently-unmeasured
   *subset* of the 109 catalogued lines (the MCP design's own Open
   question 2: "how many lines full-coverage rollout actually covers today
   ... was not measured"). This document's Decision 2 scopes to **every**
   catalogued line, matching the schedule-first design's own generalized
   reverse index exactly (built "over every catalogued line's stations,
   not just full-coverage ones" — that document's own Decision 2,
   explicit about the distinction). A backlog scoped only to
   `full_coverage_enabled` lines would be narrower than what late-tracking
   pins actually need, since a pin can resolve via schedule-first on *any*
   catalogued line, not just full-coverage ones.
3. **Different retention posture.** Phase 3b, as sketched, proposes no
   pruning at all — a snapshot table's row count is naturally bounded by
   "one row per train per day," not by a retention window, so its own
   design doesn't need one. This document's entire reason for existing
   *is* a retention-window question (Decisions 1/4) — a concern Phase 3b's
   own shape simply doesn't have.

**Where they genuinely do overlap, and what that overlap should mean for
whoever designs Phase 3b next**: both need essentially the same upstream
work — translating a TRUST Movement's `loc_stanox` to a CRS, matching it
against a catalogued line's population, and folding it into a per-train
derived state — the exact computation `full-coverage-consumer`'s
`correlate.rs` already does today, just discarded before persistence.
**Recommendation, not a design of Phase 3b itself**: when Phase 3b gets its
own dedicated design pass (already called for by the MCP integration
document), that pass should read this document and consider whether a
single new correlation consumer — built once, reading the third Redis
Streams consumer group this document's Decision 2 proposes — should feed
**both** outputs from one shared pass over the same population data: Phase
3b's per-`(line_id, uid, service_date)` snapshot table, and this document's
append-only backlog table. That would avoid two independently-built
consumers each re-solving the same STANOX→CRS→TIPLOC→population matching
problem. This document does not do that unification itself — it is
explicitly the wrong place to make Phase 3b's own scoping calls (rollout
measurement, its own new ingest/read routes) — but it would be a real
missed opportunity for whoever writes that follow-up not to at least
consider a shared producer, given how much of the matching plumbing is
identical.

## Explicitly out of scope (Non-goals)

- **Designing Phase 3b itself.** Positioned relative to it (Decision 6),
  not designed, scoped, or measured here — that remains its own,
  separately-recommended follow-up.
- **Any migration file or Rust code.** Design spec only, per this task's
  brief.
- **Deciding the exact retention-day number for any tier.** Decision 4
  argues which tiers are cheap/reasonable/need-a-conversation, but does not
  pick a final number — that is a product decision for the repo owner,
  informed by these figures.
- **Measuring what fraction of the national ~630k/day feed actually falls
  on catalogued-line CRSs.** Flagged as Open Question 1 — the numbers in
  Decision 4 are an honest ceiling, not a measured real cost.
- **Designing the downsampled/coarser long-term summary shape** Decision 4
  names as the right direction for month/year retention. Flagged as Open
  Question 5, not designed here.
- **Resolving the TRUST/RDM Train Movements licensing question with
  certainty.** An existing citation is reported and reasoned about
  honestly (Decision 5); getting a definitive answer requires a human
  checking RDM's actual current product terms, not further code research.
- **Fixing `trust-consumer`'s dead `retention_days` config field**
  (Decision 2/"Current relevant state" #2's bonus finding). Named because
  it was found doing this document's own research, not because fixing it
  is this document's job — a trivial, independent cleanup for whoever picks
  it up.
- **Committing to a `MAXLEN` bump on `movement-events`** as an interim
  step. Legitimate and cheap on its own merits (Decision 1), but not part
  of this design, and not a substitute for it — left for the repo owner to
  decide independently.

## Open questions / risks

1. **What fraction of the national ~630k/day TRUST feed actually falls on
   catalogued-line CRSs is unmeasured.** Decision 4's storage figures are
   an honest ceiling assuming full national capture; the real cost under
   Decision 2's recommended catalogued-line-only scoping could be
   meaningfully smaller, but by an unmeasured amount. Same category of gap
   as the MCP design's own unmeasured full-coverage rollout extent.
2. **The TRUST/Train Movements licensing citation (Decision 5) is
   secondhand and less precisely sourced than the LDBWS one it sits
   beside** — a human should confirm it against RDM's actual current
   product terms before committing to any tier longer than a few days,
   even though this document's own reading of the existing evidence is
   favorable.
3. **Whether the new third consumer (Decision 2) should be a wholly new
   binary crate, or a new mode of an existing one** (a natural candidate
   given Decision 6's shared-producer recommendation, but not decided
   here) — an implementation-time call.
4. **Sequencing between this design's Decision 3/step 6 dependency on the
   schedule-first design's own Decision 5 (the `upsert_train_event`
   two-field guard relaxation)** — both designs independently need the same
   companion fix to fully deliver their stated benefit in the
   Activation-missing case; which lands first, or whether they should ship
   together, is a real implementation-planning question, not resolved
   here.
5. **The downsampled/coarser schema for month/year-tier retention**
   (Decision 4's recommended two-tier model, mirroring
   `line_status_history`/`line_status_daily_stats`) is named as the right
   direction but not designed in field-level detail here.
6. **Redis's own unsized, unbounded single-instance memory posture**
   (already flagged as a non-goal in the movement-relay design) becomes
   directly relevant again if a `MAXLEN` bump is ever used as an interim
   day-scale step (Decision 1) — not a new gap this document introduces,
   but worth linking explicitly since this document's own math depends on
   it.

## Summary answers to the brief's specific questions

- **Is a global TRUST-event backlog, retained for at least a day, a good
  idea given what exists today?** Yes for the concept; no, `MAXLEN` alone
  cannot deliver it — the current Redis Streams cap already falls ~5 hours
  short of even a day, bumping it further hits a real, already-flagged
  memory constraint at roughly the day tier, and Redis Streams' single-axis
  lookup is the wrong shape for the CRS+time/`train_uid` queries this
  feature actually needs regardless of retention length. A new Postgres
  table, fed by a new consumer group on the existing `movement-events`
  stream, is the real answer (Decisions 1-2).
- **How does a late-tracking pin actually consume it?** Walked through
  concretely in Decision 3 — a CRS+time lookup to discover `train_uid` (or
  confirm a schedule match), a full `train_uid`+`service_date` backfill
  query, replayed through the existing `upsert_train_event` path so
  `train_movement_events`/`train_current_state`/`resolution_status` all
  land exactly where a live-watching consumer would have left them —
  reaching `'resolved'` directly, not staying at `'schedule_matched'`,
  whenever real backfilled data includes both `train_uid` and `train_id`
  (or `train_id` alone, once the schedule-first design's own Decision 5
  companion fix lands).
- **Real storage-cost numbers per tier?** Day ~126MB/~630k rows, week
  ~880MB/~4.4M rows, month ~3.8GB/~18.9M rows, year ~46GB/~230M rows, at
  national-feed ceiling volume (Decision 4) — day is uncontroversial, week
  is reasonable, month needs a real conversation, year at full fidelity is
  explicitly not recommended without downsampling or narrower scoping.
- **Licensing?** An existing citation says TRUST/Train Movements retention
  is unrestricted, found directly in this repo — but it is secondhand and
  markedly less precisely sourced than the LDBWS 1-year finding it sits
  beside, and should be independently reconfirmed before committing to any
  tier longer than a few days (Decision 5).
- **Relationship to the MCP design's Phase 3b?** Genuinely different in
  data shape (append-only log vs. wholesale-replaced snapshot) and scope
  boundary (all catalogued lines vs. `full_coverage_enabled` lines only),
  not a restatement or a subset of it — but both need the same upstream
  STANOX/TIPLOC/population matching, so a future Phase 3b design pass
  should seriously consider a single shared correlation consumer feeding
  both outputs, rather than building that plumbing twice (Decision 6).
