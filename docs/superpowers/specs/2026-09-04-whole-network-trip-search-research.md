# Whole-Network Trip Search — Re-Examination Research

**Status: research/scoping only, not an approved design.** Written to the
same rigor and shape as
`docs/superpowers/specs/2026-09-03-option-b-consumer-scoping.md` ("the
scoping doc") — the closest prior-art precedent for this exact kind of
question ("re-examine a deferral now that circumstances changed, give a
real verdict, draw the line precisely"). This document does not propose an
implementation plan; per this repo's process that is a separate, later step
once a direction here is picked.

Required reading, consumed in full before this document was written:
`crates/schedule-query/src/lib.rs`, `resolve.rs`, `records.rs`, `parse.rs`;
`crates/schedule-reference/src/main.rs`; `crates/full-coverage-consumer/src/population.rs`
and `config.rs`; `crates/api/src/routes/ingest.rs` (the
`/private/schedule-line-population` routes) and `crates/api/src/data/queries.rs`
(the same); `crates/api/src/routes/departures.rs` and
`docs/superpowers/specs/2026-09-03-trip-search-design.md` (the shipped
LDBWS picker's own design doc); `docs/superpowers/specs/2026-09-03-track-a-train-input-ux-research.md`
("the original research doc," specifically its Part 2); and
`docs/superpowers/specs/2026-09-03-option-b-consumer-scoping.md`.

## The question, stated precisely

The original research doc's Part 2 (`2026-09-03-track-a-train-input-ux-research.md:233-438`)
asked whether `TrackTrainForm` could offer real trip/service search over the
CIF SCHEDULE feed and answered "not now," sized at roughly a month of work,
citing (a) six unparsed record types (`BS`/`BX`/`LO`/`LI`/`CR`/`LT`), (b)
STP-overlay resolution as "a genuinely stateful algorithm, not a lookup,"
(c) a projected ~400,000+/~6.8M-row relational schema, and (d) a
RAPTOR/Connection-Scan-adjacent journey-planning query layer, transferring
`train-mcp`'s own "roughly a month" sizing directly
(`2026-09-03-track-a-train-input-ux-research.md:265-278`).

That sizing predates `crates/schedule-query` by minutes, not months, in
real repository history, but the two were built in parallel, unaware of
each other: the sibling document that re-confirmed "no cheap slice" for the
CIF path, `2026-09-03-trip-search-design.md`, was authored at 23:38 on
2026-09-03 (`cff5463`); `schedule-query`'s scaffolding started six minutes
later, 23:44 (`6b529a9`), in a different worktree, and wasn't merged until
`fd35c78`. Confirmed directly from `git log --format="%h %ad %s" --date=format:"%Y-%m-%d %H:%M"`
against both commits. **`2026-09-03-trip-search-design.md`'s own
"Re-examining the research doc's 'no cheap partial version' conclusion"
section (`:95-170`) never had `schedule-query` to examine** — it checked
two hand-rolled simplifications (skip STP resolution; skip `LI` records)
and one external crate (`nr-cif`), all found wanting, and correctly
concluded the CIF path itself was still not worth building *at that
moment*, then pursued the LDBWS-based picker instead (shipped,
`f6f6498`). This document is the first pass to ask the question with
`schedule-query` actually in hand.

## Part 1: what `schedule-query` actually gives you, precisely

### Confirmed real-data scale, not an estimate

`crates/schedule-query`'s own `ScheduleIndex::from_text` was run against
the real, local `timetable_full.zip`'s `RJTTF948MCA.txt` and confirmed to
parse **463,947 real `BS` records into 234,941 distinct UIDs, zero parse
errors**, with a real five-TIPLOC WCML-corridor `schedules_touching` query
returning **1,227 well-formed matches** (commit `7909720`, quoted in full
in its own commit message; `crates/schedule-query/src/lib.rs:1-73`'s module
doc corroborates the same figures were independently re-derived four times
across the validation sessions it cites). This is not a fixture-scale
sanity check — it is the real, full national extract.

### What it does answer

- `ScheduleIndex::schedule_for_uid(uid, date)` (`resolve.rs:129-132`) — the
  STP-overlay-resolved booked schedule for one train UID on one date,
  picking the lowest (best-precedence) `StpIndicator` via `min_by_key`
  (`resolve.rs:42-57`, `Ord` derived from declaration order,
  `records.rs:29-47`), correctly distinguishing "no schedule for this
  UID/date" (`None`) from "a schedule exists and is cancelled today"
  (`Some` with `cancelled: true`, empty `calling_points` —
  `resolve.rs:15-30`), which is exactly the `C`-beats-`N`-beats-`O`-beats-`P`
  precedence rule and empty-body-on-cancellation property independently
  confirmed against real bytes (`records.rs:30-47`'s doc comment: `488,798
  (BS) - 407,636 (LO/BX/LT) = 81,162`, exactly the real `C` count).
- `schedules_touching(index, tiplocs, date)` (`resolve.rs:77-96`) — every
  UID's resolved, non-cancelled schedule whose calling points include any
  of a given TIPLOC set, comparing via `normalize_tiploc` so the CIF
  schedule body's fixed 7-character space padding doesn't silently defeat
  a match. This is the exact query `schedule-reference`'s existing
  per-line population publish already calls in production today (see Part
  2, below).

### What it does not answer, confirmed by reading `parse.rs`/`records.rs` in full

- **No operator.** `BX` (Basic Schedule Extra Details, the record carrying
  the ATOC/operator code) is recognized structurally so it doesn't get
  mistaken for a malformed line, but **no field of it is decoded** —
  `parse.rs:34` states this in its own doc comment ("an optional `BX` line
  extends it... but not decoded — no real fixture in this plan's scope
  needed a `BX` field"), and `BasicSchedule` (`records.rs:88-96`) has no
  operator field at all.
- **No headcode.** `BS`'s Train Identity field (the headcode, e.g. `1A23`)
  is likewise undecoded — `BasicSchedule` carries only `uid`,
  `stp_indicator`, `date_from`/`date_to`, `days_of_week`
  (`records.rs:88-96`); confirmed by grepping this crate's own source for
  "headcode"/"Train Identity"/"operator"/"ATOC" — zero matches anywhere
  under `crates/schedule-query/src/`.
- **No CRS.** `CallingPoint` (`records.rs:137-145`) carries only `tiploc`,
  never a CRS code — resolving "which CRS is TIPLOC X" is a separate lookup
  this crate does not own (see Part 2's `stanox_crs` discussion below).
- **No `CR` (Change en Route) decoding**, no `AA` (Association), no
  freight-specific fields — `parse.rs:41-45`'s own doc comment: any record
  type outside `BS`/`BX`/`LO`/`LI`/`LT` is ignored outright, including a
  mid-block `CR` line (parsed as present but not disturbing the open
  block, no field extracted).

Every one of these is a real, honestly-scoped limitation, not an oversight
— `lib.rs:33-38`'s own "What this crate is not" section states plainly:
"No CIF `AA` (Association) record, no freight-specific field, no record
type not already independently exercised against real production CIF
bytes." The consequence for a search feature: **`schedule-query` today
cannot auto-fill Operator or support headcode search** — both real gaps
against the original research doc's sketched UX shapes (1) and (2)
(`2026-09-03-track-a-train-input-ux-research.md:340-361`), and against
parity with the already-shipped LDBWS picker, which does show
Operator (`crates/api/src/render.rs:140`'s `station_departure_json`
includes it, sourced from Darwin, not CIF).

## Part 2: the whole-network CIF parse is not hypothetical — it already runs in production, every 30 minutes

This is the single fact that most changes the original sizing, and it was
independently confirmed by reading `crates/schedule-reference/src/main.rs`
in full, not inferred:

- `poll_once` (`main.rs:98-170`) runs on a `poll_interval_secs`-driven
  `tokio::time::interval`, default **1800 seconds** (`config.rs:45-46`).
- Every cycle, `publish_schedule_line_population` (`main.rs:184-235`) calls
  `read_prefixed_lines_multi(mca_path, &["BS", "BX", "LO", "LI", "CR",
  "LT"])` (`main.rs:190-193`) — an **unfiltered, whole-file** streaming
  read of every schedule-body record in the real ~707MB `RJTTF*MCA.txt`
  (comment, `main.rs:66-70`: "the real 707MB `RJTTF<n>MCA.txt` is never
  held in memory whole, only its... lines" — matched prefixes, streamed,
  not scoped to any line or station) — then
  `schedule_query::ScheduleIndex::from_text(&mca_schedule_text)`
  (`main.rs:201`) parses **the entire national schedule**, exactly the
  463,947-record/234,941-UID scale confirmed in Part 1, **network-wide,
  with no line or station filter applied at parse time.**
- Only *after* that whole-network index is built does the loop narrow:
  `lines_to_publish(&config.lines)` (`main.rs:210,241-247`) iterates every
  catalogued line with at least one `tiploc`-bearing station — **not**
  gated on `LineDefinition.full_coverage_enabled` (`main.rs:172-183`'s own
  doc comment states this explicitly: "Deliberately does NOT gate on a
  second, `schedule-reference`-local scoping flag... `full-coverage-consumer`'s
  own `shadow_lines` config is the only place 'which lines does this
  deployment actually care about' is decided") — and for each, calls
  `schedule_query::schedules_touching(&index, &tiplocs, today)`
  (`main.rs:216`), POSTing the result to `/private/schedule-line-population`
  (`crates/api/src/routes/ingest.rs:330-343`), stored as one opaque `JSONB`
  row keyed `(line_id, service_date)` (`crates/api/migrations/20260904090000_schedule_line_population.sql:17-23`).
  The already-built whole-network `ScheduleIndex` itself is **not**
  persisted or kept resident — it is a stack-local value, freed at the end
  of `publish_schedule_line_population`'s scope, rebuilt from scratch next
  cycle.

**What this means concretely**: the single most expensive, most
error-prone piece the original research doc sized at "roughly a month" —
parsing six CIF record types plus STP-overlay resolution, at real national
scale — is not a future cost to schedule. **It is a sunk cost, already
paid, already running correctly in production every 30 minutes**, as part
of the already-merged Option B shadow-mode pipeline
(`a404a0f`). The only thing scoped to `full_coverage_enabled`/shadow lines
today is *downstream consumption* of the published per-line rows
(`full-coverage-consumer`'s `shadow_lines` config,
`crates/full-coverage-consumer/src/config.rs:98-104,121-149`, defaulting to
`"*"` which itself resolves to "every tiploc-bearing catalogued line," not
"every `full_coverage_enabled` line" — confirmed by
`config.rs:129-136`'s `shadow_line_ids`). The *publish* step
(`lines_to_publish`) is already whole-catalogue, not shadow-gated at all;
what it is **not** yet is whole-*station-reference* — the 110 files under
`lines/` collectively name only 267 distinct TIPLOCs
(`grep -h "tiploc = " lines/*.toml | sort -u | wc -l`), a real but partial
subset of the network the CIF file itself fully covers, and a smaller set
still than the ~2,500-row `stations` reference table
(`docs/superpowers/specs/2026-07-11-operator-station-autocomplete-design.md:20`:
"`stations`/`tocs` are small (~2,500 / ~30 rows)") that `searchStations`
already searches across for every text-autocomplete field in this app,
`TrackTrainForm`'s Origin field included.

### The whole-network CRS↔TIPLOC bridge already exists too

`common::StanoxCrsRecord` (`crates/common/src/lib.rs:741-750`) —
`{stanox, crs, tiploc, station_name, source_sequence}` — is published by
the *same* `schedule-reference` poll cycle (`main.rs:116-158`, the `TI`/`A`
parse that predates and is unrelated to the schedule-population work),
read live via `GET /private/stanox-crs`
(`crates/api/src/routes/ingest.rs:299-306`, `queries::list_stanox_crs`,
`crates/api/src/data/queries.rs:712-723`), and is already whole-network —
it resolves *every* TIPLOC the `TI` record family names (module doc,
`crates/schedule-reference/src/parser.rs:1-13`), independent of the `lines/`
catalogue entirely. This is the piece that lets a whole-network search
resolve an arbitrary typed CRS (any of the ~2,500 `stations` rows, not just
one of the 267 catalogued TIPLOCs) to the TIPLOC `schedule-query` needs,
with zero new parsing work — it is a second output of the exact same
already-running cycle.

## Part 3: what's actually left to build

### (a) Ownership and reload cadence of a whole-network `ScheduleIndex`

**Not "should something parse the file" — that's answered, see Part 2.**
The real question is narrower: *where does the whole-network grouping-by-station
step live, and does it need a permanently memory-resident index, or can it
stay a transient per-cycle computation like today's line-scoped publish?*

The honest answer, reasoned from real numbers rather than assumed: **stay
transient, don't add a permanently-resident whole-network index anywhere.**
A back-of-envelope estimate (not measured — flagged explicitly as an
estimate, not a fact) puts a fully-materialized `ScheduleIndex` at
several hundred MB to roughly 1GB resident: ~7.6M total calling points
network-wide (the prior research's own count, `LI` alone at ~6.8M of ~8.6M
`MCA` lines, `2026-09-03-track-a-train-input-ux-research.md:258-261`,
plus ~2×407,636 `LO`/`LT` rows) at a `CallingPoint` struct's real shape
(`records.rs:137-145`: a heap-allocated `String` TIPLOC, an enum, two
`Option<NaiveTime>`, two `bool`s — realistically ~80-120 bytes per entry
counting allocator overhead) is on the order of 700MB–1GB just for calling
points, before `BasicSchedule`/`HashMap` overhead. Keeping that resident
permanently in a service that's currently a lightweight, restart-safe
poller (`schedule-reference` has no HTTP server today — confirmed by
reading `main.rs` in full: it opens no axum `Router`, only a metrics port
and outbound `reqwest` calls) would change that service's operational
character materially — real memory-provisioning cost, and a real new
failure mode (a resident index now has a lifetime and can go stale or leak
across cycles, unlike today's "build it, use it, drop it" per-cycle
posture).

**Recommendation: extend `schedule-reference`'s existing per-cycle,
transient index build, don't make anything resident.** Concretely:

1. Add one new function to `crates/schedule-query/src/resolve.rs` —
   something like `departures_by_tiploc(index: &ScheduleIndex, date:
   NaiveDate) -> HashMap<String, Vec<LinePopulationEntry>>` (or an
   equivalent shape) — that does **one** O(all UIDs) pass resolving every
   UID for `date` (exactly what `schedules_touching`'s `.filter_map(...)`
   already does internally, `resolve.rs:84-95`), then buckets each
   resolved, non-cancelled schedule's calling points by normalized TIPLOC
   in a single grouping pass. This is a small, real, well-precedented
   addition — same shape, same test conventions, same crate — not a
   redesign. It matters because the *naive* alternative (looping
   `schedules_touching` once per CRS, ~2,500 times instead of ~110) would
   be roughly 23× today's per-cycle query cost (~2,500 × 234,941 ≈ 587M
   comparisons vs. today's ~110 × 234,941 ≈ 25.8M) for no reason — a single
   grouping pass is O(resolved schedules + calling points) regardless of
   how many stations are ultimately queried out of it, and avoids ever
   materializing more than one cycle's transient working set.
2. In `schedule-reference`'s own `poll_once`/`publish_schedule_line_population`
   equivalent, run this grouping pass once per cycle (reusing the
   already-built, already-in-scope `ScheduleIndex` — no second parse), then
   for every CRS the already-published `stanox_crs` table names (not just
   the 267 catalogued TIPLOCs), slice out that station's bucket and POST it
   to a new endpoint, mirroring `/private/schedule-line-population`'s exact
   shape (opaque `JSONB`, primary key `(crs, service_date)` instead of
   `(line_id, service_date)`, same `upsert`/`get` query pair, same OAuth
   credential reuse). The `ScheduleIndex` and the grouping map are both
   dropped at the end of the cycle, exactly as today — no new residency, no
   new failure mode class.
3. Cap each station's published row (mirroring `poller-ldbws`'s own
   `num_rows: 10`-per-station precedent, `crates/poller-ldbws/src/config.rs:46`)
   rather than storing every remaining departure of the day — an
   interchange station's full day list could be large, and nothing about
   this feature's actual UX need (a short pick-list) benefits from storing
   more than a bounded window.
4. Reload/publish cadence: reuse `schedule-reference`'s existing
   `poll_interval_secs` (1800s/30 min, `config.rs:45-46`) unchanged —
   already the right cadence for CIF-sourced data, which per this repo's
   own `trust-consumer`-vs-`schedule-reference` split changes roughly
   daily, not sub-hourly (`crates/trust-consumer/src/config.rs:123-125`'s
   own comment on why its 60s `reference_reload_secs` is deliberately finer
   than `schedule-reference`'s cadence for exactly this reason). One
   **explicitly unresolved** question this document does not settle: does
   the underlying delivery pipeline (`schedule-ingest`) ever receive
   same-day VSTP-style urgent amendments, or only the next scheduled full
   delivery? Nothing read this session establishes either way — flagged as
   an open question below, not assumed.

This design deliberately avoids two more expensive alternatives considered
and rejected: (i) a synchronous request-time HTTP call from `api` into a
newly-HTTP-serving `schedule-reference`, which would be a new
architecture pattern this app doesn't have anywhere else (every other
cross-service link is async, publish-then-poll, not synchronous
fan-out — introducing it here would couple `api`'s public request latency
to a batch poller's uptime and query performance, for one feature); and
(ii) storing full per-schedule calling-point detail for all ~2,500
stations without a cap, which risks unbounded JSONB row growth for busy
interchanges.

### (b) The query surface a search UI actually needs

Two shapes, both already answerable once (a) exists, reusing existing
patterns wholesale:

1. **"Upcoming departures from station X, today."** A new `GET
   /private/schedule-network-departures?crs=X&service_date=Y` route,
   near-identical in shape to the existing
   `get_schedule_line_population`/`upsert_schedule_line_population`
   pair (`crates/api/src/routes/ingest.rs:330-354`,
   `crates/api/src/data/queries.rs:731-774`) — copy-adjacent, not a novel
   pattern — plus a new public passthrough,
   `GET /public/stations/{crs}/schedule-departures`, mirroring
   `crates/api/src/routes/departures.rs`'s existing
   `get_station_departures` (404-vs-`200 []` honesty split, hand-built
   camelCase JSON via a new `render.rs` helper) exactly, just reading the
   new table instead of `station_samples`.
2. **"The full calling pattern for a specific train."** Requires no new
   query at all if each published entry already carries its full
   `calling_points` (not just the queried station's own row) — which is
   free to do, since `LinePopulationEntry` (`records.rs:147-160`) already
   stores the complete pattern per UID, not a filtered subset, and the
   grouping pass in (a) can reuse that exact type unchanged. A "show full
   pattern" UI reads directly off the same response the departures picker
   already fetched — zero additional backend work.

Neither shape needs a journey-planning algorithm (RAPTOR/Connection Scan).
That was the original research doc's inclusion, not a requirement of
`TrackTrainForm` itself — the form tracks one train a user already intends
to take; it was never a multi-leg journey planner, so RAPTOR-class
algorithm work looks, in hindsight, like scope carried over from
`train-mcp`'s different problem shape rather than something this specific
feature ever needed.

### (c) A new API route

Described above: one new migration (`schedule_network_departures` table,
copy-adjacent to `schedule_line_population`'s own migration), two new
private routes (`POST`/`GET`, copy-adjacent to
`ingest.rs:330-354`), one new public route (copy-adjacent to
`departures.rs`'s existing single route). No new OAuth group needed if
`schedule-reference`'s existing writer credential is reused (it already
posts to `/private/schedule-line-population` with its own credential,
`main.rs:167,224-234`); a new reader credential (mirroring
`internal_oauth_group_full_coverage`'s precedent, `0274548`) only if the
new GET route needs the same private-route auth split
`/schedule-line-population` uses — likely yes, for consistency, though the
*public* passthrough route (unauthenticated, matching every other
`/public/*` route) is what the frontend actually calls.

### (d) Frontend changes to `TrackTrainForm`

This is where a real, currently-unresolved **product** decision lives, not
just an engineering one. `TrackTrainForm.tsx` already has a fully-built
three-state departures picker (`not-sampled` / empty array / populated,
`TrackTrainForm.tsx:84-108,215-263`) wired to the LDBWS-backed
`/api/stations/{crs}/departures`. The new CIF-derived source is a second,
different departures list with different coverage (whole ~2,500-station
network vs. the ~286 LDBWS-sampled set), different freshness (up to 30 min
stale vs. LDBWS's ~60s poll cycle, `crates/poller-ldbws/src/config.rs:80`),
and a narrower field set (no Operator, no headcode, confirmed in Part 1).
Two real interaction shapes:

1. **Fallback-only (recommended for a first slice):** the existing
   `not-sampled` state (`TrackTrainForm.tsx:215-219`, currently a plain
   "enter the details below" sentence) becomes a second fetch to the new
   CIF-derived endpoint instead of a dead end — a station outside the
   ~286-sample set gets *a* picker instead of *no* picker, still
   auto-filling Destination/Scheduled-departure (Operator left for the
   user to type, honestly, since the data genuinely doesn't have it — the
   same "honest partial feature" posture this codebase already uses
   repeatedly, e.g. the per-station-stats doc's "53 of 286" framing,
   `2026-09-03-per-station-stats-design.md:111`, and this exact form's own
   `not-sampled` honesty state). For an already-LDBWS-sampled station,
   nothing changes — the existing picker keeps winning, sidestepping any
   need to reconcile two live sources for the same station.
2. **Merge/replace for every station:** show CIF-derived departures
   everywhere, either instead of or blended with LDBWS's. Real added value
   (uniform coverage, richer calling-pattern detail via (b)'s second
   query) but a real added cost this document does not think is worth
   paying in a first slice: reconciling two sources with different
   freshness and field completeness for the *same* station requires a
   dedup/merge policy (match by time+destination? prefer one source's
   Operator over the other's absence? what happens when they disagree
   about a cancellation?) that has no existing precedent in this codebase
   to reuse, unlike everything else in this section.

**Concrete comparison against the already-shipped LDBWS picker** (per the
brief's ask):

- **Coverage**: whole ~2,500-`stations`-table network vs. ~286 LDBWS-sampled
  stations (`2026-09-03-trip-search-design.md:199-206`'s own confirmed
  figure) — the single clearest, uncontested win for the CIF-derived
  source.
- **STP=C same-day-cancellation awareness**: **this document does not
  confirm the premise the brief poses ("correctly reflecting STP=C
  cancellations before LDBWS itself would show them") and flags it as
  likely backwards, not verified either way.** `schedules_touching`
  already filters out cancelled schedules correctly (`resolve.rs:88`,
  tested against a real Bank Holiday `STP=C` override,
  `resolve.rs:217-224`), so a CIF-derived list is correct about *planned*
  STP=C withdrawals as of its last 30-minute-or-less-stale publish. But
  Darwin/LDBWS is fed from the same underlying Network Rail timetable data
  and additionally reflects real-time, same-day operational cancellations
  (a broken-down unit, a late-notice signalling failure) that a CIF STP
  overlay — a Short Term Planning mechanism for planned changes — would
  never carry at all. Nothing read this session establishes which source
  is more current for planned changes specifically (both likely ingest the
  same upstream STP data, on different cadences this document hasn't
  measured), so this is listed as a genuinely open question, not a settled
  advantage either way.
- **Operator/headcode**: LDBWS wins outright today — both are present in
  `StationDeparture` (`operator` always populated; `headcode` always
  `None` even there, per `poller-ldbws/src/schema.rs:104-105`) — CIF search
  has neither until `schedule-query` is extended to decode `BX`/`BS` Train
  Identity (a real, small-to-medium addition needing the same
  real-byte-verification rigor this repo's other CIF work already
  applies, not free).
- **Calling-pattern detail**: CIF-derived wins — a full stop-by-stop
  pattern per UID is a byproduct of `LinePopulationEntry`'s existing shape
  (`records.rs:147-160`); LDBWS's `StationDeparture` carries only
  `skipped_stations`, not the full pattern.

## Part 4: sizing, stated honestly

**Genuinely much smaller than the original month-scale estimate — on the
order of a few focused implementation days, not a month — but not free,
and carrying one real, non-trivial ops cost increase.** Specifically:

- The original estimate's largest, hardest, most error-prone component —
  parsing six CIF record types plus STP-overlay resolution at national
  scale — is **done**, merged, and independently confirmed correct against
  the real 463,947-record/234,941-UID extract (Part 1). This is not a
  discount on the estimate; it is the removal of its dominant term.
- The original estimate's "~400,000+/~6.8M-row relational schema" premise
  turned out to be avoidable entirely — the actual implementation stores
  nothing in a normalized schema at whole-network calling-point grain; it
  keeps the resolved data in-memory, transiently, per cycle, and persists
  only small, opaque, per-key JSON blobs (Part 2's `schedule_line_population`
  precedent) — a materially cheaper storage shape than what was priced in.
- The original estimate's RAPTOR/Connection-Scan journey-planning
  component was never actually required by this feature's real shape
  (Part 3b) — it priced in a bigger problem than `TrackTrainForm` needs
  solved.
- What remains is bounded and enumerable, not open-ended: one new
  `schedule-query` grouping function; optionally, `BX`/headcode decoding
  (real work, needs real-byte verification, genuinely deferrable — see
  Recommendation); a widened publish loop in `schedule-reference` (~23×
  more per-cycle Postgres upserts than today, ~2,500 rows vs. ~110, each
  capped in size — a real, quantifiable, bounded increase, not an unknown
  one); one new migration; two new private routes and one new public route,
  each copy-adjacent to an existing, already-reviewed pattern; and a
  frontend change to `TrackTrainForm` that is additive (a second
  fetch behind the existing `not-sampled` state) if scoped as recommended
  below, not a rewrite of the existing picker.
- The one real, honestly-flagged new *ops* cost: `schedule-reference`'s
  per-cycle Postgres write volume grows meaningfully (~23× row count,
  though still bounded per row) — worth a capacity sanity-check before
  shipping, but not a blocker, and nowhere close to the "second live Kafka
  consumer, real writes to real user-facing severity data" cost class the
  scoping doc correctly gated behind Option B's own separate validation
  (`2026-09-03-option-b-consumer-scoping.md:173-192`) — this feature adds
  no new Kafka consumer group and no live-feed dependency at all; it is
  purely a batch-file-derived read path, a materially lower-risk class of
  change than Option B's live correlation consumer.

## Recommendation

**Proceed to spec+plan+implement now — scoped down, not full parity with
the shipped LDBWS picker.** Concretely, as a first slice:

1. Build (a)/(b)/(c) above: the `schedule-query` grouping function, the
   widened `schedule-reference` publish (whole `stanox_crs`-known network,
   capped per station), the new migration/routes.
2. Ship the frontend as **fallback-only** (Part 3d, shape 1): the new
   picker appears only where the existing LDBWS picker today shows
   "not available to browse for this station" — additive, not a
   replacement, sidestepping the two-source reconciliation question
   entirely for v1.
3. **Defer `BX`/headcode decoding** to a later pass — ship the
   fallback picker honestly missing Operator (mirroring this form's own
   established "leave it for the user to type" posture for an optional
   field, `TrackTrainForm.tsx:180-185` unchanged shape), rather than
   blocking this slice on new byte-offset verification work that is real
   but separable.

This is the real call, not a hedge: the enabling technology is proven, the
remaining engineering is bounded and precisely enumerated above (not a
"figure it out later" ask), the value is concrete and immediate (~2,500-
station coverage vs. ~286, for stations that today get nothing), and
scoping to fallback-only for v1 removes the one genuinely unresolved
product question (how two live sources should coexist for the same
station) from this slice's critical path without blocking or diminishing
the value it does ship.

## Open questions

1. **Does `schedule-ingest`'s delivery pipeline ever receive same-day
   VSTP-style urgent CIF amendments, or only scheduled periodic full
   deliveries?** Unresolved by this document (see Part 3d) — directly
   affects how confidently "STP=C awareness" can ever be marketed as a
   real advantage over LDBWS, and isn't answerable from the code read this
   session alone.
2. **Real memory/timing measurement of a full `ScheduleIndex` build**
   remains unmeasured — Part 3a's ~700MB–1GB estimate is reasoned from real
   struct shapes and the real confirmed record counts, not profiled. Worth
   a cheap, real check (the existing `examples/inspect.rs` dev tool against
   the real `timetable_full.zip`, timed and memory-profiled) before
   committing to cadence/capacity numbers in an implementation plan.
3. **Whether the merge/replace shape (Part 3d, shape 2) is worth pursuing
   once the fallback-only slice has real usage data** — deliberately left
   for a future pass, not resolved here, consistent with this repo's own
   "ship the honest partial thing, revisit with real data" pattern already
   used for LDBWS's own ~286-station scope and the per-station-stats
   feature's 53/286 split.
4. **`BX`/headcode decoding's own real sizing** — this document treats it
   as "real, small-to-medium, deferrable" but does not size it precisely
   (no real-byte-offset verification session was run this pass, matching
   this repo's own "no invented byte offsets" convention — a future pass
   would need to do what the `BS`/`LO`/`LI`/`LT` validation sessions
   already did for those record types, applied to `BX` and `BS`'s Train
   Identity field specifically).
