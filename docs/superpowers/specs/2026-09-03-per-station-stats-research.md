# Per-Station Delay/Cancellation Stats — Research

**Status: research/scoping only, not an approved design.** Written to the
same rigor and shape as `docs/superpowers/specs/2026-08-31-station-catalogue-completeness-research.md`
and `docs/superpowers/specs/2026-09-01-stanox-crs-live-reference-data-research.md`
(both citation-heavy audits of an existing data model's real shape and
gaps, ending in a recommendation plus an explicit open-questions list, not
a committed schema). This document does not propose an implementation
plan — per this repo's process, that would be a separate, later step once
a direction here is actually picked.

It sits directly on top of two adjacent, already-written documents this
session was pointed at as precedent and is explicit about how it relates
to each: `docs/superpowers/specs/2026-08-31-sample-data-availability-design.md`
("08-31") and `docs/superpowers/specs/2026-09-01-line-status-sample-coverage-design.md`
("09-01"). **Correction/update relative to 09-01, confirmed by reading the
code directly rather than assumed from the doc's own "sketch, not final"
framing: 09-01's proposed `SampleAvailability` enum, `LineStatus.sample_availability`
field, and the `NoCoverage`/`BelowThreshold`/`Available` three-way split
are no longer a proposal — they are implemented, on `main`, exactly as
that document sketched them** (`common::SampleAvailability`,
`crates/common/src/lib.rs:717-734`; `compute_sample_availability`,
`crates/aggregator/src/aggregation.rs:859-882`; `infer_from_samples`
consuming it, `aggregation.rs:884-960`). This document treats that as
already-existing infrastructure, not something still to design, and reuses
its vocabulary (`NoCoverage`/`BelowThreshold`/`Available`) throughout.

## The question being researched

Today, delay/cancellation/skip stats (`common::SampleStats`,
`crates/common/src/lib.rs:710-716`) are computed and stored only at the
**line** level (`LineStatus.sample_stats`), aggregated across a line's
curated `sample_stations` (2-5 CRS codes per line, `lines/*.toml`). The
underlying raw data is already per-station — `station_samples` is one row
per polled CRS, holding that station's live LDBWS departure board
(`common::StationSample { crs, polled_at, departures: Vec<StationDeparture> }`,
`crates/common/src/lib.rs:526-530`) — but nothing today turns that
per-station raw data into a per-station rate. The station detail page
(`frontend/app/stations/[crs]/page.tsx`) shows, for every line that covers
a station, that *line's* sample summary (`representativeStatus`/
`formatSampleSummary`, `page.tsx:141-148`, reading `frontend/lib/sampleStats.ts`)
— never a number that describes the station on its own, independent of
which line a viewer happens to be reading about.

This document researches what "per-station stats" would mean and take to
build. It does not recommend building it — see Recommendation.

## Method

Read `crates/aggregator/src/aggregation.rs` in full (3,331 lines),
`crates/aggregator/src/queries.rs`'s `station_samples`/`line_status`-facing
functions, `crates/aggregator/src/dedup.rs`'s module doc, `crates/api/src/data/queries.rs`'s
`station_samples`-facing functions, `crates/api/src/routes/samples.rs` and
`crates/api/src/routes/line_status.rs`'s `get_stop_point_disruption`,
`crates/poller-ldbws/src/main.rs` and `crates/poller-stations/src/main.rs`
in full, the `station_samples`/`stations`/`line_status_daily_stats`
migrations, `frontend/app/stations/[crs]/page.tsx` and
`frontend/lib/sampleStats.ts`, and both TRUST-line specs named in the
brief. Also ran two small direct scripts against this repo's own
`lines/*.toml` files (not from memory or estimate) to get real counts for
"how many sample stations exist" and "how many are shared across
operators" — both quoted below with the exact method, not just the
numbers.

## Current relevant state

### The raw data is already per-station-shaped

`station_samples` (`crates/api/migrations/20260510023522_initial.sql:52-56`):

```sql
CREATE TABLE station_samples (
    crs        CHAR(3)     PRIMARY KEY,
    polled_at  TIMESTAMPTZ NOT NULL,
    departures JSONB       NOT NULL DEFAULT '[]'
);
```

One row per CRS, `crs` as the primary key, wholesale-replaced every poll
(no history, confirmed by the upsert at `crates/api/src/data/queries.rs:258-`,
and restated directly in `latest_station_sample`'s own doc comment,
`crates/api/src/data/queries.rs:656-658`: *"`station_samples` is
wholesale-replaced per poll (one row per station, no history)"*).
`StationDeparture` (`crates/common/src/lib.rs:378-401`) carries
`service_id`, `operator`, `destination_crs`, `is_cancelled`,
`delay_minutes`, `cancel_reason`/`delay_reason`, `headcode`, and
`skipped_stations: Vec<String>` — everything `stats_from_departures`
(below) needs, already scoped to one station, with no line concept
attached at the storage layer at all. **The data model does not need to
change for a per-station number to exist** — it already is one station's
worth of raw departures per row.

### The classification logic that would compute a rate

`crates/aggregator/src/aggregation.rs`, three functions, read in full:

- `relevant_departures` (`aggregation.rs:794-810`): `line.sample_stations.iter()
  .filter_map(|crs| samples.get(crs)).flat_map(|s| s.departures.iter())
  .filter(|dep| belongs_to_line(dep, line)).collect()`. Pools departures
  across a **line's** multiple sample stations, then filters to only the
  departures that `belongs_to_line` (`aggregation.rs:983-1000`) judges
  relevant — operator membership (mandatory), then optional
  `destination_crs_filter`/`headcode_prefixes` narrowing for shared-trunk
  stations.
- `stats_from_departures` (`aggregation.rs:816-844`): the actual
  delayed/cancelled/skipped/avg-delay arithmetic, given an already-filtered
  `&[&StationDeparture]` slice and a `line: &LineDefinition` (used only to
  build `line_stations: HashSet<&str>` for the skip check — see "What
  needs restructuring" below) plus `thresholds: &Defaults` (used only for
  `delay_threshold_minutes`).
- `compute_sample_availability` (`aggregation.rs:859-882`): the
  `NoCoverage`/`BelowThreshold`/`Available` gate, per the "already
  implemented" note above — checks `has_any_row` (any of `line.sample_stations`
  present in `samples`), then `relevant.len() < thresholds.min_sample_size`,
  then calls `stats_from_departures`.

**What's directly reusable for a per-station version**: the arithmetic
core of `stats_from_departures` — counting `is_cancelled`, counting
`delay_minutes >= threshold`, averaging `delay_minutes` over non-cancelled
departures — operates on a flat `&[&StationDeparture]` slice already,
with no line-shaped assumption baked into *that* part. Handed one
station's own `departures` directly (no `relevant_departures` pooling
across multiple stations needed at all — a `StationSample` for one CRS
already **is** the input `stats_from_departures` wants, once the `belongs_to_line`
question is answered — see next section), the count/average logic runs
unchanged.

**What needs restructuring, concretely, not hand-waved**:

1. `stats_from_departures`'s **skip check** (`aggregation.rs:823-832`)
   currently means "this departure skips a stop that is on **this line's**
   route" (`line_stations: HashSet<&str>` built from `line.stations`, the
   *route* list, not `sample_stations`). A true per-station skip rate has
   a more natural, narrower definition available — "does this specific
   departure skip calling at **this station** at all" (checking
   `dep.skipped_stations.contains(&this_crs)`) — but that is a different
   question than the line-level one and would need to be decided, not
   inherited verbatim. Reusing today's `line_stations`-shaped signature
   as-is for a per-station caller would require passing in a
   single-station set, which technically works but stops being about "did
   this train skip somewhere on the route" and starts being "did this
   train skip *this* station specifically" — worth naming explicitly as a
   semantic decision, not a mechanical port.
2. `thresholds: &Defaults` (`delay_threshold_minutes`, and separately
   `min_sample_size` in `compute_sample_availability`) is sourced today
   via `thresholds_for(defaults, &line.severity_overrides)`
   (`crates/common/src/lib.rs`) — **a per-line override mechanism.** There
   is no per-station equivalent. A per-station computation needs a
   threshold source too; the only currently-available options are the
   global `Defaults` (no line-specific tuning applies, which may be wrong
   for a station served only by a line with a deliberately-tuned
   override) or inventing a new station-keyed override concept that does
   not exist anywhere in this codebase today. Not a blocker, but a real,
   unresolved design gap this document flags rather than assumes away.
3. **Visibility/location**: `relevant_departures`, `stats_from_departures`,
   `compute_sample_availability`, and `belongs_to_line` are all
   crate-private (`fn`/`pub(crate) fn`) inside `crates/aggregator`. The
   `common` crate (shared by `aggregator` and `api`) currently only holds
   the *types* (`SampleStats`, `SampleAvailability`, `Defaults`,
   `thresholds_for`) — the classification *logic* lives in `aggregator`
   only. Any reuse from `crates/api` (relevant to "where should this be
   computed," below) needs at least the single-station arithmetic core
   promoted to `common`, or duplicated. This is a small, mechanical
   refactor if pursued (`stats_from_departures`'s core loop has no
   `aggregator`-specific dependency once the skip-check question above is
   settled), not a large one.
4. **Dedup is line-scoped, not station-scoped.** `crates/aggregator/src/dedup.rs`'s
   `SeenServiceLedger` (module doc, `dedup.rs:1-40`) tracks which Darwin
   `service_id`s have already contributed to a `(line_id, period)` pair,
   specifically so a rolled-up rate (e.g. a future daily figure) counts
   each real train once, not once per 60s poll cycle it sits in a
   station's departure-board window. A per-station rollup over time would
   need its **own** ledger keyed by `(crs, period)`, not a reuse of the
   line-scoped one — a real train that calls at two of a line's sample
   stations already gets deduped once per line today; a per-station
   rollup would legitimately count it once *per station it calls at*,
   which is a different (not wrong, just different) accounting unit. This
   only matters if a per-station rollup-over-time (day/week) is ever
   built, not for a live, single-cycle number — flagged for completeness
   since Decision-analogous work elsewhere in this repo (09-01,
   `2026-08-31-line-history-graphics-design.md`) treats this exact
   distinction as load-bearing.

**Answer to "can this be computed today with no new data source":
functionally yes** — `station_samples` already has everything a per-station
`SampleStats` needs, and the counting arithmetic (not the whole pipeline,
which is line-shaped throughout) is a small, well-isolated piece of
already-working code. What's missing is not data, it's a station-shaped
call path through logic that is currently only reachable in a line-shaped
form, plus a couple of real semantic decisions (skip definition, threshold
source) that reusing the line-level code verbatim would silently paper
over rather than actually answer.

### What "per-station" means when a station serves multiple operators — checked directly, not assumed

`belongs_to_line` (`aggregation.rs:983-1000`) is the **only** thing that
decides which of a sample station's raw departures "count" for a given
line's stats today — mandatory operator membership, then optional
destination/headcode narrowing. An **unfiltered** per-station number (all
departures in `station_samples.crs = X`'s row, with no operator filter at
all) would simply skip that step, pooling every operator calling at that
station into one rate.

Whether that's a real, common situation (not hypothetical) was checked
directly against `lines/*.toml`'s actual data, not guessed:

```
$ python3 -c "... reads every lines/*.toml, strips comments, unions each
  line's operators onto every CRS in its sample_stations ..."
total distinct sample stations across the catalogue: 286
sample stations used by lines whose operators differ (>1 distinct ATOC
code among the lines that sample that station): 53
```

(script logic: for each `lines/*.toml`, parse `operators = [...]` and
`sample_stations = [...]`, comment-stripped; union operators per CRS
across every line file that names it; count CRS codes with more than one
distinct operator code attached.) Examples directly from that output:
`EDB` (Edinburgh) is a sample station for lines run by `GR`/`LD`/`SR`/`TP`
across 8 different line files; `LIV` (Liverpool Lime Street) for
`EM`/`NT`/`TP`/`VT` across 6; `NCL` (Newcastle) for `GR`/`LD`/`TP`/`XC`
across 5.

**53 of 286 sample stations (~19%) are genuinely, currently shared across
lines with different operators.** An unfiltered per-station rate at any
one of these would mix, say, a CrossCountry long-distance service's
punctuality with a local TransPennine stopping service's, under one
number — two operationally unrelated, differently-scheduled, often
differently-punctual services a passenger would not expect collapsed
together. This is not a rare edge case this design could reasonably
ignore; it is close to a fifth of the exact station set that would have
any data to compute from at all (see "Polling scope," below, on why
that's the relevant denominator, not the full station catalogue).

**Implication**: a single unfiltered "this station's rate" number would be
honest about literally what it measures ("every departure recorded at this
station, any operator") but would not answer the question a viewer
reading it on a station page is actually likely to have ("is my train
usually on time"), for a real, non-trivial fraction of stations. Two
non-exclusive shapes are more defensible than one flat number:

1. **One number per station, unfiltered** — cheapest, matches "this whole
   station" literally, but degrades exactly at the busiest, most
   multi-operator hub stations (Edinburgh, Liverpool Lime Street,
   Newcastle, per the data above) — arguably where a meaningful number
   matters most, since those are the stations most likely to actually
   clear `min_sample_size`.
2. **One number per (station, operator) pair** — mirrors this app's
   existing `belongs_to_line`/operator-filtering precedent exactly (same
   mandatory-operator-membership rule, just not tied to a specific line's
   full definition), and would read correctly at a shared station. Costs
   more to design and display (a station page would need to show
   multiple rows, one per operator serving it, rather than one figure)
   and needs a decision about what "operator serving this station" even
   means absent a `LineDefinition` to draw the boundary — `StationDeparture.operator`
   alone, with no further narrowing, since `destination_crs_filter`/
   `headcode_prefixes` are line-specific narrowing concepts with no
   station-level analogue.

This document does not pick one — see Recommendation — but the 53/286
finding is direct evidence that "just don't filter, it's one station" is
not a neutral default; it is a real, measurable simplification that would
misrepresent a meaningful share of the very stations most likely to have
enough sample volume to report anything at all.

### Where this could be computed and stored — three real options, weighed against how the pipeline is actually built

The existing pipeline is `crates/aggregator` (a periodic batch job) →
Postgres (`line_status`, `line_status_daily_stats`, `station_samples`) →
`crates/api` (read-serving HTTP layer, does not itself compute
`SampleStats`/`SampleAvailability` today — it only renders what `aggregator`
already wrote, per `crates/api/src/render.rs`'s own module doc that
storage/wire shapes are deliberately separate concerns from computation).

**Option A — a new per-cycle aggregator output, parallel to `LineStatus`.**
`aggregator`'s existing `main.rs` loop (`crates/aggregator/src/main.rs:175-186`
calls `lines_with_sample_coverage`, then `write_line_status`/
`record_daily_stats` per line, all inside the batch already reading
`load_station_samples` once per cycle — `queries.rs:65-80`) already has
the full `HashMap<String, StationSample>` in memory once per cycle. A
`compute_per_station_availability(samples, defaults) -> HashMap<String,
SampleAvailability>` (or an operator-keyed variant) could run alongside
today's per-line pass, iterating `samples.keys()` directly rather than
`line.sample_stations`, and be written to a new table analogous to
`line_status`. **Pro**: reuses the exact cycle boundary and the exact
in-memory data the line-level computation already has; no new polling or
ingestion needed. **Con**: this is genuinely new aggregator logic, not a
byproduct of the existing line pass — `samples` is loaded once per cycle
specifically to be filtered *per line*; a per-station pass over the same
map is a parallel, not a reused, computation (see "What's reusable," #3
above — the arithmetic can be shared, but the call path is new).

**Option B — a new table, `station_status`/`station_status_daily_stats`,
analogous to `line_status`/`line_status_daily_stats`.** `line_status`
(`crates/api/migrations/20260510023522_initial.sql:59-77`, JSONB
`statuses` column, no migration needed to add fields since it's a blob)
and `line_status_daily_stats` (`crates/api/migrations/20260831090001_line_status_daily_stats.sql`,
`PRIMARY KEY (line_id, day)`, incrementally-summed columns, fed by
`record_daily_stats` once per line per cycle) are the direct structural
precedent — a `station_status_daily_stats` table keyed `(crs, day)` (or
`(crs, operator, day)`, per the previous section's finding) with the same
running-sum shape would let a station page show a rolling rate the same
way a line's Trends tab does today. **Pro**: gives per-station stats a
real history, not just a live snapshot — matching what `line_status_daily_stats`
already proved out for lines, including the same dedup-by-`service_id`
correctness concern (see "What needs restructuring," #4). **Con**: a real
new migration, a real new write path in `aggregator`, and (if the
`(station, operator)` shape from the previous section is chosen) a
schema decision about whether `operator` is part of the key — meaningfully
more implementation surface than a live-only figure.

**Option C — compute on demand at read time, directly from `station_samples`,
inside `crates/api`.** This is not hypothetical or need to be invented —
**`crates/api` already has exactly this capability, built and shipping,
for a different feature.** `crates/api/src/data/queries.rs:664-679`,
`latest_station_sample(pool: &PgPool, crs: &str) -> Result<Option<StationSample>>`,
runs `SELECT crs, polled_at, departures FROM station_samples WHERE crs =
$1` — a single-station, on-demand read, already used by
`crates/api/src/routes/train.rs:524` to back `eta_blend.rs`'s Darwin/TRUST
ETA correlation for a tracked train's pin. A per-station-stats read
handler could call this exact function, then run the (per "What's
reusable," promoted-to-`common`) counting arithmetic directly against the
returned `departures`, with **zero new table, zero new migration, zero
new aggregator write path** — the only new code is a route handler plus
whatever arithmetic gets promoted out of `aggregator`. **Con, the same one
08-31/09-01 already established for `sample_stats`/`sample_availability`
generally**: a live-only number, computed at read time from whatever
`station_samples` happens to hold *right now* — no history, same
"quiet vs. under-sampled vs. stale" ambiguity 08-31's whole document is
about, now recreated for a station instead of a line, with no
`SampleAvailability`-equivalent unless one is separately built for this
path too.

**Weighed against how the pipeline is actually structured**: Option C is
the cheapest and most consistent with what already exists (`latest_station_sample`
is proven, shipping code, not a new pattern), and is the natural starting
point if the goal is "show *something* honest on the station page soon."
Option A is the natural next step if a per-cycle, `SampleAvailability`-shaped
signal (not just raw numbers) is wanted, matching the rigor 09-01 already
built for lines. Option B is the right target only once/if a per-station
history (a Trends-equivalent for a station) is an actual product goal —
building it ahead of that need would be premature, mirroring 09-01's own
explicit non-goal ("Extending `line_status_daily_stats`... rejected for
this pass," 09-01's Explicitly out of scope) for the same "don't build
the rollup before there's a concrete use for it" reasoning.

### Polling scope — confirmed directly against `poller-ldbws`

`poller-ldbws` does **not** poll a broad or independent station list —
it polls exactly the deduplicated union of every line's (catalogue and
custom) `sample_stations`, and nothing else:

- `poller-ldbws/src/main.rs:123`: `fetch_sample_stations` calls
  `GET /private/sample-stations` to learn which CRS codes to sample each
  cycle (`main.rs:1-17`'s own module doc: *"samples live departure-board
  data for every station any line's inference logic depends on"*).
- `crates/api/src/routes/samples.rs:20-29`, the handler behind that route:
  loads `app.config.lines` (the static catalogue) plus every row from
  `custom_lines` (`crates/api/src/data/custom_lines`), converts custom
  lines via `LineDefinition::from`, and returns
  `dedup_sample_stations(&lines)` — `crates/api/src/data/samples.rs:11-23`,
  a deduplicated, sorted union of every line's `sample_stations` field,
  confirmed by its own doc comment (`samples.rs:7`).
- Measured directly (same script as the operator finding above): **286
  distinct CRS codes** are the current real union across all of
  `lines/*.toml`'s `sample_stations` arrays.

**`poller-stations`, by contrast, polls the full RDM Stations reference
feed** (`crates/poller-stations/src/main.rs:1-9`'s own doc: *"polls the RDM
Stations JSON feed... forwards parsed `StationReference`s"*) into the
separate `stations` table (`crates/api/migrations/20260706004003_reference_data.sql:10-18`)
— this is name/coordinate/operator/accessibility reference metadata for
every station RDM knows about (Great Britain's full national rail
network, several times larger than 286 — not independently re-counted in
this pass, but `docs/superpowers/specs/2026-08-31-station-catalogue-completeness-research.md`'s
whole premise is that the *route-membership* list, `lines/*.toml`'s
`stations` arrays — a separate, larger-than-`sample_stations` but still
curated list, not the full `stations` table either — already has real,
acknowledged gaps against real open stations), never live departure data.

**Implication, stated plainly**: "per-station stats" can only ever exist,
under current polling scope, for the 286 CRS codes that are someone's
`sample_stations` entry. The other, much larger set of stations
`poller-stations`/the `stations` table knows about structurally has no
live departure data behind it at all — not a threshold-miss, not a
`NoCoverage` cycle, but zero possibility of ever computing anything,
because `poller-ldbws` never once calls RDM for that CRS. A per-station
stats feature that wants to cover "any station a user might look up" (the
`/stations/[crs]` route accepts any CRS-shaped code, gated only by
`lookupStation`'s catalogue-name check, `frontend/app/stations/[crs]/page.tsx:36-44`)
would need to either scope itself explicitly to "stations that happen to
already be a sample station for some line" (a real, if arbitrary-feeling
from a user's perspective, subset) or **broaden `poller-ldbws`'s own
scope** — a materially bigger, separate piece of work: RDM rate limits and
`poller-ldbws`'s own per-cycle "one LDBWS call per station" design
(`poller-ldbws/src/main.rs:14-16`'s own doc: *"no bulk/multi-station LDBWS
operation"*) already strains at 286 sequential per-station calls per
60-second cycle (the file's own comment at `main.rs:51` sizes the current
load specifically against `sample_stations`'s ~280); polling the full
station catalogue instead would be an order-of-magnitude-larger, separate
scoping question this document does not attempt to answer, only names as
a real, load-bearing prerequisite if "per-station stats for any station"
is the actual goal rather than "per-station stats for the ~286 stations
that already have data."

### Relationship to the TRUST/full-schedule line of work

Read both `docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`
("the design spec") and `docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md`
("the validation findings," including its 2026-08-30 and
2026-08-31/09-01 re-run appendices) in full.

**`DataQuality::TrustInferred` is defined but not live, confirmed by a
fresh grep, not carried over from either prior document's own claim**:
`crates/common/src/lib.rs:287` is its only occurrence anywhere under
`crates/` — `grep -rn "TrustInferred" crates/` returns exactly that one
line (the enum variant's own definition), with no constructor, match arm,
or test referencing it anywhere else in the codebase. `DataQuality::Tfl`
and `::LdbwsInferred`/`::Knowledgebase`/`::Planned` are all constructed at
real call sites (`poller-tfl/src/schema.rs`, `aggregation.rs`'s
`infer_from_samples`/incident path); `TrustInferred` has none. The design
spec's own name for this feature ("TRUST-Based Schedule-Adherence
Inference") maps directly onto that unused variant — confirming, exactly
as the brief expects, that this is a real, reserved-but-dormant slot, not
a currently-live pipeline stage.

**The validation findings' Task 8 verdict, re-run three times (2026-08-29,
2026-08-30, 2026-08-31/09-01), lands on "not yet" every time** — first
blocked by a live-instance SSO defect, then by `trust-consumer`'s
STANOX↔CRS gap (since fixed, commit `6adf64f`), and in the final,
most-complete run the *mechanism* is confirmed working end-to-end (a real
pin resolved against a real live train, a real cancellation was
TRUST-caught in real time that sampling's own output did not reflect
during the same window) but the empirical sample is explicitly judged too
small to carry a verdict — **1 of 1** real spot-checked disruption
instances in a ~35-minute monitoring window, against the plan's own
stated bar of a real **N of M** across enough real disruption days. The
validation findings' own words, restated because they are the load-bearing
conclusion for this section: *"the concrete next step is narrower than
both prior write-ups': ...re-run Task 4 onward with a real,
uninterrupted, multi-hour-or-longer monitoring window... the mechanism is
now proven, so this is purely a matter of giving it enough real
wall-clock time... Only then re-run Task 8 with an N of M large enough to
carry a confident verdict either way."*

**Would TRUST/full-schedule data materially change what per-station stats
could show?** Yes, and the design spec's own framing already names the
axis directly relevant here, not just to lines: *"knowing which stations
[are affected]... whereas a full-feed correlation would [cover every
scheduled service touching a line's TIPLOCs]"* (design spec, "What
'higher fidelity' actually buys," lines ~316-390, its own words: *"Coverage
of stations/segments Darwin sampling never reaches... most clearly better
in kind, not just degree"*). Concretely, for this document's specific
question: TRUST-plus-CIF-schedule data is keyed by TIPLOC/STANOX across a
**line's entire route**, not a curated 2-5-station subset — meaning it
would see every train's arrival at **every** calling point, not just the
286 CRS codes `poller-ldbws` currently reaches. That directly resolves
this document's own "Polling scope" finding above (the hard 286-station
ceiling under current LDBWS-only scope) **for free**, as a side effect of
data TRUST/CIF already carries, rather than requiring the separate
LDBWS-scope-broadening work that section names as its own real
prerequisite. It would not, on its own, resolve the multi-operator-mixing
question (a shared TIPLOC/station still serves multiple operators'
trains; that filtering question is orthogonal to which underlying feed
supplies the raw departures).

**Sequencing recommendation**: **independently, not blocked on TRUST, but
worth a light one-way check before committing to Option B (a new
per-station history table) specifically.** The reasoning:

- Per-station stats built against the current LDBWS-only 286-station
  scope (Options A or C above) are real, buildable, and useful today,
  entirely independent of whether TRUST/full-schedule ever ships — this
  document's "Where should this be computed" analysis does not depend on
  any TRUST fact.
- But if TRUST/full-schedule *does* eventually reach "go" (still an open,
  currently-"not yet" question per the validation findings, not this
  document's to resolve), the 286-station LDBWS ceiling would very likely
  be superseded by broader TIPLOC-based coverage for whichever
  lines/stations TRUST ends up covering first — meaning a heavier
  investment now (Option B's new table, or LDBWS-scope-broadening) risks
  being built against the smaller of two datasets shortly before a
  larger one might arrive. This is a real, named risk, not a decided
  blocker: TRUST is currently "not yet," with no committed timeline, and
  "wait for a not-yet-approved separate project" is not, by itself, a
  sound reason to delay a real, independently-useful feature indefinitely.
- The pragmatic middle ground, consistent with how 08-31/09-01 sequenced
  their own smaller, independent pieces of a related problem: **build the
  cheap, no-new-storage version (Option C) now if this is pursued at all,
  and treat the more committal Option B (a new per-station history table,
  or LDBWS scope-broadening) as the piece worth deliberately holding
  until TRUST's own Task 8 actually reaches a verdict** — not because
  per-station stats *needs* TRUST, but because the shape of a durable,
  schema-backed per-station history is exactly the kind of investment
  that would benefit from knowing which underlying feed it's really
  going to be fed by.

**Cross-reference note (per the brief's instruction, checked, not
blocking)**: `docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md`,
a parallel document reportedly covering the "sample vs. full-coverage
presentation" question generally, **does not exist yet** — checked at the
end of this research pass (`ls docs/superpowers/specs/ | grep
full-coverage` returns nothing as of this writing). No cross-reference is
made; a future reader completing this document's Recommendation should
check for it and reconcile the two if it has since landed, particularly
around how a "sample" vs. "full-coverage" data-quality label would apply
to a per-station (not just per-line) figure.

## Recommendation

**Research-stage recommendation, not a committed design**: if per-station
stats are pursued, start with **Option C** (compute on demand at read
time, reusing `latest_station_sample`, `crates/api/src/data/queries.rs:664`,
already shipping for a different feature) for a single, **operator-scoped**
number per (station, operator) pair — not an unfiltered per-station
blend, per the 53/286 finding above — scoped explicitly and honestly to
the ~286 CRS codes that are already someone's `sample_stations` entry,
with the station page saying so plainly for any other CRS (mirroring
`get_stop_point_disruption`'s existing "not covered" honesty precedent,
`crates/api/src/routes/line_status.rs:290-295`, and the station page's own
existing `coverage: 'none'` messaging,
`frontend/app/stations/[crs]/page.tsx:125-127`) rather than silently
showing nothing or a misleading zero. This gets a real, honest number in
front of users cheaply, using only already-proven code paths, without
committing to a new migration, a new aggregator write path, or a bet on
TRUST's still-undecided outcome.

Do **not** start with Option B (a new `station_status_daily_stats` table)
until either a concrete product need for per-station *history* (not just
a live snapshot) is confirmed, or TRUST's Task 8 reaches an actual
verdict — building durable per-station storage now risks being built
against the smaller of two possible future datasets (see "Relationship to
TRUST," above).

## Open questions — not resolved here

1. **Operator-scoped vs. unfiltered vs. both, for the actual UI.** This
   document establishes the 53/286 mixing problem is real and
   quantified, and leans toward operator-scoped in the Recommendation,
   but does not settle how a station page with 4-8 operators (Edinburgh,
   Newcastle, etc.) should actually render that — one row per operator,
   a picker, or something else — a real UX question, not a data one.
2. **Threshold source for a per-station computation.** No per-station
   `min_sample_size`/`delay_threshold_minutes` override concept exists
   today (`severity_overrides` is a `LineDefinition` field). Falling back
   to global `Defaults` is the only currently-available option; whether
   that's good enough, or whether some stations would need their own
   tuning the way some lines already do, is unexamined here.
3. **Skip-rate definition for a per-station number** — "skipped a stop
   somewhere on the route" (today's line-level meaning) vs. "skipped
   calling at this specific station" (the more natural per-station
   reading, and mechanically simpler — a direct `skipped_stations`
   membership check against one CRS) — flagged as a real semantic choice
   in "What needs restructuring" above, not decided.
4. **Whether broadening `poller-ldbws`'s scope beyond the current 286
   `sample_stations`-derived list is ever worth doing on its own**,
   independent of TRUST — this document names it as a real, separate,
   substantially-sized prerequisite for "per-station stats for any
   station" but does not scope or size that work (RDM rate limits, cycle
   time budget, whether RDM's terms permit polling stations no line
   currently curates).
5. **Whether `station_status`(`_daily_stats`) should be keyed by
   `(crs)`, `(crs, operator)`, or something else entirely** if Option
   B is ever pursued — this document raises the question via the
   operator-mixing finding but does not pick a key shape, since that
   choice should probably follow from however Open question 1 is
   answered on the frontend/product side, not be decided schema-first.
6. **Interaction with `docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md`**,
   confirmed not to exist as of this pass (see "Cross-reference note"
   above) — genuinely open until that document lands and can be read.
7. **Whether this is worth building at all right now** — this document
   was scoped as pure research per the brief ("what per-station stats
   would mean and take to build," not "should we"), and deliberately
   does not make a product-priority call; that is left to whoever reads
   this alongside the rest of this app's roadmap.

## References

- `crates/common/src/lib.rs:378-401` (`StationDeparture`), `:526-530`
  (`StationSample`), `:710-716` (`SampleStats`), `:717-734`
  (`SampleAvailability`, already implemented), `:284-294` (`DataQuality`,
  including `TrustInferred` at line 287).
- `crates/aggregator/src/aggregation.rs:794-810` (`relevant_departures`),
  `:816-844` (`stats_from_departures`), `:859-882`
  (`compute_sample_availability`), `:884-960` (`infer_from_samples`),
  `:983-1000` (`belongs_to_line`).
- `crates/aggregator/src/queries.rs:65-80` (`load_station_samples`).
- `crates/aggregator/src/dedup.rs:1-40` (module doc, per-line dedup
  ledger).
- `crates/aggregator/src/main.rs:175-186` (`lines_with_sample_coverage`
  gate).
- `crates/api/src/data/queries.rs:258-` (`upsert_station_samples`),
  `:525-` (`last_station_samples_fetch`), `:656-679`
  (`latest_station_sample`, already shipping, backs `eta_blend.rs`).
- `crates/api/src/routes/samples.rs:20-29`, `crates/api/src/data/samples.rs:11-23`
  (`dedup_sample_stations` — confirms `poller-ldbws`'s exact polling
  scope).
- `crates/api/src/routes/train.rs:524` (`latest_station_sample` call
  site).
- `crates/api/src/routes/line_status.rs:278-320` (`get_stop_point_disruption`).
- `crates/api/migrations/20260510023522_initial.sql:52-77`
  (`station_samples`, `line_status`).
- `crates/api/migrations/20260706004003_reference_data.sql:10-18`
  (`stations`).
- `crates/api/migrations/20260831090001_line_status_daily_stats.sql`
  (`line_status_daily_stats`, the Option B structural precedent).
- `crates/poller-ldbws/src/main.rs:1-17,51,123,160-`
  (`fetch_sample_stations`, module doc on polling scope).
- `crates/poller-stations/src/main.rs:1-9` (full RDM Stations feed,
  contrast with `poller-ldbws`'s narrower scope).
- `frontend/app/stations/[crs]/page.tsx:36-44,60-70,125-148`
  (`lookupStation`, `fetchStationDisruptions`, rendering).
- `frontend/lib/sampleStats.ts` (`representativeStatus`,
  `formatSampleSummary`).
- `docs/superpowers/specs/2026-08-31-sample-data-availability-design.md`
  (prior art on `sample_stats`/staleness ambiguity, line-scoped).
- `docs/superpowers/specs/2026-09-01-line-status-sample-coverage-design.md`
  (prior art on `SampleAvailability`, now implemented — see Corrections
  above).
- `docs/superpowers/specs/2026-08-31-station-catalogue-completeness-research.md`
  (structural precedent for this document's shape; also the source of the
  `line.stations` vs. `sample_stations` distinction relied on above).
- `docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`
  and `docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md`
  (TRUST/full-schedule line of work, read in full for the "Relationship to
  TRUST" section).
