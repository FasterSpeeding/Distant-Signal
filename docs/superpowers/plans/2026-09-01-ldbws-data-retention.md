# Live Departure Board (LDBWS) Data Retention — Findings + Remediation Plan

> **Status: this closes out a specific deferred item from this session's
> licence audit, not a fresh investigation.** The audit (source PDFs since
> deleted — its findings are taken as ground truth per this task's brief,
> not re-derived) found exactly one RDM feed licence with a binding
> retention limit: the **"Live Departure Board"** product (National Rail
> Enquiries feed, Data Publisher Rail Delivery Group), Schedule 1 §9:
> **"Must delete all data received within 1 year."** Every other checked
> licence said "may retain any data received" — unrestricted. The audit's
> first pass flagged, but explicitly could not resolve, whether
> `tracked_train_tickets` retains Live-Departure-Board-sourced data
> indefinitely, and deferred it as "needs someone with the actual table
> schema to check." This document is that check, done against the real
> schema and code, plus a full trace of every other table that could
> plausibly hold Live-Departure-Board-derived content.
>
> **Bottom line:** the audit's own flagged concern about
> `tracked_train_tickets` was a false lead — confirmed below, not just
> re-asserted. There is exactly **one** real, confirmed compliance-relevant
> gap in this codebase, and it is a different table entirely:
> `line_status_daily_stats`, which is fed by data traced back to
> Live-Departure-Board samples and, in the actually-deployed configuration,
> has **no pruning wired up at all** — rows accumulate indefinitely. This
> document reports that finding with full citations and proposes a
> remediation, but does **not** make the underlying legal call itself (see
> "Human sign-off needed," below) — that finding is a genuine open
> ambiguity, not a deferral of research that could have been resolved by
> reading more code.

---

## Step 1: what "Live Departure Board" data actually is in this app

Two crates could plausibly be "the LDBWS feed": `poller-ldbws` and
`poller-stations`. They are **not the same RDM product**:

- `crates/poller-ldbws/src/main.rs:1-16` (module doc): "samples live
  departure-board data for every station any line's inference logic
  depends on, and forwards parsed `StationSample`s to the `api` crate's
  `/private/station-samples` ingestion endpoint... a documentation-
  discovery pass against a fetched Swagger spec for RDM's **Live Departure
  Board REST product**, `GetDepBoardWithDetails`." This is confirmed
  elsewhere in the repo too:
  `docs/superpowers/plans/2026-07-06-ldbws-sampler-poller.md:26` — "RDM's
  Live Departure Board product" — and
  `docs/superpowers/specs/2026-07-06-ldbws-sampler-poller-design.md:6` —
  "LDBWS — Live Departure Board Web Service, the Darwin real-time feed."
  **This is the feed the audit's flagged licence covers.**
- `crates/poller-stations/src/main.rs:1-9` (module doc): "polls the **RDM
  Stations JSON feed**... forwards parsed `StationReference`s to the `api`
  crate's `/private/stations` ingestion endpoint... RSPS5050 P-03-00 Rev
  A, §6." This is a wholly separate RDM product (static station reference
  data — names, CRS codes — not live departures) and is out of scope for
  the Live Departure Board licence.

`poller-ldbws` writes to the database via a chain confirmed by direct
grep and read:
- `crates/api/src/routes/ingest.rs:40-41,121-127` mounts
  `POST /private/station-samples` → `queries::post_station_samples`.
- `crates/api/src/data/queries.rs:258-285` (`upsert_station_samples`) is
  the actual write path.
- The target table is `station_samples`
  (`crates/api/migrations/20260510023522_initial.sql:52-56`).

**This confirms the audit's own implicit assumption**: Live-Departure-
Board data enters this app's database through exactly one table,
`station_samples`, and nowhere else directly.

---

## Step 2: retention shape of `station_samples` itself

`crates/api/src/data/queries.rs:255-257` (doc comment on
`upsert_station_samples`): *"Upserts a batch of station samples (LDBWS
departure-board snapshots). No history — this is a point-in-time sample,
wholesale-replaced per poll, same rationale as
`upsert_stations`/`upsert_tocs`."*

Confirmed by the actual query (`queries.rs:265-278`):

```sql
INSERT INTO station_samples (crs, polled_at, departures)
VALUES ($1, $2, $3)
ON CONFLICT (crs) DO UPDATE SET
    polled_at  = EXCLUDED.polled_at,
    departures = EXCLUDED.departures
```

And the schema (`crates/api/migrations/20260510023522_initial.sql:52-56`):

```sql
CREATE TABLE station_samples (
    crs        CHAR(3)     PRIMARY KEY,
    polled_at  TIMESTAMPTZ NOT NULL,
    departures JSONB       NOT NULL DEFAULT '[]'
);
```

`crs` is the primary key — one row per station, always overwritten
in-place on the next poll cycle (60s default,
`crates/poller-ldbws/src/config.rs` / `docs/superpowers/specs/2026-07-06-ldbws-sampler-poller-design.md:6`
area). **`station_samples` never accumulates history at all — it is
ephemeral by construction.** There is no pruning job because none is
needed: the table's row count is bounded by station count, not by time,
and last night's departures are gone the moment tomorrow morning's poll
lands. This table poses no retention risk under the 1-year limit.

---

## Step 3: `tracked_train_tickets` — the audit's flagged table, resolved

`crates/api/migrations/20260829090000_journey_ticket_tracking.sql:26-46`
defines the table exactly as the task brief described: `operator`,
`ticket_type`, `origin_crs`, `destination_crs`, `source`, plus ownership/
timestamp columns. Its own migration comment
(lines 8-14) is explicit that this is a deliberately minimal column set,
audited for privacy reasons already, unrelated to this retention
question. The `source` check constraint
(line 44) permits exactly four values: `'manual'`, `'pkpass-semantics'`,
`'pkpass-heuristic'`, `'pdf-heuristic'`. Tracing each:

- **`manual`**: `crates/api/src/routes/train.rs:73-91` (`post_ticket`)
  takes `Json(entry): Json<TicketEntryRequest>` — the request body a
  client (the frontend, on behalf of the user) POSTs directly —
  validates it (`train_tracking::validate_ticket_entry`), checks
  ownership, and calls `train_tracking::create_ticket` (line 86), which
  (`crates/api/src/data/train_tracking.rs:457-475`) inserts `operator`,
  `ticket_type`, `origin_crs`, `destination_crs`, `source` **verbatim
  from that request body** — no join, no lookup, no call to
  `station_samples` or any LDBWS-derived data anywhere in this path. This
  is the user's own typed input.
- **`pkpass-semantics` / `pkpass-heuristic` / `pdf-heuristic`**:
  `crates/api/src/data/ticket_extraction.rs:1-7` (module doc): *"reads
  openly-documented file formats a user already has (Apple Wallet
  `.pkpass`, PDF e-tickets) and returns a `PartialTicket` preview — this
  module and every function in it NEVER writes to the database... and
  NEVER decodes a barcode or touches ITSO data."* A grep of this file for
  `reqwest|http|ldbws|station_samples|departure` (the network/feed
  vocabulary that would indicate a live API call) turns up **zero
  network-call sites** — every hit is either a doc comment or a literal
  JSON field name (`departureStationName`) being read out of a
  user-uploaded file's already-parsed content
  (`ticket_extraction.rs:68`). Per the module doc, this code path never
  writes to the database at all — a `PartialTicket` is returned to the
  frontend as a pre-fill suggestion, and only becomes a real row if the
  user reviews and re-submits it through the same `post_ticket` /
  `create_ticket` path above (confirmed by the migration's own comment,
  `20260829090000_journey_ticket_tracking.sql:16-21`: *"'pkpass-semantics'
  / 'pkpass-heuristic' / 'pdf-heuristic' are all pre-fills the user
  reviewed and explicitly confirmed via a manual-entry POST before this
  row existed — confirmation, not the parse itself, is what makes the row
  trustworthy."*).

**Verdict: confirmed, not assumed — `tracked_train_tickets` has no code
path, of any kind, by which Live-Departure-Board API data can reach it.**
Both of its data sources (manual user entry, and parsing of a
user-uploaded `.pkpass`/PDF file) are entirely independent of the
Live Departure Board feed. The audit's flagged concern about this table
was a false lead based on an incomplete trace; it can be closed with no
further action.

---

## Step 4: `train_current_state` / `train_movement_events` — a different feed, confirmed

`crates/trust-consumer/src/main.rs:1-5` (module doc): *"persistent Kafka
consumer for **Network Rail's TRUST Train Movements feed** (via RDM),
filtered to exactly the currently user-tracked `(train_uid, date)`
set."* This is RDM's Train Movements product, publisher Network Rail
Infrastructure Limited (per this session's audit background) — a wholly
separate product, licence, and poller from Live Departure Board /
`poller-ldbws`. The tables it writes
(`crates/api/migrations/20260828120000_train_tracking.sql:92,119`,
`train_movement_events` and `train_current_state`) are out of scope for
the Live Departure Board licence's retention clause. (For completeness:
`trust-consumer` has its own `retention_days` config field, default 90
days, `crates/trust-consumer/src/config.rs:74-78`, doc comment noting
`tracked_trains`/`train_current_state` are kept indefinitely by design —
but since TRUST's own licence is unrestricted per the audit, this is not
a compliance question and is not investigated further here.)

`tracked_trains` itself (the pin record `tracked_train_tickets`
references) is also user-submitted, not LDBWS-derived: its
`pin_origin_crs`/`pin_destination_crs`/`pin_operator` columns come from
`TrackPinRequest`, the JSON body of the user-initiated
`POST /Train/track` (`crates/api/src/data/train_tracking.rs:41-70`) — the
same "client POST body written verbatim" shape as ticket entry, not a
feed ingestion path.

---

## Step 5: the one table that genuinely does carry Live-Departure-Board-derived content long-term

`line_status`, `line_status_history`, and `line_status_daily_stats` are
all aggregator output, and all trace back to `station_samples` through
`crates/aggregator/src/aggregation.rs` (`infer_from_samples`,
`compute_sample_stats`, both operating on `&[StationSample]` fetched from
`station_samples`). Checking each for retention shape:

- **`line_status`**
  (`crates/api/migrations/20260510023522_initial.sql:69-78`): *"one row
  per line, fully replaced each aggregation cycle."* `line_id` is the
  primary key. Ephemeral, same shape as `station_samples`. No gap.
- **`line_status_history`**
  (`crates/api/migrations/20260510023522_initial.sql:89-96`): append-only,
  but **actively pruned**. This is the reference implementation the task
  brief pointed at:
  `crates/aggregator/src/queries.rs:312-315` (`prune_history`):
  ```sql
  DELETE FROM line_status_history WHERE computed_at < NOW() - ($1 || ' days')::interval
  ```
  wired into the real poll loop at `crates/aggregator/src/main.rs:105`
  (`queries::prune_history(pool, retention_days)`, called unconditionally
  every cycle — not gated behind an `Option`). The retention window is a
  required CLI/env arg with a default,
  `crates/aggregator/src/config.rs:52-54`
  (`history_retention_days`, `default_value_t = 7`), and the deployed
  Helm chart sets it explicitly:
  `charts/distant-signal/values.yaml:484-487`
  (`aggregator.historyRetentionDays: 7`), wired to the container env at
  `charts/distant-signal/templates/aggregator-deployment.yaml:79-80`
  (`HISTORY_RETENTION_DAYS`). **7 days, always pruned, always configured
  — nowhere near the 1-year limit.** No gap.
- **`line_status_daily_stats`** — see below. **This is the real finding.**

---

## The finding: `line_status_daily_stats` accumulates indefinitely in production

`line_status_daily_stats`
(`crates/api/migrations/20260831090001_line_status_daily_stats.sql:33-56`)
is a daily rollup, one row per `(line_id, day)`, of summed counts:
`sample_cycles`, `total`, `delayed`, `cancelled`, `skipped`,
`running_count`, plus a weighted delay-minutes sum. It is fed exclusively
by `dedup::dedup_new_sample_stats`
(`crates/aggregator/src/dedup.rs:167-174`), which operates on
`samples: &[StationSample]` — i.e., directly on Live-Departure-Board
data — and is written via `queries::record_daily_stats`, called every
cycle at `crates/aggregator/src/main.rs:121-126`. `SampleStats` itself
(`crates/common/src/lib.rs:670-673`, doc comment): *"...computed... from
LDBWS `StationSample`s..."* — explicit, in-repo confirmation of
provenance.

**Retention is unconfigured in the deployed chart, and the code default
is "never prune":**

- `crates/aggregator/src/config.rs:56-66` (`daily_stats_retention_days`):
  `Option<i64>`, **no `default_value_t`** — defaults to `None`. The doc
  comment says so explicitly: *"Deliberately `Option`, defaulting to
  `None` (no pruning at all)... the real retention ceiling is an
  unresolved product decision... until then rows accumulate
  indefinitely."*
- `crates/aggregator/src/main.rs:130-134`:
  ```rust
  let daily_stats_pruned = if let Some(retention) = daily_stats_retention_days {
      queries::prune_daily_stats(pool, retention).await?
  } else {
      0
  };
  ```
  When unset, the prune function (`crates/aggregator/src/queries.rs:398`,
  `prune_daily_stats`) is **never called**.
- Confirmed unset in the actually-deployed configuration: a grep of
  `charts/distant-signal/values.yaml` and
  `charts/distant-signal/templates/aggregator-deployment.yaml` for
  `dailyStats`/`daily_stats`/`DAILY_STATS` returns **no matches at all** —
  unlike `HISTORY_RETENTION_DAYS`, there is no
  `DAILY_STATS_RETENTION_DAYS` env var wired into the aggregator
  Deployment template. There is no default anywhere in the deployment
  path that would cause pruning to happen.

**This is not a new bug this document is introducing** — the `None`
default and the "unresolved product decision" framing are pre-existing,
already documented in
`docs/superpowers/specs/2026-08-31-line-history-graphics-design.md:751-759`
(Open question 1), written for an unrelated reason (how far back the
Trends tab's UI should ever show, not licence compliance). What this
document adds is the licence angle: **whatever the product answer turns
out to be, it now also needs a hard ceiling under 1 year**, because this
table is fed by content the task brief's ground-truth audit finding
covers.

---

## Human sign-off needed: does an aggregate daily rollup count as "data received"?

This is the one place this document does **not** make the final call,
per this task's own instruction to flag genuine ambiguity rather than
resolve it unilaterally.

`line_status_daily_stats` does not store anything resembling a raw LDBWS
field — no train identifiers, no headcodes, no calling-point lists, no
per-service timestamps, no station names beyond the `line_id` grouping
key. Each row is five/six summed integers and a float per line per day
(`sample_cycles`, `total`, `delayed`, `cancelled`, `skipped`,
`running_count`, a weighted delay sum —
`20260831090001_line_status_daily_stats.sql:33-56`). It is several
processing steps removed from anything Live Departure Board's API
actually returned: `StationSample` → `dedup_new_sample_stats`'s
per-service dedup and counting → a daily `SUM()`.

Whether Schedule 1 §9's "data received" language reaches a rollup this
aggregated — versus applying only to the raw departure-board payloads
themselves, which (per Step 2) are never retained past the next poll
cycle anyway — is a licence-interpretation question, not a code question.
This document flags it for whoever owns the licence relationship to
confirm, the same way the rest of this session's licence work has been
handled. **The remediation below is recommended regardless of how that
question resolves** — it is a low-cost, no-regret fix either way, and it
also happens to unblock the pre-existing open product question from the
line-history-graphics spec.

---

## Remediation plan

Modeled directly on `line_status_history`'s existing, already-correct
pattern (`history_retention_days` / `HISTORY_RETENTION_DAYS` /
`aggregator.historyRetentionDays`), which this plan treats as the
in-repo reference implementation for "a scheduled prune, config-driven
retention window."

**Non-goals:**
- Not attempting to resolve the line-history-graphics spec's Open
  question 1 in full (how far back the Trends tab's UI should let a user
  scroll is a product/UX decision, not this document's to make). This
  plan only proposes a retention *ceiling* under 1 year — a value at or
  below that ceiling can still be chosen later for product reasons; it
  just can no longer be "unset, i.e. forever."
- Not touching `station_samples`, `line_status`, `line_status_history`,
  `tracked_train_tickets`, `tracked_trains`, `train_current_state`, or
  `train_movement_events` — all confirmed clean above, no code changes
  needed for any of them.
- Not writing any code — this is a plan only, per this task's
  constraints. The tasks below are for whoever picks this plan up next.

- [ ] **Task 1: Get the human sign-off from the section above first.**
  If the licence owner determines aggregate daily rollups fall outside
  "data received" entirely, this plan can be downgraded from "compliance
  fix" to "good practice, do it anyway for the product reason the spec
  already flagged" — worth recording either way in whatever document
  tracks the audit's resolution. **Still open** — not resolved by this
  implementation pass, per this plan's own note that the underlying
  legal-sufficiency question is separate from whether the remediation is
  worth doing. Tasks 2-7 below were implemented regardless, since the
  plan itself judges this a low-cost, no-regret fix either way.

- [x] **Task 2: Give `daily_stats_retention_days` a real default,
  strictly under 365.** In `crates/aggregator/src/config.rs:56-66`,
  change the `#[arg(long, env)]` attribute to include
  `default_value_t = <N>` and drop the `Option` wrapper (matching
  `history_retention_days`'s shape exactly), where `N` is chosen with
  enough margin below 365 to comfortably guarantee deletion inside the
  1-year window even accounting for poll/prune cadence (e.g. 300 — a
  specific number is a product/legal call, not this plan's to pick, but
  it must leave real margin under 365, not sit at 364). Update the doc
  comment to drop the "rows accumulate indefinitely" framing and instead
  note the licence-driven ceiling, citing this document.
  **Done: `default_value_t = 300`.**

- [x] **Task 3: Un-gate the prune call in the poll loop.**
  `crates/aggregator/src/main.rs:130-134`'s `if let Some(retention) = ...`
  branch becomes unconditional once Task 2 removes the `Option`,
  mirroring `prune_history`'s unconditional call at line 105 exactly.
  Update the metrics/logging around `daily_stats_pruned` (currently
  assumes it can legitimately be a no-op) to match `prune_history`'s
  pattern.
  **Done.** The metrics/logging (counter increment + `daily_stats_pruned`
  log field) already matched `prune_history`'s unconditional pattern
  exactly; only the dead `if let Some`/`else 0` branch needed removing.

- [x] **Task 4: Wire the Helm chart the same way `historyRetentionDays`
  already is.** Add `aggregator.dailyStatsRetentionDays` to
  `charts/distant-signal/values.yaml` (near `historyRetentionDays` at
  line ~487, same section), defaulting to the value chosen in Task 2, and
  add the corresponding `DAILY_STATS_RETENTION_DAYS` env var to
  `charts/distant-signal/templates/aggregator-deployment.yaml` next to
  the existing `HISTORY_RETENTION_DAYS` block (lines 79-80). This closes
  the actual production gap — right now, even if Task 2 changes the code
  default, an existing values.yaml override or chart upgrade path could
  still leave it unset in a real deployment unless the chart explicitly
  carries a value.
  **Done.** Verified with `helm lint` (passes, pre-existing required-value
  guards unrelated) and `helm template` (renders
  `DAILY_STATS_RETENTION_DAYS: "300"`).

- [x] **Task 5: Update the line-history-graphics spec's Open question 1**
  (`docs/superpowers/specs/2026-08-31-line-history-graphics-design.md:751-759`)
  to note it's now resolved-with-constraint: the "how far back" product
  decision still stands, but must land at or under whatever ceiling Task
  2 sets, for the licence reason this document establishes. Cross-link
  back to this document.
  **Done.** Also lightly updated Decision 1's retention paragraph, which
  still described "unset for v1, or a generous default like 400" as live
  options — both now contradict the sub-365 requirement, so leaving them
  unedited would have made the spec self-contradictory against the
  updated Open question 1.

- [x] **Task 6: Confirm no other consumer of `line_status_daily_stats`
  assumes unbounded retention.** A quick grep of `crates/api` for reads
  of this table (the Trends tab's route, whatever it's called) to check
  it degrades honestly if a user requests a range older than the new
  retention ceiling — likely already handled by the same sparse-data /
  gap-rendering logic Decision 3 of the design spec describes for
  under-covered ranges, but worth a one-line confirmation rather than
  assuming.
  **Confirmed, no code change needed.**
  `crates/api/src/data/queries.rs`'s `daily_stats_for_range` is a plain
  `WHERE day BETWEEN $2 AND $3` select that returns an empty `Vec` for
  any day with no row — including a range entirely outside the retention
  window — matching `line_status_history_for_range`'s existing behavior
  by its own doc comment, no error path. The route
  (`crates/api/src/routes/line_status.rs`'s `get_line_daily_stats`) maps
  whatever comes back straight to JSON. The frontend's
  `frontend/app/lines/[id]/history/TrendsResults.tsx` already renders a
  `sampleCycles`-driven gap for sparse days and an explicit "Not enough
  sampled data yet for this line." message for a fully-empty range
  (Decision 3's sparse-data floor, `SPARSE_DATA_FLOOR_CYCLES = 20`) — so
  a range partly or fully pruned by the new retention ceiling degrades
  the same honest way as any other under-covered range, not a crash or
  misleading empty state.

- [x] **Task 7: Do not touch `trustConsumer.retentionDays`**
  (`charts/distant-signal/values.yaml:606-607`) or anything under
  `crates/trust-consumer/` as part of this work — confirmed out of scope
  in Step 4 above (different feed, unrestricted licence).
  **Confirmed untouched.**

---

## Implementation status (2026-09-01)

Tasks 2-7 implemented. Task 1 (human sign-off on whether an aggregated
daily rollup counts as "data received") remains genuinely open — not
resolved by this pass, per this document's own framing that the
remediation is worth doing regardless of how that question resolves.
Verification: `cargo build --workspace` and `cargo test --workspace`
both pass (0 failures); `helm lint`/`helm template` confirm the new
`DAILY_STATS_RETENTION_DAYS` env var renders correctly.
